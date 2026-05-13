# design-version-client

> Local, library-first snapshot store for designer binary files.

Save and restore any version of your `.psd`, `.psb`, `.3dm`, `.pdf`, `.png`, or `.jpg` — no cloud, no daemon, no 800 MB memory spike.

## Status

**Milestone 1 complete** — walking skeleton is implemented and all tests pass. See `plan.md` for the full roadmap.

## Quick start (CLI)

```sh
# Build
cargo build --release

# Alias for convenience (optional)
alias dvc=./target/release/dvc

# 1. Initialise a store (creates .dvc/ in the current directory)
dvc init

# 2. Snapshot a file
dvc snapshot ~/Desktop/logo_v3.psd --label "before client review"
# Snapshot #1 — 2026-05-13T21:00:00Z (142.7 MiB) hash=ab3f1e2d...

# 3. Make changes, then snapshot again
dvc snapshot ~/Desktop/logo_v3.psd --label "after client review"
# Snapshot #2 — 2026-05-13T21:05:00Z (143.1 MiB) hash=cd91aa44...

# 4. List all snapshots
dvc list
# ID     Created                      Hash prefix      Size         Label
# ──────────────────────────────────────────────────────────────────────────────
# 1      2026-05-13T21:00:00Z         ab3f1e2d...      142.7 MiB    before client review
# 2      2026-05-13T21:05:00Z         cd91aa44...      143.1 MiB    after client review
# ──────────────────────────────────────────────────────────────────────────────
#   2 snapshot(s) — 285.8 MiB logical total

# 5. Restore an earlier version
dvc restore 1 ~/Desktop/logo_v3_restored.psd

# 6. Verify blob integrity
dvc verify 1

# 7. Garbage-collect orphaned blobs (after manually deleting snapshot records)
dvc gc
```

### Custom store path

By default the store lives at `.dvc/` relative to where you run the command.
Pass `--store` to override:

```sh
dvc --store /Volumes/ExternalSSD/design-snapshots init
dvc --store /Volumes/ExternalSSD/design-snapshots snapshot big-file.psb
```

## Architecture

See [`CONTEXT.md`](CONTEXT.md) for domain context and [`UML.md`](UML.md) for architecture diagrams.

Key decisions are in [`docs/adr/`](docs/adr/):
- [ADR-0000](docs/adr/0000-record-architecture-decisions.md) — recording decisions
- [ADR-0001](docs/adr/0001-storage-layout.md) — full-file CAS storage layout

## Running the tests

```sh
# Unit + integration tests (all fast)
cargo test

# Property tests are included — proptest runs 100 cases by default

# Large-file test (800 MB, needs a fast SSD)
RUN_LARGE_FILE_TESTS=1 cargo test -- --ignored large_file_800mb_snapshot_restore
```

## ⚠ Name clash note

The `dvc` binary name is taken by [iterative.ai's Data Version Control](https://dvc.org). Do not publish or distribute under this name. A new name is required before any public release (tracked in `plan.md` → Risks).

## Roadmap

See `plan.md` → Milestone 2 and 3 for planned work:
- Multi-file / directory snapshots
- Named labels first-class in CLI
- `dvc gc` improvements
- Cross-platform CI (macOS ✅, Windows ⬜, Linux ⬜)
- Binary rename
- Chunked CAS for inter-version dedup
- UI binding (Tauri / native plugin) — separate planning conversation
