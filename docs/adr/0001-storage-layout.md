# 0001. Storage Layout — Full-File Content-Addressable Store (Option A)

- **Status:** Accepted
- **Date:** 2026-05-13

## Context

Designers work with large opaque binary files (`.psd`, `.psb`, `.3dm`, `.pdf`) that can reach 800 MB. We need a storage strategy for v1 that:

1. Is simple enough to implement, audit, and reason about correctly.
2. Can survive a crash mid-write without producing a corrupt snapshot.
3. Does not preclude a more efficient strategy (chunking / binary deltas) in a future version.
4. Keeps restore latency predictable (no reconstruction step in v1).
5. Provides natural deduplication when the user saves the same content twice.

Three options were evaluated:

| Option | Description |
|--------|-------------|
| **A — Full-file CAS** | Each snapshot stores the entire file as a single blob, keyed by BLAKE3 hash. Identical files share one blob automatically. |
| **B — Chunked CAS** | File is split into fixed/variable-size chunks; only changed chunks are stored. Reduces storage for incremental edits but adds reconstruction complexity. |
| **C — Binary deltas** | Store forward or reverse binary diffs (e.g., `xdelta3`, `bsdiff`) relative to a base snapshot. Smallest storage for sequential edits; highest complexity and restore latency. |

## Decision

We adopt **Option A — Full-File CAS** for v1.

**Layout:**

```
<store_root>/
├── manifest.db               # SQLite — projects + snapshots index
└── objects/
    ├── tmp/                  # In-flight writes (uuid-named, cleaned on open)
    ├── ab/
    │   └── ab3f...           # blob file: objects/<hash[0..2]>/<hash>
    └── cd/
        └── cd91...
```

**Write path (atomic):**

1. Open source file with a `BufReader`.
2. Tee the byte stream through a BLAKE3 `Hasher` while writing to `objects/tmp/<uuid>` via a `BufWriter`.
3. `File::sync_all()` (fsync) the tmp file.
4. `std::fs::rename(tmp, objects/<hash[0..2]>/<hash>)` — atomic on POSIX; on Windows use `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` if needed.
5. Record the snapshot in `manifest.db`.

If the process crashes after step 3 but before step 5, a stray blob sits in `objects/tmp/`. On the next `init` or `open`, `tmp/` is swept for files older than a configurable threshold (default: 1 hour) and deleted. The manifest never referenced these blobs, so GC is trivially correct.

**Verify on restore:** re-hash the blob before writing to the destination. Abort and surface an error if the stored hash doesn't match — protects against silent disk corruption.

**Two-char prefix sharding** (`objects/<h[0..2]>/`) keeps directory entry counts manageable up to ~65 k unique blobs per prefix bucket, which is well beyond any single-designer use case.

## Consequences

### Positive
- Simplest possible implementation — no reconstruction step.
- Atomic writes protect against corruption at the storage layer.
- Natural deduplication: 50 identical saves → 1 blob.
- Forward-compatible: manifest schema stores `blob_hash`; a future chunked-CAS implementation can add a `chunks` table and migrate blobs incrementally without changing the public API.
- Restore is a single sequential read — predictable, fast.

### Negative / Accepted trade-offs
- **Storage growth is linear in the number of unique versions.** 50 distinct 800 MB saves = 40 GB. Mitigated by: (a) `dsv gc` to prune old snapshots, (b) surfacing storage usage in `dsv list` output, (c) clear documentation.
- **No space savings for near-identical revisions.** PSD files re-compress their internal structure on every save, making byte-level similarity low in practice. Chunked CAS would save little without application-aware chunking, which is out of scope.
- **Windows atomic rename.** `std::fs::rename` is not guaranteed atomic over an existing destination on Windows. Tracked as a risk; addressed in the implementation with a Windows-specific code path if required. See `plan.md` → Risks.

## Alternatives Considered

### Option B — Chunked CAS
Rejected for v1. Content-defined chunking (CDC) adds ~500 LOC of non-trivial code (chunk boundary detection, chunk index per snapshot, multi-chunk restore path). The savings are unreliable for opaque compressed binaries like PSD. The data model accommodates adding a `chunks` table later; this is a deferral, not a foreclosure.

### Option C — Binary Deltas
Rejected outright for v1. Delta encoding is fragile for large binary formats (corrupt base = all subsequent restores broken). Restore requires chained delta application. Acceptable only for plain-text or well-understood binary formats with reliable diff tools. Out of scope for v1 and possibly v2.
