# CONTEXT

> Living document. Update whenever the domain language or system shape changes.
> AI agents read this first to orient. Keep it skimmable.

## Purpose

`design-version-client` is a **local, library-first snapshot store for designer binary files** (`.psd`, `.psb`, `.3dm`, `.pdf`, `.png`, `.jpg`). Designers accumulate destructive edits inside large opaque binaries; existing version-control tools either choke on files ≥100MB or demand a network back-end. This tool captures full-file snapshots under a content-addressed hash (BLAKE3), stores them in a local SQLite manifest, and lets designers restore any prior state — all with zero network dependency and a sub-5-second save budget for 800MB files.

v1 targets a single-file workflow on macOS (primary) and Windows (required), with Linux as a nice-to-have. A thin CLI (`dvc`) serves as the manual-testing entry point; the real consumer surface is the `core` library crate, which future UIs (Tauri/Electron, native plugins) will call directly.

## Domain Glossary

| Term | Meaning |
|------|---------|
| **snapshot** | An immutable, point-in-time capture of a single file's full byte content, identified by its BLAKE3 hash and recorded in the manifest. |
| **project** | A logical grouping rooted at a directory path. Snapshots belong to a project. v1 supports one file per project; multi-file is a fast-follow. |
| **blob** | The raw file bytes stored in the content-addressable object store under `objects/<hash[0..2]>/<hash>`. One blob per unique content, regardless of how many snapshots reference it. |
| **manifest** | The SQLite database (`manifest.db`) that records project metadata and the snapshot history. The source of truth for `list` and `restore`. |
| **CAS** | Content-Addressable Store — the `objects/` directory tree where blobs live, keyed by hash. Guarantees deduplication across identical saves. |
| **restore** | Copying a blob back to a target path, with hash verification before overwriting the destination. |
| **GC** | Garbage collection — deleting blobs in `objects/` that no snapshot in the manifest references. User-initiated only in v1. |
| **label** | A human-readable tag attached to a snapshot at capture time (e.g., `"before-client-review"`). Optional; defaults to timestamp. |
| **atomic write** | The write strategy for blobs: write to `objects/tmp/<uuid>`, fsync, then rename into place. Survives a crash mid-write without leaving a corrupt blob. |

## System Shape

```
┌─────────────────────────────────────────────────────┐
│  Consumers                                          │
│  ┌──────────────┐   ┌───────────────────────────┐  │
│  │  cli (dvc)   │   │  future UI / native plugin │  │
│  └──────┬───────┘   └──────────────┬────────────┘  │
│         └──────────────┬───────────┘               │
│                ┌───────▼────────┐                  │
│                │  core (lib)    │                  │
│                │  ┌──────────┐  │                  │
│                │  │   api    │  │  public surface  │
│                │  └────┬─────┘  │                  │
│                │  ┌────▼─────┐  │                  │
│                │  │  cas     │◄─┼── objects/ tree  │
│                │  │  hash    │  │  (BLAKE3 blobs)  │
│                │  │  manifest│◄─┼── manifest.db    │
│                │  │  error   │  │  (SQLite)        │
│                │  └──────────┘  │                  │
│                └────────────────┘                  │
└─────────────────────────────────────────────────────┘
```

- **No daemon. No network. Strictly local in v1.**
- All I/O is streaming — an 800MB file is never fully resident in memory.
- The `core` crate is the stability boundary; `cli` may change shape freely.

## Key Decisions

- [0000 Record architecture decisions](docs/adr/0000-record-architecture-decisions.md)
- [0001 Storage layout — full-file CAS (Option A)](docs/adr/0001-storage-layout.md)

## Conventions

### Code
- **Rust edition 2021.** Stable toolchain; no nightly features.
- **`thiserror`** for library error types; **`anyhow`** only in `cli`.
- **No `unwrap()` / `expect()` in `core`** except in tests. All fallible paths return `Result`.
- Streaming I/O everywhere — `BufReader` / `BufWriter`; never `read_to_end` on user files.
- Atomic blob writes: tmp → fsync → rename. Never write directly to the final path.
- Single-pass hashing: hash the bytes *while* writing (tee-style splitter), not as a separate read.

### Tests
- Unit tests live in the same file (`#[cfg(test)]` module).
- Integration tests in `core/tests/integration.rs`.
- Large-file tests (≥800MB) are gated behind `RUN_LARGE_FILE_TESTS=1`.
- Property tests use `proptest`; target: random-bytes → snapshot → restore → byte-identical.

### Commits
- Conventional Commits style: `feat:`, `fix:`, `test:`, `docs:`, `chore:`.
- One logical change per commit. Don't mix scaffold and implementation.

### Naming
- The binary is tentatively `dvc` for v1 internal use. **Do not publish under this name** — it clashes with iterative.ai's Data Version Control tool (see Risks in `plan.md`).

## Out of Scope (v1)

- **Branching / merging** — snapshot and restore only.
- **Network / cloud sync** — strictly local.
- **Chunked CAS / binary deltas** — full-file only; data model is forward-compatible.
- **Multi-file / directory snapshots** — API is designed for it, but v1 exposes one file at a time.
- **Content-type validation** — no magic-byte checks; caller is trusted.
- **Async runtime** — synchronous, single-writer; no `tokio` in `core`.
- **Watch mode / autosave** — Milestone 3+.
- **UI binding** (Tauri, N-API, native plugin) — separate planning conversation.
