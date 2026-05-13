//! High-level public API — the stable surface that CLI and future UIs call.

use std::{
    fs,
    io::{self, BufWriter},
    path::{Path, PathBuf},
};

use crate::{
    cas,
    error::{DvcError, Result},
    hash::{encode_hex, hash_reader},
    manifest::{self, Project, Snapshot},
};

// ── Store layout helpers ───────────────────────────────────────────────────────

const MANIFEST_FILE: &str = "manifest.db";

fn manifest_path(store_root: &Path) -> PathBuf {
    store_root.join(MANIFEST_FILE)
}

// ── init ──────────────────────────────────────────────────────────────────────

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

// ── snapshot ──────────────────────────────────────────────────────────────────

/// Snapshot `file_path` into the store at `store_root`.
///
/// Streams the file into the CAS (single-pass hash + write), then records
/// the snapshot in the manifest.  Returns the new `Snapshot` record.
pub fn snapshot(
    store_root: &Path,
    file_path: &Path,
    label: Option<&str>,
) -> Result<Snapshot> {
    let conn = manifest::open(&manifest_path(store_root))?;
    let root_str = store_root
        .to_str()
        .ok_or_else(|| DvcError::InvalidArgument("store_root is not valid UTF-8".into()))?;

    let project = manifest::find_project_by_path(&conn, root_str)?.ok_or_else(|| {
        DvcError::NotFound(format!("project not initialised at {root_str}"))
    })?;

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

// ── list ──────────────────────────────────────────────────────────────────────

/// List all snapshots for the project at `store_root`, oldest first.
pub fn list(store_root: &Path) -> Result<Vec<Snapshot>> {
    let conn = manifest::open(&manifest_path(store_root))?;
    let root_str = store_root
        .to_str()
        .ok_or_else(|| DvcError::InvalidArgument("store_root is not valid UTF-8".into()))?;

    let project = manifest::find_project_by_path(&conn, root_str)?.ok_or_else(|| {
        DvcError::NotFound(format!("project not initialised at {root_str}"))
    })?;

    manifest::list_snapshots(&conn, project.id)
}

// ── restore ───────────────────────────────────────────────────────────────────

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

// ── verify ────────────────────────────────────────────────────────────────────

/// Verify that the blob for `snapshot_id` is intact (re-hash and compare).
///
/// Returns `Ok(())` if the blob matches the stored hash, or a
/// `DvcError::HashMismatch` / `DvcError::NotFound` otherwise.
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

// ── gc ────────────────────────────────────────────────────────────────────────

/// Garbage-collect orphaned blobs for the project at `store_root`.
///
/// Deletes any blob under `objects/` that is not referenced by at least one
/// snapshot in the manifest.  Returns the number of blobs deleted.
///
/// This is a best-effort scan — it only looks at the two-char prefix directories
/// that match the BLAKE3 hex alphabet.
pub fn gc(store_root: &Path) -> Result<u64> {
    let conn = manifest::open(&manifest_path(store_root))?;
    let root_str = store_root
        .to_str()
        .ok_or_else(|| DvcError::InvalidArgument("store_root is not valid UTF-8".into()))?;

    let project = manifest::find_project_by_path(&conn, root_str)?.ok_or_else(|| {
        DvcError::NotFound(format!("project not initialised at {root_str}"))
    })?;

    let referenced: std::collections::HashSet<String> =
        manifest::referenced_hashes(&conn, project.id)?
            .into_iter()
            .collect();

    let objects_dir = store_root.join("objects");
    if !objects_dir.exists() {
        return Ok(0);
    }

    let mut deleted = 0u64;

    for prefix_entry in fs::read_dir(&objects_dir)? {
        let prefix_entry = prefix_entry?;
        let prefix_path = prefix_entry.path();

        // Skip tmp/ and any non-directory.
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
                    cas::delete_blob(store_root, hash)?;
                    deleted += 1;
                }
            }
        }
    }

    Ok(deleted)
}

// ── Total storage helper ───────────────────────────────────────────────────────

/// Logical total bytes (sum of file_size for all snapshots, not deduplicated).
pub fn total_logical_bytes(store_root: &Path) -> Result<u64> {
    let conn = manifest::open(&manifest_path(store_root))?;
    let root_str = store_root
        .to_str()
        .ok_or_else(|| DvcError::InvalidArgument("store_root is not valid UTF-8".into()))?;

    let project = manifest::find_project_by_path(&conn, root_str)?.ok_or_else(|| {
        DvcError::NotFound(format!("project not initialised at {root_str}"))
    })?;

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
        // Second call must not fail.
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

        // Snapshot a file, then delete the snapshot record to orphan the blob.
        let fp = write_test_file(&files, "orphan.psd", b"soon orphaned");
        let snap = snapshot(store.path(), &fp, None).unwrap();
        let blob_hash = snap.blob_hash.clone();

        assert!(cas::blob_exists(store.path(), &blob_hash));

        // Remove the snapshot row directly via manifest.
        let conn = manifest::open(&manifest_path(store.path())).unwrap();
        manifest::delete_snapshot(&conn, snap.id).unwrap();
        drop(conn);

        let removed = gc(store.path()).unwrap();
        assert_eq!(removed, 1);
        assert!(!cas::blob_exists(store.path(), &blob_hash));
    }
}
