//! Diff report between two snapshots — metadata + byte-level stats.

use crate::manifest::Snapshot;

/// Side-by-side comparison of two snapshots.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffReport {
    pub left: Snapshot,
    pub right: Snapshot,
    /// True if both snapshots reference the same blob (identical content).
    pub same_content: bool,
    /// `right.file_size as i64 - left.file_size as i64` (positive = grew).
    pub size_delta_bytes: i64,
    /// Percentage change: `(delta / left_size) * 100`. `None` if left size is 0.
    pub size_delta_percent: Option<f64>,
}

impl DiffReport {
    pub fn from_snapshots(left: Snapshot, right: Snapshot) -> Self {
        let same_content = left.blob_hash == right.blob_hash;
        let size_delta_bytes = right.file_size as i64 - left.file_size as i64;
        let size_delta_percent = if left.file_size == 0 {
            None
        } else {
            Some((size_delta_bytes as f64 / left.file_size as f64) * 100.0)
        };
        Self {
            left,
            right,
            same_content,
            size_delta_bytes,
            size_delta_percent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_snap(id: i64, hash: &str, size: u64) -> Snapshot {
        Snapshot {
            id,
            project_id: 1,
            file_path: "test.psd".into(),
            blob_hash: hash.into(),
            file_size: size,
            label: None,
            batch_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn same_content() {
        let a = fake_snap(1, "aaa", 1000);
        let b = fake_snap(2, "aaa", 1000);
        let report = DiffReport::from_snapshots(a, b);
        assert!(report.same_content);
        assert_eq!(report.size_delta_bytes, 0);
        assert_eq!(report.size_delta_percent, Some(0.0));
    }

    #[test]
    fn different_content() {
        let a = fake_snap(1, "aaa", 1000);
        let b = fake_snap(2, "bbb", 1200);
        let report = DiffReport::from_snapshots(a, b);
        assert!(!report.same_content);
        assert_eq!(report.size_delta_bytes, 200);
        assert!((report.size_delta_percent.unwrap() - 20.0).abs() < 0.01);
    }

    #[test]
    fn shrink() {
        let a = fake_snap(1, "aaa", 1000);
        let b = fake_snap(2, "bbb", 800);
        let report = DiffReport::from_snapshots(a, b);
        assert_eq!(report.size_delta_bytes, -200);
        assert!((report.size_delta_percent.unwrap() - (-20.0)).abs() < 0.01);
    }

    #[test]
    fn zero_size_left() {
        let a = fake_snap(1, "aaa", 0);
        let b = fake_snap(2, "bbb", 500);
        let report = DiffReport::from_snapshots(a, b);
        assert!(report.size_delta_percent.is_none());
    }
}
