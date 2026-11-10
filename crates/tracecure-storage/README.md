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
  single-segment partition is not rewritten unless retention has something to
  remove from it.
- Retention by time, enforced in that same pass: an optional default maximum
  age plus per-`stream_id` overrides. Off unless configured. See
  [Retention](#retention) and
  [ADR-003](../../docs/decisions/ADR-003-m1-retention.md).
- Search: substring filter on `body`, optional `[from, to)` time range for segment pruning,
  and optional `level` / `app` / `host` / `stream_id` filters (`SearchFilter`). Rows are
  tested on those plain columns before their labels and fields are decoded, and `limit`
  applies after the filter.
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

// Structured filters: the newest 100 errors from apps starting with "billing".
let filter = SearchFilter::from_params("", Some("error"), Some("billing*"), None, None)?;
let (matches, scanned) = store.search_filtered(&filter, 100, None, None).await?;
let mut rx = store.subscribe();

// Clean shutdown:
store.close().await?;
```

## Retention

```rust
use tracecure_storage::{RetentionConfig, Store, StoreConfig};

let cfg = StoreConfig {
    retention: RetentionConfig {
        default: Some(Duration::from_secs(30 * 86_400)),          // 30 days
        per_stream: [("noisy".to_string(), Duration::from_secs(86_400))].into(),
    },
    ..StoreConfig::default()
};
let store = Store::open_with("./data", cfg).await?;
```

- A stream with its own rule ignores the default — shorter or longer.
- No rules (the default) means nothing is ever deleted.
- **Partition-aligned:** a stream's records leave a partition once the whole
  partition is older than the rule, so data outlives its rule by up to one
  partition (1 h) plus the compactor's grace (30 min) and tick (5 min).
- A partition past every rule is deleted without being read; that needs a
  default rule. Otherwise the partition is rewritten once, without the expired
  streams.
- What has been pruned is remembered in memory. After a restart each old
  segment is read once and — if it has nothing to drop — left untouched.
- Deletion is final. There is no tombstone or undo.

`Store::trigger_compaction_at(now)` runs a pass as of a chosen time, which is
how the tests ask "what is gone a week from now" without waiting a week.

## Tunables are not operator-configurable — except retention

Retention is the exception: the server takes it from `--retention` and
`--retention-stream` (or `TRACECURE_RETENTION` / `TRACECURE_RETENTION_STREAMS`)
and passes it in through `Store::open_with`. It is policy about the
operator's data, not tuning.

Every other number above is a compile-time default. `StoreConfig` (`store.rs`) bundles
`WalConfig` / `MemtableConfig` / `PartitionConfig` / `CompactorConfig` and is
reachable only through `Store::open_with(data_dir, cfg)`; `Store::open` hardcodes
`StoreConfig::default()`. The server calls `open_with` with a default config
whose only non-default field is `retention`. Outside this crate's own
integration tests, nothing else constructs a `StoreConfig`.

There is no config file and no environment variable for any of these —
`tracecure-server serve` accepts `--data`, `--bind`, `--ui-dist` and the two
retention flags.
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
  e.g. `2026-04-23T13-0000000005.parquet`. On open the counter resumes above
  the highest sequence already on disk, so a flush after restart never reuses
  a filename. (The sequence is the part after the *last* `-`; the partition
  label contains its own.)

Replay stops cleanly at a partial trailing frame (torn write at crash time);
everything before it is kept.

## What's NOT here yet

- SQLite catalog (SP4). Retention works without one — see ADR-003.
- Size-based retention ("keep at most N GB").
- Label index / inverted postings (SP5).
- Tiered compaction (post-M1).

## Notable deviations from the initial design

- **WAL body is `serde_json`, not `bincode`.** `tracecure_core::Record` uses `#[serde(skip_serializing_if = …)]` on empty label/field maps, which bincode's positional encoding can't round-trip. JSON is self-describing and tolerates skipped fields; body is ~3–4× larger but the WAL is bounded by memtable size so this isn't a pressure point.
- **Partition labels use underscores at sub-hour granularity.** Windows forbids `:` in filenames, so `%H:%M` → `%H_%M` for minute/second partitions.
