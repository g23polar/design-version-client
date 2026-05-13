//! Integration tests — require the full workspace to be built.
//!
//! Large-file tests (≥800 MB) are gated behind `RUN_LARGE_FILE_TESTS=1`.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use design_version_core::{
    diff, gc, gc_dry_run, init, list, list_by_file, list_by_label, restore, restore_batch,
    snapshot, snapshot_dir, update_label, verify, verify_all, DvcError,
};

// -- helpers ------------------------------------------------------------------

fn make_store() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path()).unwrap();
    dir
}

fn write_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut f = fs::File::create(&p).unwrap();
    f.write_all(content).unwrap();
    p
}

// -- small-file round-trip ----------------------------------------------------

#[test]
fn small_file_snapshot_restore_byte_identical() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();

    let original: Vec<u8> = (0u64..10 * 1024)
        .map(|i| {
            (i.wrapping_mul(6364136223846793005u64)
                .wrapping_add(1442695040888963407u64)
                & 0xFF) as u8
        })
        .collect();

    let src = write_file(files.path(), "design.psd", &original);
    let snap = snapshot(store.path(), &src, Some("baseline")).unwrap();

    let restored = files.path().join("design_restored.psd");
    restore(store.path(), snap.id, &restored).unwrap();

    let restored_bytes = fs::read(&restored).unwrap();
    assert_eq!(
        original, restored_bytes,
        "restored bytes must be identical to original"
    );
}

// -- multiple snapshots -------------------------------------------------------

#[test]
fn multiple_snapshots_listed_in_order() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();

    let versions = ["v1 content", "v2 content", "v3 content"];
    let mut ids = Vec::new();

    for (i, content) in versions.iter().enumerate() {
        let src = write_file(files.path(), &format!("f{i}.psd"), content.as_bytes());
        let snap = snapshot(store.path(), &src, Some(&format!("v{}", i + 1))).unwrap();
        ids.push(snap.id);
    }

    let snaps = list(store.path()).unwrap();
    assert_eq!(snaps.len(), 3);
    for (snap, expected_label) in snaps.iter().zip(["v1", "v2", "v3"]) {
        assert_eq!(snap.label.as_deref(), Some(expected_label));
    }
}

// -- deduplication ------------------------------------------------------------

#[test]
fn identical_content_stored_once() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();

    let content = b"same bytes every time";
    let s1_path = write_file(files.path(), "f1.psd", content);
    let s2_path = write_file(files.path(), "f2.psd", content);

    let snap1 = snapshot(store.path(), &s1_path, None).unwrap();
    let snap2 = snapshot(store.path(), &s2_path, None).unwrap();

    assert_eq!(snap1.blob_hash, snap2.blob_hash);
}

// -- verify -------------------------------------------------------------------

#[test]
fn verify_passes_on_intact_blob() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();
    let src = write_file(files.path(), "ok.png", b"pixel data here");
    let snap = snapshot(store.path(), &src, None).unwrap();
    verify(store.path(), snap.id).unwrap();
}

#[test]
fn verify_fails_on_missing_snapshot() {
    let store = make_store();
    let err = verify(store.path(), 9999).unwrap_err();
    assert!(matches!(err, DvcError::NotFound(_)));
}

#[test]
fn verify_all_reports_all_blobs() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();
    write_file(files.path(), "a.psd", b"aaa");
    write_file(files.path(), "b.psd", b"bbb");
    snapshot(store.path(), &files.path().join("a.psd"), None).unwrap();
    snapshot(store.path(), &files.path().join("b.psd"), None).unwrap();

    let report = verify_all(store.path()).unwrap();
    assert_eq!(report.checked, 2);
    assert_eq!(report.ok, 2);
    assert!(report.corrupt.is_empty());
    assert!(report.missing.is_empty());
}

// -- gc -----------------------------------------------------------------------

