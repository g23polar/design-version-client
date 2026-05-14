use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use design_version_core as dsv;

/// dsv — local design file snapshot tool
#[derive(Debug, Parser)]
#[command(name = "dsv", about = "Local snapshot store for designer binary files")]
struct Cli {
    /// Path to the dsv store directory (default: .dsv in the current directory)
    #[arg(long, short, global = true, default_value = ".dsv")]
    store: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialise a new dsv store
    Init,

    /// Snapshot a file or directory into the store
    Snapshot {
        /// Path to a file or directory to snapshot
        path: PathBuf,

        /// Optional human-readable label for this snapshot
        #[arg(long, short)]
        label: Option<String>,
    },

    /// List snapshots in the store
    List {
        /// Filter by label substring
        #[arg(long)]
        label: Option<String>,

        /// Filter by file path substring
        #[arg(long)]
        file: Option<String>,
    },

    /// Restore a snapshot (or batch) to a path
    Restore {
        /// Destination path to write the restored file/directory to
        out: PathBuf,

        /// Snapshot ID (from `dsv list`)
        #[arg(long, short)]
        id: Option<i64>,

        /// Restore all files in a batch (directory snapshot)
        #[arg(long)]
        batch: Option<String>,
    },

    /// Verify the integrity of snapshot blob(s)
    Verify {
        /// Snapshot ID. Omit to verify all blobs.
        id: Option<i64>,

        /// Verify all blobs in a batch
        #[arg(long)]
        batch: Option<String>,
    },

    /// Garbage-collect orphaned blobs
    Gc {
        /// Actually delete orphaned blobs (default is dry-run)
        #[arg(long)]
        confirm: bool,
    },

    /// Set or update a snapshot's label
    Label {
        /// New label text
        new_label: String,

        /// Snapshot ID
        #[arg(long, short)]
        id: Option<i64>,

        /// Apply label to all snapshots in a batch
        #[arg(long)]
        batch: Option<String>,
    },

    /// Compare two snapshots
    Diff {
        /// First snapshot ID
        id1: i64,

        /// Second snapshot ID
        id2: i64,
    },

