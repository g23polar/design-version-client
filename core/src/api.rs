//! High-level public API — the stable surface that CLI and future UIs call.

use std::{
    collections::HashSet,
    fs,
    io::{self, BufWriter},
    path::{Path, PathBuf},
};

use uuid::Uuid;
use walkdir::WalkDir;

use crate::{
    cas,
    diff::DiffReport,
    error::{DvcError, Result},
    hash::{encode_hex, hash_reader},
    manifest::{self, Project, Snapshot},
};

// -- Store layout helpers -----------------------------------------------------

const MANIFEST_FILE: &str = "manifest.db";

fn manifest_path(store_root: &Path) -> PathBuf {
    store_root.join(MANIFEST_FILE)
}

/// Open manifest + resolve project. Many API fns share this preamble.
fn open_project(store_root: &Path) -> Result<(rusqlite::Connection, Project)> {
    let conn = manifest::open(&manifest_path(store_root))?;
    let root_str = store_root
        .to_str()
        .ok_or_else(|| DvcError::InvalidArgument("store_root is not valid UTF-8".into()))?;
    let project = manifest::find_project_by_path(&conn, root_str)?
        .ok_or_else(|| DvcError::NotFound(format!("project not initialised at {root_str}")))?;
    Ok((conn, project))
}

// -- init ---------------------------------------------------------------------

/// Initialise a new project store at `store_root`.
///
/// Creates the directory tree, initialises the SQLite manifest, and
/// registers the project.  If a project is already registered at this path
/// the existing record is returned without error (idempotent).
pub fn init(store_root: &Path) -> Result<Project> {
    fs::create_dir_all(store_root)?;
    cas::init_store(store_root)?;
    cas::sweep_tmp(store_root)?;

    let conn = manifest::open(&manifest_path(store_root))?;
    let root_str = store_root
        .to_str()
        .ok_or_else(|| DvcError::InvalidArgument("store_root is not valid UTF-8".into()))?;

    let project = match manifest::find_project_by_path(&conn, root_str)? {
        Some(p) => p,
        None => {
            let id = manifest::insert_project(&conn, root_str)?;
            manifest::get_project(&conn, id)?
        }
    };

    Ok(project)
}

// -- snapshot (single file) ---------------------------------------------------

/// Snapshot `file_path` into the store at `store_root`.
///
/// Streams the file into the CAS (single-pass hash + write), then records
/// the snapshot in the manifest.  Returns the new `Snapshot` record.
pub fn snapshot(store_root: &Path, file_path: &Path, label: Option<&str>) -> Result<Snapshot> {
    let (conn, project) = open_project(store_root)?;

    let file = fs::File::open(file_path)?;
    let file_size = file.metadata()?.len();
    let blob_hash = cas::write_blob(store_root, file)?;

    let path_str = file_path
        .to_str()
        .ok_or_else(|| DvcError::InvalidArgument("file_path is not valid UTF-8".into()))?;

    let snap_id =
        manifest::insert_snapshot(&conn, project.id, path_str, &blob_hash, file_size, label)?;
    manifest::get_snapshot(&conn, snap_id)
}

// -- snapshot_dir (multi-file) ------------------------------------------------

