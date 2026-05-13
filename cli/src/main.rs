use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use design_version_core as dsv;

/// dsv — local design file snapshot tool (v1)
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

    /// Snapshot a file into the store
    Snapshot {
        /// Path to the file to snapshot
        file: PathBuf,

        /// Optional human-readable label for this snapshot
        #[arg(long, short)]
        label: Option<String>,
    },

    /// List snapshots in the store
    List,

    /// Restore a snapshot to a path
    Restore {
        /// Snapshot ID (from `dsv list`)
        id: i64,

        /// Destination path to write the restored file to
        out: PathBuf,
    },

    /// Verify the integrity of a snapshot's blob
    Verify {
        /// Snapshot ID
        id: i64,
    },

    /// Garbage-collect orphaned blobs
    Gc,
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

        Command::Snapshot { file, label } => {
            let snap = dsv::snapshot(&cli.store, &file, label.as_deref())
                .with_context(|| format!("Failed to snapshot {}", file.display()))?;
            println!(
                "Snapshot #{} — {} ({} bytes) hash={}",
                snap.id,
                snap.created_at,
                snap.file_size,
                &snap.blob_hash[..16]
            );
            if let Some(lbl) = &snap.label {
                println!("  label: {lbl}");
            }
        }

        Command::List => {
            let snaps = dsv::list(&cli.store)
                .with_context(|| "Failed to list snapshots")?;

            if snaps.is_empty() {
                println!("No snapshots yet. Run `dsv snapshot <file>` to capture one.");
                return Ok(());
            }

            let total = dsv::total_logical_bytes(&cli.store).unwrap_or(0);
            println!("{:<6} {:<28} {:<16} {:<12} {}", "ID", "Created", "Hash prefix", "Size", "Label");
            println!("{}", "-".repeat(80));
            for snap in &snaps {
                println!(
                    "{:<6} {:<28} {:<16} {:<12} {}",
                    snap.id,
                    snap.created_at,
                    &snap.blob_hash[..16],
                    format_bytes(snap.file_size),
                    snap.label.as_deref().unwrap_or("—")
                );
            }
            println!("{}", "-".repeat(80));
            println!(
                "  {} snapshot(s) — {} logical total",
                snaps.len(),
                format_bytes(total)
            );
        }

        Command::Restore { id, out } => {
            dsv::restore(&cli.store, id, &out)
                .with_context(|| format!("Failed to restore snapshot #{id} to {}", out.display()))?;
            println!("Restored snapshot #{id} → {}", out.display());
        }

        Command::Verify { id } => {
            dsv::verify(&cli.store, id)
                .with_context(|| format!("Failed to verify snapshot #{id}"))?;
            println!("Snapshot #{id}: OK");
        }

        Command::Gc => {
            let deleted = dsv::gc(&cli.store)
                .with_context(|| "GC failed")?;
            if deleted == 0 {
                println!("Nothing to collect.");
            } else {
                println!("Deleted {deleted} orphaned blob(s).");
            }
        }
    }

    Ok(())
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
