# Plan: design-version-client v1 — backend

**Status:** Milestone 1 implemented — 2026-05-13.

## Goal

Build a local, library-first snapshot store for designer file workflows. Each "snapshot" captures a single file's full byte content under a content-addressed hash; designers can list and restore prior versions. v1 targets a single-file workflow on macOS + Windows (+ Linux nice-to-have), with a thin CLI for manual validation. The UI (Electron / library consumer / other) is deliberately deferred until the backend is stable.

## Context

- **Files in scope**: `.psd`, `.psb`, `.3dm`, `.pdf`, `.png`, `.jpg`. Max individual file size ~800MB.
- **Semantics**: snapshot/restore only. No branching, no merging.
- **Storage strategy**: full-file content-addressable store (Option A). Chunked CAS / binary deltas are out of scope for v1 but the data model must not preclude them.
- **Hash**: BLAKE3.
- **Index**: SQLite (embedded, via `rusqlite`).
- **Runtime**: Rust workspace — `core` library crate + `cli` binary crate.
- **No daemon. No network. Strictly local in v1.**

Decisions confirmed via the `new-project-bootstrap` deep-dive on 2026-05-13.

## Approach

- **Two-crate workspace.** `core` exposes the public API; `cli` is a thin binary used for manual testing during v1. Future UIs (Electron via Tauri / N-API, native library consumer) call `core` directly without going through `cli`.
- **Streaming everywhere.** Never load an 800MB file into memory. `io::copy` between `BufReader` and `BufWriter`; BLAKE3's `Hasher::update_reader` (or equivalent streaming wrapper) for hashing.
- **Atomic blob writes.** Write to `objects/tmp/<uuid>`, fsync, rename to `objects/<hash[0..2]>/<hash>`. Survives crash mid-write. Two-char prefix sharding avoids 100k-file flat directories.
- **SQLite manifest.** Tables: `projects (id, root_path, created_at)`, `snapshots (id, project_id, file_path, blob_hash, file_size, label, created_at)`. Single-writer assumption — v1 is single-process.
- **Verify on restore.** Re-hash blob on restore; abort if it doesn't match the stored hash. Trust-but-verify; protects against silent disk corruption.
- **Explicit GC only.** No automatic pruning in v1. User invokes `dvc gc` to delete orphaned blobs. Designer trust > disk efficiency.

**Alternatives considered and rejected (for v1):**

- **Chunked CAS / binary deltas** — higher complexity for unreliable savings on opaque binary formats (PSDs re-compress throughout on small edits). The full-file CAS data model leaves room to add chunking later under the same manifest, so this is a deferral, not a foreclosure.
- **SHA-256** — ~5× slower than BLAKE3 on 800MB binaries. The standards argument doesn't apply to a local-only tool.
- **Async runtime (`tokio`)** — operations are sequential and short-lived; no concurrency need in v1. Adding `tokio` now would be premature complexity.
- **Single binary, no library split** — couples future UIs to CLI argument shapes. Library-first preserves optionality.

## Changes

- `Cargo.toml` — workspace root; `members = ["core", "cli"]`.
- `core/Cargo.toml` — deps: `blake3`, `rusqlite` (`bundled` feature), `hex`, `thiserror`, `uuid`. Dev-deps: `tempfile`, `proptest`.
- `core/src/lib.rs` — public API surface: `init`, `snapshot`, `list`, `restore`, `verify`, `gc`.
- `core/src/cas.rs` — content-addressable store: `write_blob`, `read_blob`, `blob_exists`, `delete_blob`.
- `core/src/hash.rs` — BLAKE3 streaming wrapper; hex encoding helpers.
- `core/src/manifest.rs` — SQLite schema, migration runner, CRUD for projects + snapshots.
- `core/src/error.rs` — error type tree (`thiserror`).
- `cli/Cargo.toml` — deps: `core` (path), `clap` (derive feature), `anyhow`.
- `cli/src/main.rs` — clap subcommands: `init`, `snapshot`, `list`, `restore`, `verify`, `gc`.
- `core/tests/integration.rs` — round-trip tests (snapshot → restore → byte-identical); large-file test gated by `RUN_LARGE_FILE_TESTS=1`.
- `README.md` — replace placeholder with v1 quickstart for manual CLI testing.
- `CONTEXT.md` — fill Purpose, Domain Glossary, System Shape, Conventions.

