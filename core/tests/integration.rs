//! Integration tests — require the full workspace to be built.
//!
//! Large-file tests (≥800 MB) are gated behind `RUN_LARGE_FILE_TESTS=1`.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use design_version_core::{gc, init, list, restore, snapshot, verify, DvcError};

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_store() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path()).unwrap();
    dir
}

fn write_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
    let p = dir.join(name);
    let mut f = fs::File::create(&p).unwrap();
    f.write_all(content).unwrap();
    p
}

// ── small-file round-trip ────────────────────────────────────────────────────

#[test]
fn small_file_snapshot_restore_byte_identical() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();

    // 10 KB of pseudo-random-ish bytes.
    let original: Vec<u8> = (0u64..10 * 1024)
        .map(|i| (i.wrapping_mul(6364136223846793005u64).wrapping_add(1442695040888963407u64) & 0xFF) as u8)
        .collect();

    let src = write_file(files.path(), "design.psd", &original);
    let snap = snapshot(store.path(), &src, Some("baseline")).unwrap();

    let restored = files.path().join("design_restored.psd");
    restore(store.path(), snap.id, &restored).unwrap();

    let restored_bytes = fs::read(&restored).unwrap();
    assert_eq!(original, restored_bytes, "restored bytes must be identical to original");
}

// ── multiple snapshots ─────────────────────────────────────────────────────────

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

// ── deduplication ──────────────────────────────────────────────────────────────

#[test]
fn identical_content_stored_once() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();

    let content = b"same bytes every time";
    let s1_path = write_file(files.path(), "f1.psd", content);
    let s2_path = write_file(files.path(), "f2.psd", content);

    let snap1 = snapshot(store.path(), &s1_path, None).unwrap();
    let snap2 = snapshot(store.path(), &s2_path, None).unwrap();

    // Both snapshots must reference the same blob.
    assert_eq!(snap1.blob_hash, snap2.blob_hash);
}

// ── verify ─────────────────────────────────────────────────────────────────────

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

// ── gc ──────────────────────────────────────────────────────────────────────

#[test]
fn gc_on_empty_store_is_zero() {
    let store = make_store();
    let deleted = gc(store.path()).unwrap();
    assert_eq!(deleted, 0);
}

#[test]
fn gc_does_not_delete_referenced_blobs() {
    let store = make_store();
    let files = tempfile::tempdir().unwrap();
    let src = write_file(files.path(), "keep.psd", b"keep me around");
    let snap = snapshot(store.path(), &src, None).unwrap();
    let hash = snap.blob_hash.clone();

    let deleted = gc(store.path()).unwrap();
    assert_eq!(deleted, 0);
    assert!(design_version_core::cas::blob_exists(store.path(), &hash));
}

// ── proptest ──────────────────────────────────────────────────────────────────

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

// ── large file test (gated) ───────────────────────────────────────────────────

#[test]
#[ignore = "set RUN_LARGE_FILE_TESTS=1 and run with -- --ignored to enable"]
fn large_file_800mb_snapshot_restore() {
    if std::env::var("RUN_LARGE_FILE_TESTS").unwrap_or_default() != "1" {
        return;
    }

    const SIZE: usize = 800 * 1024 * 1024;
    let store = make_store();
    let files = tempfile::tempdir().unwrap();

    // Write 800 MB of patterned bytes without allocating them all at once.
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