    /// Delete a snapshot or batch
    Delete {
        /// Snapshot ID to delete
        #[arg(long, short)]
        id: Option<i64>,

        /// Delete all snapshots in a batch
        #[arg(long)]
        batch: Option<String>,

        /// Actually perform deletion (default is dry-run)
        #[arg(long)]
        confirm: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init => {
            let project = dsv::init(&cli.store)
                .with_context(|| format!("Failed to init store at {}", cli.store.display()))?;
            println!(
                "Initialised store at {} (project id={})",
                project.root_path, project.id
            );
        }

        Command::Snapshot { path, label } => {
            if path.is_dir() {
                let snaps = dsv::snapshot_dir(&cli.store, &path, label.as_deref())
                    .with_context(|| format!("Failed to snapshot directory {}", path.display()))?;
                let batch_id = snaps[0].batch_id.as_deref().unwrap_or("?");
                println!("Snapshot {} file(s) — batch {batch_id}", snaps.len());
                for snap in &snaps {
                    println!(
                        "  #{} {} ({}) hash={}",
                        snap.id,
                        snap.file_path,
                        format_bytes(snap.file_size),
                        &snap.blob_hash[..16]
                    );
                }
                if let Some(lbl) = &label {
                    println!("  label: {lbl}");
                }
            } else {
                let snap = dsv::snapshot(&cli.store, &path, label.as_deref())
                    .with_context(|| format!("Failed to snapshot {}", path.display()))?;
                println!(
                    "Snapshot #{} — {} ({}) hash={}",
                    snap.id,
                    snap.created_at,
                    format_bytes(snap.file_size),
                    &snap.blob_hash[..16]
                );
                if let Some(lbl) = &snap.label {
                    println!("  label: {lbl}");
                }
            }
        }

        Command::List { label, file } => {
            let snaps = if let Some(ref pat) = label {
                dsv::list_by_label(&cli.store, pat).with_context(|| "Failed to list snapshots")?
            } else if let Some(ref pat) = file {
                dsv::list_by_file(&cli.store, pat).with_context(|| "Failed to list snapshots")?
            } else {
                dsv::list(&cli.store).with_context(|| "Failed to list snapshots")?
            };

            if snaps.is_empty() {
                println!("No snapshots found.");
                return Ok(());
            }

            let total: u64 = snaps.iter().map(|s| s.file_size).sum();
            println!(
                "{:<6} {:<28} {:<16} {:<12} {:<14} Label",
                "ID", "Created", "Hash prefix", "Size", "Batch"
            );
            println!("{}", "-".repeat(90));
            for snap in &snaps {
                let batch_str = snap
                    .batch_id
                    .as_deref()
                    .map(|b| &b[..8.min(b.len())])
                    .unwrap_or("—");
                println!(
                    "{:<6} {:<28} {:<16} {:<12} {:<14} {}",
                    snap.id,
                    snap.created_at,
                    &snap.blob_hash[..16],
                    format_bytes(snap.file_size),
                    batch_str,
                    snap.label.as_deref().unwrap_or("—")
                );
            }
            println!("{}", "-".repeat(90));
            println!(
                "  {} snapshot(s) — {} logical total",
                snaps.len(),
                format_bytes(total)
            );
        }

        Command::Restore { out, id, batch } => {
            if let Some(batch_id) = batch {
                let count = dsv::restore_batch(&cli.store, &batch_id, &out)
                    .with_context(|| format!("Failed to restore batch {batch_id}"))?;
                println!(
                    "Restored {count} file(s) from batch {batch_id} → {}",
                    out.display()
                );
            } else if let Some(id) = id {
                dsv::restore(&cli.store, id, &out).with_context(|| {
                    format!("Failed to restore snapshot #{id} to {}", out.display())
                })?;
                println!("Restored snapshot #{id} → {}", out.display());
            } else {
                anyhow::bail!("Must specify either --id <ID> or --batch <BATCH_ID>");
            }
        }

        Command::Verify { id, batch } => {
            if let Some(batch_id) = batch {
                let report = dsv::verify_batch(&cli.store, &batch_id)
                    .with_context(|| format!("Failed to verify batch {batch_id}"))?;
                print_verify_report(&report);
            } else if let Some(id) = id {
                dsv::verify(&cli.store, id)
                    .with_context(|| format!("Failed to verify snapshot #{id}"))?;
                println!("Snapshot #{id}: OK");
            } else {
                let report =
                    dsv::verify_all(&cli.store).with_context(|| "Failed to verify store")?;
                print_verify_report(&report);
            }
        }

        Command::Gc { confirm } => {
            let report = dsv::gc(&cli.store, confirm).with_context(|| "GC failed")?;

            if report.orphaned_count == 0 {
                println!("Nothing to collect.");
            } else if confirm {
                println!(
                    "Deleted {} orphaned blob(s) ({})",
                    report.orphaned_count,
                    format_bytes(report.orphaned_bytes)
                );
            } else {
                println!(
                    "Found {} orphaned blob(s) ({}).",
                    report.orphaned_count,
                    format_bytes(report.orphaned_bytes)
                );
                println!("Run `dsv gc --confirm` to delete them.");
            }
        }

        Command::Label {
            new_label,
            id,
            batch,
        } => {
            if let Some(batch_id) = batch {
                let count = dsv::update_label_by_batch(&cli.store, &batch_id, &new_label)
                    .with_context(|| format!("Failed to update label for batch {batch_id}"))?;
                println!(
                    "Updated label to \"{new_label}\" on {count} snapshot(s) in batch {batch_id}"
                );
            } else if let Some(id) = id {
                dsv::update_label(&cli.store, id, &new_label)
                    .with_context(|| format!("Failed to update label for snapshot #{id}"))?;
                println!("Snapshot #{id}: label set to \"{new_label}\"");
            } else {
                anyhow::bail!("Must specify either --id <ID> or --batch <BATCH_ID>");
            }
        }

        Command::Diff { id1, id2 } => {
            let report = dsv::diff(&cli.store, id1, id2)
                .with_context(|| format!("Failed to diff snapshots #{id1} and #{id2}"))?;

            println!("Comparing snapshot #{id1} vs #{id2}\n");

            println!(
                "{:<18} {:<36} {:<36}",
                "",
                format!("#{id1}"),
                format!("#{id2}")
            );
            println!("{}", "-".repeat(90));
            println!(
                "{:<18} {:<36} {:<36}",
                "File", report.left.file_path, report.right.file_path
            );
            println!(
                "{:<18} {:<36} {:<36}",
                "Size",
                format_bytes(report.left.file_size),
                format_bytes(report.right.file_size)
            );
            println!(
                "{:<18} {:<36} {:<36}",
                "Hash",
                &report.left.blob_hash[..32],
                &report.right.blob_hash[..32]
            );
            println!(
                "{:<18} {:<36} {:<36}",
                "Label",
                report.left.label.as_deref().unwrap_or("—"),
                report.right.label.as_deref().unwrap_or("—")
            );
            println!(
                "{:<18} {:<36} {:<36}",
                "Created", report.left.created_at, report.right.created_at
            );
            println!("{}", "-".repeat(90));

            if report.same_content {
                println!("Content: IDENTICAL");
            } else {
                let sign = if report.size_delta_bytes >= 0 {
                    "+"
                } else {
                    ""
                };
                let pct = match report.size_delta_percent {
                    Some(p) => format!("{sign}{p:.1}%"),
                    None => "N/A".to_string(),
                };
                println!(
                    "Content: DIFFERENT — {sign}{} ({pct})",
                    format_bytes(report.size_delta_bytes.unsigned_abs())
                );
            }
        }

        Command::Delete { id, batch, confirm } => {
            if !confirm {
                println!("Dry-run mode. Use --confirm to actually delete.");
            }
            
            if let Some(batch_id) = batch {
                if confirm {
                    let report = dsv::delete_batch(&cli.store, &batch_id)
                        .with_context(|| format!("Failed to delete batch {batch_id}"))?;
                    println!(
                        "Deleted {} snapshot(s) and {} blob(s) ({} freed)",
                        report.snapshots_deleted,
                        report.blobs_deleted,
                        format_bytes(report.bytes_freed)
                    );
                } else {
                    let snaps = dsv::list_by_batch(&cli.store, &batch_id)
                        .with_context(|| format!("Failed to list batch {batch_id}"))?;
                    println!(
                        "Would delete {} snapshot(s) in batch {}",
                        snaps.len(),
                        &batch_id[..8.min(batch_id.len())]
                    );
                    for snap in &snaps {
                        println!("  #{} {} ({})", snap.id, snap.file_path, format_bytes(snap.file_size));
                    }
                }
            } else if let Some(id) = id {
                if confirm {
                    let report = dsv::delete_snapshot(&cli.store, id)
                        .with_context(|| format!("Failed to delete snapshot #{id}"))?;
                    println!(
                        "Deleted snapshot #{} and {} blob(s) ({} freed)",
                        id,
                        report.blobs_deleted,
                        format_bytes(report.bytes_freed)
                    );
                } else {
                    let snap = dsv::get_snapshot(&cli.store, id)
                        .with_context(|| format!("Failed to get snapshot #{id}"))?;
                    println!(
                        "Would delete snapshot #{} {} ({})",
                        id,
                        snap.file_path,
                        format_bytes(snap.file_size)
                    );
                }
            } else {
                anyhow::bail!("Must specify either --id <ID> or --batch <BATCH_ID>");
            }
        }
    }

    Ok(())
}

fn print_verify_report(report: &dsv::VerifyReport) {
    println!(
        "Checked {} blob(s): {} OK, {} corrupt, {} missing",
        report.checked,
        report.ok,
        report.corrupt.len(),
        report.missing.len()
    );
    for h in &report.corrupt {
        println!("  CORRUPT: {h}");
    }
    for h in &report.missing {
        println!("  MISSING: {h}");
    }
    if report.corrupt.is_empty() && report.missing.is_empty() {
        println!("All blobs OK.");
    }
}

fn format_bytes(n: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if n >= GIB {
        format!("{:.1} GiB", n as f64 / GIB as f64)
    } else if n >= MIB {
        format!("{:.1} MiB", n as f64 / MIB as f64)
    } else if n >= KIB {
        format!("{:.1} KiB", n as f64 / KIB as f64)
    } else {
        format!("{n} B")
    }
}