## Risks / Open Questions

- **Windows atomic rename semantics.** `std::fs::rename` over an existing target is not atomic on Windows the same way it is on Unix. May need `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` via the `windows` crate. Test on Windows CI early.
- **fsync portability.** `File::sync_all()` behavior varies across platforms. Document the durability story; consider a partial-write detector (verify blob hash post-write before recording in manifest) as a belt-and-braces measure.
- **800MB I/O budget.** Target ≤5s save. BLAKE3 + disk copy on SSD should clear this with margin. Bench early. Prefer a single-pass `tee`-style splitter (hash while writing) over a two-pass read-then-copy.
- **Storage growth.** 50 saves × 800MB = 40GB on a single file. Designers will not accept this silently. Surface storage usage in `dvc list` output and document `dvc gc` clearly.
- **No multi-file projects in v1.** A real "design project" is often a directory of linked files (PSD + linked PNGs). The data model accommodates this (manifest stores `file_path`), but the v1 API exposes one file at a time. Multi-file is the first fast-follow.
- **No content-type validation.** v1 trusts the user's file path. No magic-byte checks. Acceptable for v1.
- **Name clash with DVC (iterative.ai).** "DVC" is taken by Data Version Control (a popular ML tooling project). `dvc` as a binary name will create confusion in any external context. Pick a different binary name before public release. Open question, not a v1 blocker.
- **Storage decision deserves an ADR.** ADR-0001 — Storage layout (Option A + migration path to chunked/delta). Should be opened before implementation, not after.

## Checklist

### Milestone 1 — walking skeleton

- [x] Cargo workspace: root `Cargo.toml`, `core/`, `cli/` scaffolds
- [x] `core::error` — error type with variants for IO, hash mismatch, manifest, not-found
- [x] `core::hash` — `hash_reader<R: Read>(reader) -> Result<[u8; 32]>` (streaming)
- [x] `core::cas` — `write_blob`, `read_blob`, `blob_exists`, atomic-rename write path, two-char sharding
- [x] `core::manifest` — schema, migration runner, CRUD for projects + snapshots
- [x] `core::api` — `init(root)`, `snapshot(project, file_path, label)`, `list(project)`, `restore(snapshot_id, out_path)`
- [x] `cli` — clap subcommands wired to `core` API
- [x] Unit tests: hash determinism, manifest round-trip
- [x] Integration test: small-file (10KB) snapshot/restore — byte-identical
- [x] Integration test (gated): 800MB snapshot/restore — byte-identical, ≤5s on SSD
- [x] Property test (`proptest`): random bytes → snapshot → restore → equality
- [x] `README.md` — replace placeholder with manual quickstart
- [x] `CONTEXT.md` — fill Purpose, Glossary, System Shape, Conventions
- [x] ADR-0001 — storage layout decision (Option A + migration path)
- [x] `UML.md` — generate after Milestone 1 runs, per `uml-maintenance` skill

### Milestone 2 — fast-follow (not in this implementation pass)

- [ ] Multi-file project snapshots (directory snapshot)
- [ ] Named snapshots / labels first-class in CLI
- [ ] `dvc gc` (prune orphaned blobs)
- [ ] `dvc verify` (re-hash all blobs, report corruption)
- [ ] Cross-platform CI (macOS, Windows, Linux)
- [ ] Rename binary (resolve `dvc` clash with iterative.ai)

### Milestone 3+ — future

- [ ] Chunked CAS for inter-version dedup
- [ ] Watch mode / autosave
- [ ] UI binding decision (Electron via Tauri vs. library consumer vs. native plugin) — separate planning conversation
- [ ] Cloud sync (separate architecture; out of v1 scope)