/// Snapshot all files in `dir_path` into the store.
///
/// Walks the directory (skipping hidden files/dirs), snapshots each file,
/// and groups them under a shared `batch_id`.  If any file fails, all
/// snapshots and blobs for this batch are rolled back.
///
/// Returns the list of snapshots created.
pub fn snapshot_dir(
    store_root: &Path,
    dir_path: &Path,
    label: Option<&str>,
) -> Result<Vec<Snapshot>> {
    if !dir_path.is_dir() {
        return Err(DvcError::InvalidArgument(format!(
            "{} is not a directory",
            dir_path.display()
        )));
    }

    let (conn, project) = open_project(store_root)?;
    let batch_id = Uuid::new_v4().to_string();

    // Collect files first so we can report count / handle errors cleanly.
    let files: Vec<PathBuf> = WalkDir::new(dir_path)
        .into_iter()
        .filter_entry(|e| {
            // Skip hidden files/directories, but always allow the root entry.
            if e.depth() == 0 {
                return true;
            }
            e.file_name()
                .to_str()
                .map(|s| !s.starts_with('.'))
                .unwrap_or(false)
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();

    if files.is_empty() {
        return Err(DvcError::InvalidArgument(format!(
            "no files found in {}",
            dir_path.display()
        )));
    }

    let mut snap_ids: Vec<i64> = Vec::with_capacity(files.len());
    let mut written_hashes: Vec<String> = Vec::new();

    for file_path in &files {
        let result = (|| -> Result<i64> {
            let file = fs::File::open(file_path)?;
            let file_size = file.metadata()?.len();
            let blob_hash = cas::write_blob(store_root, file)?;

            let rel_path = file_path.strip_prefix(dir_path).unwrap_or(file_path);
            let path_str = rel_path
                .to_str()
                .ok_or_else(|| DvcError::InvalidArgument("file_path is not valid UTF-8".into()))?;

            written_hashes.push(blob_hash.clone());
            let sid = manifest::insert_snapshot_with_batch(
                &conn,
                project.id,
                path_str,
                &blob_hash,
                file_size,
                label,
                Some(&batch_id),
            )?;
            Ok(sid)
        })();

        match result {
            Ok(sid) => snap_ids.push(sid),
            Err(e) => {
                // Rollback: delete manifest rows and orphaned blobs.
                for sid in &snap_ids {
                    let _ = manifest::delete_snapshot(&conn, *sid);
                }
                // Only delete blobs that aren't referenced by other snapshots.
                let referenced: HashSet<String> = manifest::referenced_hashes(&conn, project.id)?
                    .into_iter()
                    .collect();
                for hash in &written_hashes {
                    if !referenced.contains(hash) {
                        let _ = cas::delete_blob(store_root, hash);
                    }
                }
                return Err(e);
            }
        }
    }

    let mut snapshots = Vec::with_capacity(snap_ids.len());
    for sid in snap_ids {
        snapshots.push(manifest::get_snapshot(&conn, sid)?);
    }
    Ok(snapshots)
}

// -- list ---------------------------------------------------------------------

/// List all snapshots for the project at `store_root`, oldest first.
pub fn list(store_root: &Path) -> Result<Vec<Snapshot>> {
    let (conn, project) = open_project(store_root)?;
    manifest::list_snapshots(&conn, project.id)
}

/// List snapshots filtered by label substring.
pub fn list_by_label(store_root: &Path, pattern: &str) -> Result<Vec<Snapshot>> {
    let (conn, project) = open_project(store_root)?;
    manifest::list_snapshots_by_label(&conn, project.id, pattern)
}

/// List snapshots filtered by file path substring.
pub fn list_by_file(store_root: &Path, pattern: &str) -> Result<Vec<Snapshot>> {
    let (conn, project) = open_project(store_root)?;
    manifest::list_snapshots_by_file(&conn, project.id, pattern)
}

/// List all snapshots in a specific batch.
pub fn list_by_batch(store_root: &Path, batch_id: &str) -> Result<Vec<Snapshot>> {
    let conn = manifest::open(&manifest_path(store_root))?;
    manifest::list_snapshots_by_batch(&conn, batch_id)
}

/// Get a specific snapshot by ID.
pub fn get_snapshot(store_root: &Path, snapshot_id: i64) -> Result<Snapshot> {
    let conn = manifest::open(&manifest_path(store_root))?;
    manifest::get_snapshot(&conn, snapshot_id)
}

// -- label --------------------------------------------------------------------

/// Update the label on a single snapshot.
pub fn update_label(store_root: &Path, snapshot_id: i64, new_label: &str) -> Result<()> {
    let conn = manifest::open(&manifest_path(store_root))?;
    manifest::update_label(&conn, snapshot_id, new_label)
}

/// Update the label on all snapshots in a batch.
pub fn update_label_by_batch(store_root: &Path, batch_id: &str, new_label: &str) -> Result<u64> {
    let conn = manifest::open(&manifest_path(store_root))?;
    manifest::update_label_by_batch(&conn, batch_id, new_label)
}

// -- restore ------------------------------------------------------------------

/// Restore snapshot `snapshot_id` to `out_path`.
///
/// Verifies the blob hash before writing to `out_path`.  If verification
/// fails the destination file is NOT written and an error is returned.
pub fn restore(store_root: &Path, snapshot_id: i64, out_path: &Path) -> Result<()> {
    let conn = manifest::open(&manifest_path(store_root))?;
    let snap = manifest::get_snapshot(&conn, snapshot_id)?;

    // Verify blob integrity before writing out.
    let reader = cas::read_blob(store_root, &snap.blob_hash)?;
    let actual_digest = hash_reader(reader)?;
    let actual_hex = encode_hex(&actual_digest);

    if actual_hex != snap.blob_hash {
        return Err(DvcError::HashMismatch {
            expected: snap.blob_hash.clone(),
            actual: actual_hex,
        });
    }

    // Write verified blob to destination.
    let reader = cas::read_blob(store_root, &snap.blob_hash)?;
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let out_file = fs::File::create(out_path)?;
    let mut writer = BufWriter::new(out_file);
    let mut reader = reader;
    io::copy(&mut reader, &mut writer)?;

    Ok(())
}

/// Restore all snapshots in a batch to `out_dir`, preserving relative paths.
pub fn restore_batch(store_root: &Path, batch_id: &str, out_dir: &Path) -> Result<u64> {
    let conn = manifest::open(&manifest_path(store_root))?;
    let snaps = manifest::list_snapshots_by_batch(&conn, batch_id)?;

    if snaps.is_empty() {
        return Err(DvcError::NotFound(format!("batch {batch_id}")));
    }

    for snap in &snaps {
        let out_path = out_dir.join(&snap.file_path);
        // Verify blob integrity.
        let reader = cas::read_blob(store_root, &snap.blob_hash)?;
        let actual_digest = hash_reader(reader)?;
        let actual_hex = encode_hex(&actual_digest);

        if actual_hex != snap.blob_hash {
            return Err(DvcError::HashMismatch {
                expected: snap.blob_hash.clone(),
                actual: actual_hex,
            });
        }

        let reader = cas::read_blob(store_root, &snap.blob_hash)?;
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let out_file = fs::File::create(&out_path)?;
        let mut writer = BufWriter::new(out_file);
        let mut reader = reader;
        io::copy(&mut reader, &mut writer)?;
    }

    Ok(snaps.len() as u64)
}

// -- verify -------------------------------------------------------------------

/// Verify that the blob for `snapshot_id` is intact (re-hash and compare).
pub fn verify(store_root: &Path, snapshot_id: i64) -> Result<()> {
    let conn = manifest::open(&manifest_path(store_root))?;
    let snap = manifest::get_snapshot(&conn, snapshot_id)?;

    if !cas::blob_exists(store_root, &snap.blob_hash) {
        return Err(DvcError::NotFound(format!(
            "blob {} for snapshot {}",
            snap.blob_hash, snapshot_id
        )));
    }

    let reader = cas::read_blob(store_root, &snap.blob_hash)?;
    let actual_digest = hash_reader(reader)?;
    let actual_hex = encode_hex(&actual_digest);

    if actual_hex != snap.blob_hash {
        return Err(DvcError::HashMismatch {
            expected: snap.blob_hash,
            actual: actual_hex,
        });
    }

    Ok(())
}

/// Report from verify_all / verify_batch.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VerifyReport {
    pub checked: u64,
    pub ok: u64,
    pub corrupt: Vec<String>,
    pub missing: Vec<String>,
}

/// Verify all blobs referenced by any snapshot in the store.
pub fn verify_all(store_root: &Path) -> Result<VerifyReport> {
    let (conn, project) = open_project(store_root)?;
    let hashes = manifest::referenced_hashes(&conn, project.id)?;
    verify_hashes(store_root, &hashes)
}

/// Verify all blobs in a specific batch.
pub fn verify_batch(store_root: &Path, batch_id: &str) -> Result<VerifyReport> {
    let conn = manifest::open(&manifest_path(store_root))?;
    let snaps = manifest::list_snapshots_by_batch(&conn, batch_id)?;
    let hashes: Vec<String> = snaps
        .into_iter()
        .map(|s| s.blob_hash)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    verify_hashes(store_root, &hashes)
}

fn verify_hashes(store_root: &Path, hashes: &[String]) -> Result<VerifyReport> {
    let mut report = VerifyReport {
        checked: 0,
        ok: 0,
        corrupt: Vec::new(),
        missing: Vec::new(),
    };

    for hash in hashes {
        report.checked += 1;

        if !cas::blob_exists(store_root, hash) {
            report.missing.push(hash.clone());
            continue;
        }

        let reader = cas::read_blob(store_root, hash)?;
        let actual_digest = hash_reader(reader)?;
        let actual_hex = encode_hex(&actual_digest);

        if actual_hex == *hash {
            report.ok += 1;
        } else {
            report.corrupt.push(hash.clone());
        }
    }

    Ok(report)
}

// -- diff ---------------------------------------------------------------------

/// Compare two snapshots by id. Pure metadata — no blob I/O.
pub fn diff(store_root: &Path, id_a: i64, id_b: i64) -> Result<DiffReport> {
    let conn = manifest::open(&manifest_path(store_root))?;
    let snap_a = manifest::get_snapshot(&conn, id_a)?;
    let snap_b = manifest::get_snapshot(&conn, id_b)?;
    Ok(DiffReport::from_snapshots(snap_a, snap_b))
}

// -- gc -----------------------------------------------------------------------

/// GC dry-run report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GcReport {
    /// Number of orphaned blobs found.
    pub orphaned_count: u64,
    /// Total size of orphaned blobs in bytes.
    pub orphaned_bytes: u64,
    /// Hashes of orphaned blobs.
    pub orphaned_hashes: Vec<String>,
}

