pub mod api;
pub mod cas;
pub mod diff;
pub mod error;
pub mod hash;
pub mod manifest;

// Convenience re-exports for the most common entry points.
pub use api::{
    diff, gc, gc_dry_run, init, list, list_by_file, list_by_label, restore, restore_batch,
    snapshot, snapshot_dir, total_logical_bytes, update_label, update_label_by_batch, verify,
    verify_all, verify_batch, GcReport, VerifyReport,
};

pub use diff::DiffReport;
pub use error::{DvcError, Result};
pub use manifest::{Project, Snapshot};
