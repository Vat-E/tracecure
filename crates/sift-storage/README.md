# sift-storage

M1 storage layer for Sift. Replaces M0's flat-NDJSON `sift_server::Store`.

## What's here

- WAL with serde_json+CRC32 frames, 10 ms / 1 MB group-commit fsync.
- Memtable: 256 MB / 60 s flush triggers.
- Parquet segments (zstd-3, 1M-row row groups, no bloom filters — see ADR-002).
- Filesystem-as-catalog: `{data}/wal/*.log` + `{data}/segments/*.parquet`.
- WAL replay on `Store::open`; partial trailing frames tolerated.

See [`docs/superpowers/specs/2026-04-23-sp1-sift-storage-design.md`](../../docs/superpowers/specs/2026-04-23-sp1-sift-storage-design.md) for the full design and [`docs/decisions/ADR-002-m1-writer-knobs-revision.md`](../../docs/decisions/ADR-002-m1-writer-knobs-revision.md) for why the defaults are what they are.

## Public API

```rust
use sift_storage::Store;

let store = Store::open("./data").await?;
store.append(record).await?;
let (matches, scanned) = store.search("error", 100).await?;
let mut rx = store.subscribe();
```

Same four methods as M0's `sift_server::Store`. SP2 swaps the server over.

## What's NOT here yet

- Time-partitioned segment layout (SP3).
- Background compaction (SP3).
- Retention / eviction (SP4).
- SQLite catalog (SP4).
- Label index / inverted postings (SP5).
- WAL rotation on flush — the WAL grows unboundedly between restarts. Bounded by memtable size × restart cadence for now.

## Notable deviations from the initial design

- **WAL body is `serde_json`, not `bincode`.** `sift_core::Record` uses `#[serde(skip_serializing_if = …)]` on empty label/field maps, which bincode's positional encoding can't round-trip. JSON is self-describing and tolerates skipped fields; body is ~3–4× larger but the WAL is bounded by memtable size so this isn't a pressure point.