/// Scan for orphaned blobs without deleting anything.
pub fn gc_dry_run(store_root: &Path) -> Result<GcReport> {
    let (orphaned, _) = find_orphaned_blobs(store_root)?;

    let mut total_bytes = 0u64;
    for hash in &orphaned {
        if let Some(size) = cas::blob_size(store_root, hash)? {
            total_bytes += size;
        }
    }

    Ok(GcReport {
        orphaned_count: orphaned.len() as u64,
        orphaned_bytes: total_bytes,
        orphaned_hashes: orphaned,
    })
}

/// Garbage-collect orphaned blobs for the project at `store_root`.
///
/// If `confirm` is false, acts as a dry-run (returns the report without deleting).
/// If `confirm` is true, deletes orphaned blobs and returns the report.
pub fn gc(store_root: &Path, confirm: bool) -> Result<GcReport> {
    let report = gc_dry_run(store_root)?;

    if confirm {
        for hash in &report.orphaned_hashes {
            cas::delete_blob(store_root, hash)?;
        }
    }

    Ok(report)
}

fn find_orphaned_blobs(store_root: &Path) -> Result<(Vec<String>, HashSet<String>)> {
    let (conn, project) = open_project(store_root)?;
    let referenced: HashSet<String> = manifest::referenced_hashes(&conn, project.id)?
        .into_iter()
        .collect();

    let objects_dir = store_root.join("objects");
    if !objects_dir.exists() {
        return Ok((Vec::new(), referenced));
    }

    let mut orphaned = Vec::new();

    for prefix_entry in fs::read_dir(&objects_dir)? {
        let prefix_entry = prefix_entry?;
        let prefix_path = prefix_entry.path();

        if !prefix_path.is_dir() {
            continue;
        }
        if prefix_path.file_name().and_then(|n| n.to_str()) == Some("tmp") {
            continue;
        }

        for blob_entry in fs::read_dir(&prefix_path)? {
            let blob_entry = blob_entry?;
            if let Some(hash) = blob_entry.file_name().to_str() {
                if !referenced.contains(hash) {
                    orphaned.push(hash.to_string());
                }
            }
        }
    }

    Ok((orphaned, referenced))
}

