# design-version-client

> Local, library-first snapshot store for designer binary files.

Save and restore any version of your `.psd`, `.psb`, `.3dm`, `.pdf`, `.png`, or `.jpg` — no cloud, no daemon, no 800 MB memory spike.

## Status

**Milestone 2 complete** — multi-file directory snapshots, label workflows, GC/verify improvements, diff, and CI.

## Quick start (CLI)

```sh
# Build
cargo build --release

# Alias for convenience (optional)
alias dsv=./target/release/dsv

# 1. Initialise a store (creates .dsv/ in the current directory)
dsv init

# 2. Snapshot a single file
dsv snapshot ~/Desktop/logo_v3.psd --label "before client review"

# 3. Snapshot an entire directory (all files grouped as a batch)
dsv snapshot ~/Desktop/project-assets/ --label "pre-handoff"

# 4. List all snapshots
dsv list

# 5. Filter by label or file name
dsv list --label "review"
dsv list --file "logo"

# 6. Restore a single snapshot
dsv restore 1 ~/Desktop/logo_v3_restored.psd

# 7. Restore a full batch (directory snapshot)
dsv restore --batch <batch-id> ~/Desktop/restored-assets/

# 8. Compare two snapshots
dsv diff 1 2

# 9. Update a snapshot's label
dsv label 1 "approved-by-client"

# 10. Verify blob integrity
dsv verify         # verify all blobs
dsv verify 1       # verify a single snapshot

# 11. Garbage-collect orphaned blobs
dsv gc             # dry-run: shows what would be deleted
dsv gc --confirm   # actually delete orphaned blobs
```

### Custom store path

By default the store lives at `.dsv/` relative to where you run the command.
Pass `--store` to override:

```sh
dsv --store /Volumes/ExternalSSD/design-snapshots init
dsv --store /Volumes/ExternalSSD/design-snapshots snapshot big-file.psb
```

## Documentation

- **[User Guide](docs/USER-GUIDE.md)** — Complete walkthrough for non-technical users
- **[Architecture](UML.md)** — Technical overview, module map, and data flows
- **[Context](CONTEXT.md)** — Domain background and design decisions
- **[ADRs](docs/adr/)** — Architecture decision records

## CLI Reference

| Command | Description |
|---|---|
| `dsv init` | Initialise a new store |
| `dsv snapshot <path> [--label TEXT]` | Snapshot a file or directory |
| `dsv list [--label PAT] [--file PAT]` | List snapshots, optionally filtered |
| `dsv restore <id> <out>` | Restore a single snapshot |
| `dsv restore --batch <id> <out_dir>` | Restore all files in a batch |
| `dsv diff <id1> <id2>` | Compare two snapshots (metadata + size delta) |
| `dsv label <id> <text>` | Set/update a snapshot's label |
| `dsv label --batch <id> <text>` | Label all snapshots in a batch |
| `dsv verify [id] [--batch ID]` | Verify blob integrity (all, one, or batch) |
| `dsv gc [--confirm]` | Garbage-collect orphaned blobs (dry-run by default) |

## Architecture

See [`CONTEXT.md`](CONTEXT.md) for domain context and [`UML.md`](UML.md) for architecture diagrams.

Key decisions are in [`docs/adr/`](docs/adr/):
- [ADR-0000](docs/adr/0000-record-architecture-decisions.md) — recording decisions
- [ADR-0001](docs/adr/0001-storage-layout.md) — full-file CAS storage layout

## Running the tests

```sh
# Unit + integration tests (53 tests)
cargo test

# Property tests are included — proptest runs 100 cases by default

# Large-file test (800 MB, needs a fast SSD)
RUN_LARGE_FILE_TESTS=1 cargo test -- --ignored large_file_800mb_snapshot_restore

# Lint
cargo clippy -- -D warnings
cargo fmt -- --check
```

## Roadmap

See `plan.md` for detailed planning.

### Done
- ✅ Milestone 1 — single-file snapshot/restore/verify/gc
- ✅ Milestone 2 — multi-file snapshots, labels, GC/verify improvements, diff, CI

### Future (Milestone 3+)
- Chunked CAS for inter-version dedup
- Watch mode / autosave
- UI binding (Tauri / native plugin) — separate planning conversation
- Cloud sync
