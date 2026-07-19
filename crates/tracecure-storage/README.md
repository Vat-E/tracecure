# tracecure-storage

M1 storage layer for Tracecure. Superseded M0's flat-NDJSON store — 
`tracecure_server::Store` is now a re-export of this type
(`crates/tracecure-server/src/lib.rs`), not a separate implementation.

## What's here

- WAL with serde_json+CRC32 frames, 10 ms / 1 MB group-commit fsync, rotation after each flush.
- Memtable: 256 MB / 60 s flush triggers.
- Time-partitioned parquet segments (1 h default; `PartitionConfig` to tune). zstd-3 compression, 1M-row row groups, no bloom filters — see ADR-002.
- Background compactor: ticks every 5 min and merges a closed partition (past
  its end + 30 min grace) down to one segment. Skips the active partition and
  any partition already holding fewer than 2 segments, so a closed
  single-segment partition is never rewritten. It never prunes by age or size
  — see "What's NOT here yet".
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

## Tunables are not operator-configurable

Every number above is a compile-time default. `StoreConfig` (`store.rs`) bundles
`WalConfig` / `MemtableConfig` / `PartitionConfig` / `CompactorConfig` and is
reachable only through `Store::open_with(data_dir, cfg)`; `Store::open` hardcodes
`StoreConfig::default()`, and that is the only path the server takes
(`tracecure_server::AppState::open` → `Store::open`). Outside this crate's own
integration tests, nothing constructs a `StoreConfig`.

There is no config file and no environment variable for any of these —
`tracecure-server serve` accepts exactly `--data`, `--bind`, and `--ui-dist`.
(`RUST_LOG` is honoured, but only for tracing verbosity, not for storage.) Changing the fsync interval,
the flush thresholds, the partition granularity, or the compaction tick means
editing the defaults and rebuilding. Treat `open_with` as a test seam, not an
operator knob.

## Restart behaviour

The "filesystem as catalog" line above is load-bearing: there is no manifest
recording what *should* be on disk, so `Store::open` trusts the directory
listing and repairs anything that disagrees with it. Concretely:

- **`wal/current` is the sole authority for replay.** Only the log it names is
  replayed. Every other `wal/*.log` is deleted unread — normally correct, since
  a rotated WAL's records are already in a segment, but it means a stray log
  file is discarded rather than recovered. If `wal/current` is missing entirely,
  the lowest-numbered log becomes the keeper and the rest are deleted.
- **An unreadable segment is skipped, not fatal.** A parquet file that fails to
  load is logged at `error` and dropped from the catalog; `Store::open` still
  succeeds. Those records are then silently absent from every search, and
  nothing detects the gap afterwards.
- **`*.compacting.parquet` leftovers are swept.** Mid-compaction temp files that
  never got renamed are deleted on open.
- **Segment filenames carry a sequence counter** — `{partition-label}-{seq:010}.parquet`,
  e.g. `2026-04-23T13-0000000005.parquet`.

  > ⚠️ **Known bug:** `next_seq` is *intended* to resume from the highest
  > existing sequence on open, but the parse at `store.rs:85-95` runs
  > `parse::<u64>()` against the whole file stem
  > (`2026-04-23T13-0000000005`), which never parses. `max()` is therefore
  > always `None` and the counter restarts at **1** on every open. Two
  > restarts inside one partition window can make a flush overwrite an
  > existing segment.

Replay stops cleanly at a partial trailing frame (torn write at crash time);
everything before it is kept.

## What's NOT here yet

- SQLite catalog (SP4).
- Time-based retention / eviction (SP4).
- Label index / inverted postings (SP5).
- Tiered compaction (post-M1).

## Notable deviations from the initial design

- **WAL body is `serde_json`, not `bincode`.** `tracecure_core::Record` uses `#[serde(skip_serializing_if = …)]` on empty label/field maps, which bincode's positional encoding can't round-trip. JSON is self-describing and tolerates skipped fields; body is ~3–4× larger but the WAL is bounded by memtable size so this isn't a pressure point.
- **Partition labels use underscores at sub-hour granularity.** Windows forbids `:` in filenames, so `%H:%M` → `%H_%M` for minute/second partitions.