// -- Delete operations -----------------------------------------------------

/// Report from delete operations.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeleteReport {
    pub snapshots_deleted: u64,
    pub blobs_deleted: u64,
    pub bytes_freed: u64,
}

/// Delete a specific snapshot and clean up orphaned blobs.
pub fn delete_snapshot(store_root: &Path, snapshot_id: i64) -> Result<DeleteReport> {
    let conn = manifest::open(&manifest_path(store_root))?;

    // Get snapshot info before deletion for reporting
    let snapshot = manifest::get_snapshot(&conn, snapshot_id)?;
    let blob_hash = snapshot.blob_hash.clone();

    // Delete snapshot from manifest
    manifest::delete_snapshot(&conn, snapshot_id)?;

    // Check if the blob is now orphaned and delete it if so
    let (conn, project) = open_project(store_root)?;
    let referenced = manifest::referenced_hashes(&conn, project.id)?;

    let mut bytes_freed = 0;
    let blobs_deleted = if !referenced.contains(&blob_hash) {
        // Blob is orphaned, delete it
        if let Ok(Some(size)) = cas::blob_size(store_root, &blob_hash) {
            bytes_freed = size;
        }
        cas::delete_blob(store_root, &blob_hash)?;
        1
    } else {
        0
    };

    Ok(DeleteReport {
        snapshots_deleted: 1,
        blobs_deleted,
        bytes_freed,
    })
}

