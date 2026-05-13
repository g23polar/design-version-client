use std::io::{self, Read};

use crate::error::Result;

/// Hash all bytes from `reader` using BLAKE3, streaming — never fully buffered.
/// Returns the 32-byte digest.
pub fn hash_reader<R: Read>(mut reader: R) -> Result<[u8; 32]> {
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024]; // 256 KiB chunks
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(*hasher.finalize().as_bytes())
}

/// Encode a 32-byte digest as a lowercase hex string (64 chars).
pub fn encode_hex(digest: &[u8; 32]) -> String {
    hex::encode(digest)
}

/// Decode a 64-char hex string back to 32 bytes.
/// Returns None if the string is not valid hex of the right length.
pub fn decode_hex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let bytes = hex::decode(s).ok()?;
    bytes.try_into().ok()
}

/// A writer that simultaneously feeds bytes into a BLAKE3 hasher and a wrapped writer.
/// Use this for single-pass hash+write without reading the source twice.
pub struct HashWriter<W: io::Write> {
    inner: W,
    hasher: blake3::Hasher,
}

impl<W: io::Write> HashWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: blake3::Hasher::new(),
        }
    }

    /// Finalise and return the digest alongside the inner writer.
    pub fn finish(self) -> ([u8; 32], W) {
        (*self.hasher.finalize().as_bytes(), self.inner)
    }
}

impl<W: io::Write> io::Write for HashWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn hash_reader_deterministic() {
        let data = b"hello design versioning";
        let d1 = hash_reader(Cursor::new(data)).unwrap();
        let d2 = hash_reader(Cursor::new(data)).unwrap();
        assert_eq!(d1, d2);
    }

    #[test]
    fn hash_reader_differs_on_different_data() {
        let d1 = hash_reader(Cursor::new(b"aaa")).unwrap();
        let d2 = hash_reader(Cursor::new(b"bbb")).unwrap();
        assert_ne!(d1, d2);
    }

    #[test]
    fn hex_round_trip() {
        let data = b"round trip test";
        let digest = hash_reader(Cursor::new(data)).unwrap();
        let hex = encode_hex(&digest);
        assert_eq!(hex.len(), 64);
        let back = decode_hex(&hex).unwrap();
        assert_eq!(back, digest);
    }

    #[test]
    fn hash_writer_matches_hash_reader() {
        let data = b"test data for tee writer";
        let expected = hash_reader(Cursor::new(data)).unwrap();

        let mut out = Vec::new();
        let mut hw = HashWriter::new(&mut out);
        std::io::copy(&mut Cursor::new(data), &mut hw).unwrap();
        let (digest, _) = hw.finish();

        assert_eq!(digest, expected);
        assert_eq!(out.as_slice(), data);
    }
}
