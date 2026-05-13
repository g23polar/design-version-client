use std::{
    fs,
    io::{self, BufReader, BufWriter, Read},
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::{
    error::{DvcError, Result},
    hash::{encode_hex, HashWriter},
};

/// Root of the object store relative to the store root.
const OBJECTS_DIR: &str = "objects";
const TMP_DIR: &str = "objects/tmp";

fn tmp_dir(store_root: &Path) -> PathBuf {
    store_root.join(TMP_DIR)
}

fn blob_path(store_root: &Path, hex_hash: &str) -> PathBuf {
    debug_assert!(hex_hash.len() == 64, "hash must be 64 hex chars");
    let prefix = &hex_hash[..2];
    store_root.join(OBJECTS_DIR).join(prefix).join(hex_hash)
}

/// Ensure the objects/ and objects/tmp/ directories exist.
pub fn init_store(store_root: &Path) -> Result<()> {
    fs::create_dir_all(tmp_dir(store_root))?;
    Ok(())
}

/// Sweep stale tmp files (present before this process started).
/// Called on store open so crashed in-flight writes don't accumulate.
pub fn sweep_tmp(store_root: &Path) -> Result<()> {
    let tmp = tmp_dir(store_root);
    if !tmp.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&tmp)? {
        let entry = entry?;
        // Best-effort removal; ignore individual errors.
        let _ = fs::remove_file(entry.path());
    }
    Ok(())
}

/// Write `reader` into the CAS.  Returns the hex-encoded BLAKE3 hash.
/// If a blob with that hash already exists, the write is a no-op and the
/// hash is returned immediately (deduplication).
///
/// Write strategy: tmp → fsync → rename (atomic on POSIX; best-effort on Windows).
pub fn write_blob<R: Read>(store_root: &Path, reader: R) -> Result<String> {
    init_store(store_root)?;

    let tmp_path = tmp_dir(store_root).join(Uuid::new_v4().to_string());
    let tmp_file = fs::File::create(&tmp_path)?;
    let buf_writer = BufWriter::new(tmp_file);
    let mut hash_writer = HashWriter::new(buf_writer);

    let mut buf_reader = BufReader::new(reader);
    io::copy(&mut buf_reader, &mut hash_writer)?;

    let (digest, buf_writer) = hash_writer.finish();
    // Flush and fsync before rename.
    let file = buf_writer.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    drop(file);

    let hex = encode_hex(&digest);

    // Check for existing blob — no-op if already present.
    let dest = blob_path(store_root, &hex);
    if dest.exists() {
        let _ = fs::remove_file(&tmp_path);
        return Ok(hex);
    }

    // Ensure the two-char prefix directory exists.
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    // Atomic rename.
    fs::rename(&tmp_path, &dest)?;

    Ok(hex)
}

/// Open a blob for reading.  Returns a `BufReader` over the blob file.
pub fn read_blob(store_root: &Path, hex_hash: &str) -> Result<BufReader<fs::File>> {
    let path = blob_path(store_root, hex_hash);
    if !path.exists() {
        return Err(DvcError::NotFound(format!("blob {hex_hash}")));
    }
    Ok(BufReader::new(fs::File::open(path)?))
}

/// Returns true if a blob with this hash exists in the store.
pub fn blob_exists(store_root: &Path, hex_hash: &str) -> bool {
    blob_path(store_root, hex_hash).exists()
}

/// Delete a blob from the store.  No-op if the blob doesn't exist.
pub fn delete_blob(store_root: &Path, hex_hash: &str) -> Result<()> {
    let path = blob_path(store_root, hex_hash);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Return the on-disk size of a blob in bytes, or None if not present.
pub fn blob_size(store_root: &Path, hex_hash: &str) -> Result<Option<u64>> {
    let path = blob_path(store_root, hex_hash);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(fs::metadata(path)?.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::TempDir;

    fn tmp_store() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn write_and_read_blob() {
        let dir = tmp_store();
        let data = b"some designer file contents";
        let hash = write_blob(dir.path(), Cursor::new(data)).unwrap();
        assert_eq!(hash.len(), 64);

        let mut reader = read_blob(dir.path(), &hash).unwrap();
        let mut out = Vec::new();
        io::copy(&mut reader, &mut out).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn deduplication() {
        let dir = tmp_store();
        let data = b"duplicate content";
        let h1 = write_blob(dir.path(), Cursor::new(data)).unwrap();
        let h2 = write_blob(dir.path(), Cursor::new(data)).unwrap();
        assert_eq!(h1, h2);

        // Only one blob file should exist.
        let prefix = &h1[..2];
        let blob_dir = dir.path().join(OBJECTS_DIR).join(prefix);
        let count = fs::read_dir(blob_dir).unwrap().count();
        assert_eq!(count, 1);
    }

    #[test]
    fn blob_exists_and_delete() {
        let dir = tmp_store();
        let data = b"to be deleted";
        let hash = write_blob(dir.path(), Cursor::new(data)).unwrap();
        assert!(blob_exists(dir.path(), &hash));
        delete_blob(dir.path(), &hash).unwrap();
        assert!(!blob_exists(dir.path(), &hash));
    }

    #[test]
    fn read_missing_blob_returns_not_found() {
        let dir = tmp_store();
        let fake = "a".repeat(64);
        let err = read_blob(dir.path(), &fake).unwrap_err();
        assert!(matches!(err, DvcError::NotFound(_)));
    }
}