/// Delete all snapshots in a batch and clean up orphaned blobs.
pub fn delete_batch(store_root: &Path, batch_id: &str) -> Result<DeleteReport> {
    let conn = manifest::open(&manifest_path(store_root))?;

    // Get all snapshots in the batch before deletion
    let snapshots = manifest::list_snapshots_by_batch(&conn, batch_id)?;
    let blob_hashes: Vec<String> = snapshots.iter().map(|s| s.blob_hash.clone()).collect();
    let _snapshot_count = snapshots.len() as u64;

    // Delete all snapshots in the batch
    let deleted_count = manifest::delete_snapshots_by_batch(&conn, batch_id)?;

    // Check which blobs are now orphaned
    let (conn, project) = open_project(store_root)?;
    let referenced = manifest::referenced_hashes(&conn, project.id)?;

    let mut bytes_freed = 0;
    let mut blobs_deleted = 0;

    for blob_hash in blob_hashes {
        if !referenced.contains(&blob_hash) {
            // Blob is orphaned, delete it
            if let Ok(Some(size)) = cas::blob_size(store_root, &blob_hash) {
                bytes_freed += size;
            }
            if cas::delete_blob(store_root, &blob_hash).is_ok() {
                blobs_deleted += 1;
            }
        }
    }

    Ok(DeleteReport {
        snapshots_deleted: deleted_count,
        blobs_deleted,
        bytes_freed,
    })
}

// -- Total storage helper -----------------------------------------------------