#[test]
fn gc_on_empty_store_is_zero() {
    let store = make_store();
    let report = gc(store.path(), true).unwrap();
    assert_eq!(report.orphaned_count, 0);
}

#[test]
fn gc_does_not_delete_referenced_blobs() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();
    let src = write_file(files.path(), "keep.psd", b"keep me around");
    let snap = snapshot(store.path(), &src, None).unwrap();
    let hash = snap.blob_hash.clone();

    let report = gc(store.path(), true).unwrap();
    assert_eq!(report.orphaned_count, 0);
    assert!(design_version_core::cas::blob_exists(store.path(), &hash));
}

#[test]
fn gc_dry_run_does_not_delete() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();
    let src = write_file(files.path(), "orphan.psd", b"orphan");
    let snap = snapshot(store.path(), &src, None).unwrap();
    let hash = snap.blob_hash.clone();

    // Delete the snapshot to orphan the blob.
    let conn = design_version_core::manifest::open(&store.path().join("manifest.db")).unwrap();
    design_version_core::manifest::delete_snapshot(&conn, snap.id).unwrap();
    drop(conn);

    let report = gc_dry_run(store.path()).unwrap();
    assert_eq!(report.orphaned_count, 1);
    assert!(report.orphaned_bytes > 0);
    // Blob should still exist.
    assert!(design_version_core::cas::blob_exists(store.path(), &hash));

    // Now actually delete.
    let report = gc(store.path(), true).unwrap();
    assert_eq!(report.orphaned_count, 1);
    assert!(!design_version_core::cas::blob_exists(store.path(), &hash));
}

// -- diff ---------------------------------------------------------------------

#[test]
fn diff_same_content() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();
    let src = write_file(files.path(), "a.psd", b"same");
    let s1 = snapshot(store.path(), &src, Some("first")).unwrap();
    let s2 = snapshot(store.path(), &src, Some("second")).unwrap();

    let report = diff(store.path(), s1.id, s2.id).unwrap();
    assert!(report.same_content);
    assert_eq!(report.size_delta_bytes, 0);
}

#[test]
fn diff_different_content() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();
    let src1 = write_file(files.path(), "a.psd", b"short");
    let src2 = write_file(
        files.path(),
        "b.psd",
        b"a much longer file with more content",
    );
    let s1 = snapshot(store.path(), &src1, None).unwrap();
    let s2 = snapshot(store.path(), &src2, None).unwrap();

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

// -- labels -------------------------------------------------------------------

#[test]
fn label_filtering() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();
    let src = write_file(files.path(), "a.psd", b"content");

    snapshot(store.path(), &src, Some("before-review")).unwrap();
    snapshot(store.path(), &src, Some("after-review")).unwrap();
    snapshot(store.path(), &src, None).unwrap();

    let results = list_by_label(store.path(), "review").unwrap();
    assert_eq!(results.len(), 2);

    let results = list_by_label(store.path(), "before").unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn file_filtering() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();
    let src1 = write_file(files.path(), "logo.psd", b"logo");
    let src2 = write_file(files.path(), "banner.png", b"banner");

    snapshot(store.path(), &src1, None).unwrap();
    snapshot(store.path(), &src2, None).unwrap();

    let results = list_by_file(store.path(), "logo").unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].file_path.contains("logo"));
}

#[test]
fn update_label_works() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();
    let src = write_file(files.path(), "a.psd", b"data");
    let snap = snapshot(store.path(), &src, Some("old")).unwrap();

    update_label(store.path(), snap.id, "new").unwrap();
    let snaps = list(store.path()).unwrap();
    assert_eq!(snaps[0].label.as_deref(), Some("new"));
}

// -- directory snapshots ------------------------------------------------------

