# tracecure-storage

M1 storage layer for Tracecure. Replaces M0's flat-NDJSON `tracecure_server::Store`.

## What's here

- WAL with serde_json+CRC32 frames, 10 ms / 1 MB group-commit fsync, rotation after each flush.
- Memtable: 256 MB / 60 s flush triggers.
- Time-partitioned parquet segments (1 h default; `PartitionConfig` to tune). zstd-3 compression, 1M-row row groups, no bloom filters — see ADR-002.
- Background compactor: ticks every 5 min, merges all segments in a closed partition (past its end + 30 min grace) into one.
- Search: substring filter on `body`, optional `[from, to)` time range for segment pruning.
- Filesystem-as-catalog: `{data}/wal/{N}.log` + `{data}/wal/current` + `{data}/segments/{partition}-{seq}.parquet`.
- WAL replay + orphan sweep on `Store::open`; partial trailing frames tolerated.
- `Store::close` for clean shutdown (compactor stop + WAL shutdown signal).

## Public API

```rust
use tracecure_storage::Store;

let store = Store::open("./data").await?;
store.append(record).await?;

// Optional time-range filter:
let (matches, scanned) = store.search("error", 100, None, None).await?;
let mut rx = store.subscribe();

// Clean shutdown:
store.close().await?;
```

## What's NOT here yet

- SQLite catalog (SP4).
- Time-based retention / eviction (SP4).
- Label index / inverted postings (SP5).
- Tiered compaction (post-M1).

## Notable deviations from the initial design

- **WAL body is `serde_json`, not `bincode`.** `tracecure_core::Record` uses `#[serde(skip_serializing_if = …)]` on empty label/field maps, which bincode's positional encoding can't round-trip. JSON is self-describing and tolerates skipped fields; body is ~3–4× larger but the WAL is bounded by memtable size so this isn't a pressure point.
- **Partition labels use underscores at sub-hour granularity.** Windows forbids `:` in filenames, so `%H:%M` → `%H_%M` for minute/second partitions.