/// Logical total bytes (sum of file_size for all snapshots, not deduplicated).
pub fn total_logical_bytes(store_root: &Path) -> Result<u64> {
    let (conn, project) = open_project(store_root)?;
    manifest::total_logical_bytes(&conn, project.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn make_store() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path()).unwrap();
        dir
    }

    fn write_test_file(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content).unwrap();
        path
    }

    #[test]
    fn init_is_idempotent() {
        let store = make_store();
        init(store.path()).unwrap();
    }

    #[test]
    fn snapshot_and_list() {
        let store = make_store();
        let files = tempfile::tempdir().unwrap();
        let fp = write_test_file(&files, "design.psd", b"fake psd content");

        let snap = snapshot(store.path(), &fp, Some("initial save")).unwrap();
        assert_eq!(snap.label.as_deref(), Some("initial save"));
        assert_eq!(snap.file_size, b"fake psd content".len() as u64);

        let snaps = list(store.path()).unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].id, snap.id);
    }

    #[test]
    fn verify_passes_for_good_blob() {
        let store = make_store();
        let files = tempfile::tempdir().unwrap();
        let fp = write_test_file(&files, "a.png", b"pixel data");
        let snap = snapshot(store.path(), &fp, None).unwrap();
        verify(store.path(), snap.id).unwrap();
    }

    #[test]
    fn gc_removes_orphaned_blob() {
        let store = make_store();
        let files = tempfile::tempdir().unwrap();

        let fp = write_test_file(&files, "orphan.psd", b"soon orphaned");
        let snap = snapshot(store.path(), &fp, None).unwrap();
        let blob_hash = snap.blob_hash.clone();

        assert!(cas::blob_exists(store.path(), &blob_hash));

        let conn = manifest::open(&manifest_path(store.path())).unwrap();
        manifest::delete_snapshot(&conn, snap.id).unwrap();
        drop(conn);

        // Dry run should find the orphan but not delete it.
        let report = gc_dry_run(store.path()).unwrap();
        assert_eq!(report.orphaned_count, 1);
        assert!(cas::blob_exists(store.path(), &blob_hash));

        // Confirm = false should not delete.
        let report = gc(store.path(), false).unwrap();
        assert_eq!(report.orphaned_count, 1);
        assert!(cas::blob_exists(store.path(), &blob_hash));

        // Confirm = true should delete.
        let report = gc(store.path(), true).unwrap();
        assert_eq!(report.orphaned_count, 1);
        assert!(!cas::blob_exists(store.path(), &blob_hash));
    }

    #[test]
    fn diff_same_content() {
        let store = make_store();
        let files = tempfile::tempdir().unwrap();
        let fp = write_test_file(&files, "a.psd", b"same content");

        let s1 = snapshot(store.path(), &fp, Some("first")).unwrap();
        let s2 = snapshot(store.path(), &fp, Some("second")).unwrap();

        let report = diff(store.path(), s1.id, s2.id).unwrap();
        assert!(report.same_content);
        assert_eq!(report.size_delta_bytes, 0);
    }

    #[test]
    fn diff_different_content() {
        let store = make_store();
        let files = tempfile::tempdir().unwrap();

        let fp1 = write_test_file(&files, "a.psd", b"short");
        let fp2 = write_test_file(&files, "b.psd", b"a longer file content here");

        let s1 = snapshot(store.path(), &fp1, None).unwrap();
        let s2 = snapshot(store.path(), &fp2, None).unwrap();

        let report = diff(store.path(), s1.id, s2.id).unwrap();
        assert!(!report.same_content);
        assert!(report.size_delta_bytes > 0);
        assert!(report.size_delta_percent.unwrap() > 0.0);
    }

    #[test]
    fn diff_nonexistent_snapshot() {
        let store = make_store();
        assert!(diff(store.path(), 999, 1000).is_err());
    }

    #[test]
    fn verify_all_reports() {
        let store = make_store();
        let files = tempfile::tempdir().unwrap();
        let fp1 = write_test_file(&files, "a.psd", b"file a");
        let fp2 = write_test_file(&files, "b.psd", b"file b");

        snapshot(store.path(), &fp1, None).unwrap();
        snapshot(store.path(), &fp2, None).unwrap();

        let report = verify_all(store.path()).unwrap();
        assert_eq!(report.checked, 2);
        assert_eq!(report.ok, 2);
        assert!(report.corrupt.is_empty());
        assert!(report.missing.is_empty());
    }

    #[test]
    fn label_filtering() {
        let store = make_store();
        let files = tempfile::tempdir().unwrap();
        let fp = write_test_file(&files, "a.psd", b"content");

        snapshot(store.path(), &fp, Some("before-review")).unwrap();
        snapshot(store.path(), &fp, Some("after-review")).unwrap();
        snapshot(store.path(), &fp, None).unwrap();

        let results = list_by_label(store.path(), "review").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn file_filtering() {
        let store = make_store();
        let files = tempfile::tempdir().unwrap();
        let fp1 = write_test_file(&files, "logo.psd", b"logo");
        let fp2 = write_test_file(&files, "banner.png", b"banner");

        snapshot(store.path(), &fp1, None).unwrap();
        snapshot(store.path(), &fp2, None).unwrap();

        let results = list_by_file(store.path(), "logo").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn update_label_works() {
        let store = make_store();
        let files = tempfile::tempdir().unwrap();
        let fp = write_test_file(&files, "a.psd", b"data");
        let snap = snapshot(store.path(), &fp, Some("old")).unwrap();

        update_label(store.path(), snap.id, "new").unwrap();
        let snaps = list(store.path()).unwrap();
        assert_eq!(snaps[0].label.as_deref(), Some("new"));
    }

    #[test]
    fn snapshot_dir_round_trip() {
        let store = make_store();
        let dir = tempfile::tempdir().unwrap();

        // Create a directory structure.
        let sub = dir.path().join("subdir");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.path().join("root.psd"), b"root file").unwrap();
        fs::write(sub.join("nested.png"), b"nested file").unwrap();

        let snaps = snapshot_dir(store.path(), dir.path(), Some("batch-test")).unwrap();
        assert_eq!(snaps.len(), 2);
        assert!(snaps[0].batch_id.is_some());
        let batch_id = snaps[0].batch_id.as_ref().unwrap().clone();
        assert_eq!(snaps[1].batch_id.as_deref(), Some(batch_id.as_str()));

        // Restore the batch.
        let out = tempfile::tempdir().unwrap();
        let count = restore_batch(store.path(), &batch_id, out.path()).unwrap();
        assert_eq!(count, 2);

        // Verify content.
        for snap in &snaps {
            let restored = out.path().join(&snap.file_path);
            assert!(restored.exists(), "missing: {}", snap.file_path);
        }
    }

    #[test]
    fn snapshot_dir_skips_hidden() {
        let store = make_store();
        let dir = tempfile::tempdir().unwrap();

        fs::write(dir.path().join("visible.psd"), b"yes").unwrap();
        fs::write(dir.path().join(".hidden"), b"no").unwrap();
        let dot_dir = dir.path().join(".git");
        fs::create_dir_all(&dot_dir).unwrap();
        fs::write(dot_dir.join("config"), b"no").unwrap();

        let snaps = snapshot_dir(store.path(), dir.path(), None).unwrap();
        assert_eq!(snaps.len(), 1);
        assert!(snaps[0].file_path.contains("visible"));
    }
}