#[test]
fn snapshot_dir_round_trip() {
    let store = make_store();
    let dir = tempfile::tempdir().unwrap();

    fs::create_dir_all(dir.path().join("subdir")).unwrap();
    fs::write(dir.path().join("root.psd"), b"root file").unwrap();
    fs::write(dir.path().join("subdir/nested.png"), b"nested file").unwrap();

    let snaps = snapshot_dir(store.path(), dir.path(), Some("batch-test")).unwrap();
    assert_eq!(snaps.len(), 2);
    assert!(snaps[0].batch_id.is_some());
    let batch_id = snaps[0].batch_id.as_ref().unwrap().clone();
    assert_eq!(snaps[1].batch_id.as_deref(), Some(batch_id.as_str()));

    // Restore the batch.
    let out = tempfile::tempdir().unwrap();
    let count = restore_batch(store.path(), &batch_id, out.path()).unwrap();
    assert_eq!(count, 2);

    for snap in &snaps {
        let restored_path = out.path().join(&snap.file_path);
        assert!(restored_path.exists(), "missing: {}", snap.file_path);
    }
}

#[test]
fn snapshot_dir_skips_hidden_files() {
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

#[test]
fn snapshot_dir_preserves_relative_paths() {
    let store = make_store();
    let dir = tempfile::tempdir().unwrap();

    fs::create_dir_all(dir.path().join("assets/icons")).unwrap();
    fs::write(dir.path().join("main.psd"), b"main").unwrap();
    fs::write(dir.path().join("assets/icons/logo.png"), b"logo").unwrap();

    let snaps = snapshot_dir(store.path(), dir.path(), None).unwrap();
    let paths: Vec<&str> = snaps.iter().map(|s| s.file_path.as_str()).collect();
    assert!(paths.contains(&"main.psd"));
    assert!(paths.contains(&"assets/icons/logo.png"));
}

// -- proptest -----------------------------------------------------------------

proptest::proptest! {
    #[test]
    fn random_bytes_snapshot_restore_equal(data in proptest::collection::vec(proptest::num::u8::ANY, 0..64_000)) {
        let store = make_store();
        let files = tempfile::tempdir().unwrap();
        let src = write_file(files.path(), "rand.bin", &data);
        let snap = snapshot(store.path(), &src, None).unwrap();
        let out = files.path().join("rand_out.bin");
        restore(store.path(), snap.id, &out).unwrap();
        let restored = fs::read(&out).unwrap();
        proptest::prop_assert_eq!(data, restored);
    }
}

// -- large file test (gated) --------------------------------------------------

#[test]
#[ignore = "set RUN_LARGE_FILE_TESTS=1 and run with -- --ignored to enable"]
fn large_file_800mb_snapshot_restore() {
    if std::env::var("RUN_LARGE_FILE_TESTS").unwrap_or_default() != "1" {
        return;
    }

    const SIZE: usize = 800 * 1024 * 1024;
    let store = make_store();
    let files = tempfile::tempdir().unwrap();

    let src = files.path().join("large.psd");
    {
        let f = fs::File::create(&src).unwrap();
        let mut w = std::io::BufWriter::new(f);
        let chunk = vec![0xABu8; 256 * 1024];
        let mut written = 0;
        while written < SIZE {
            let n = chunk.len().min(SIZE - written);
            w.write_all(&chunk[..n]).unwrap();
            written += n;
        }
    }

    let start = std::time::Instant::now();
    let snap = snapshot(store.path(), &src, Some("800mb test")).unwrap();
    let snap_elapsed = start.elapsed();

    let out = files.path().join("large_restored.psd");
    let start2 = std::time::Instant::now();
    restore(store.path(), snap.id, &out).unwrap();
    let restore_elapsed = start2.elapsed();

    println!("snapshot: {snap_elapsed:.2?}  restore: {restore_elapsed:.2?}");
    assert!(
        snap_elapsed.as_secs() <= 5,
        "snapshot took >5s: {snap_elapsed:.2?}"
    );
    assert!(
        restore_elapsed.as_secs() <= 5,
        "restore took >5s: {restore_elapsed:.2?}"
    );
    assert_eq!(snap.file_size, SIZE as u64);
}
