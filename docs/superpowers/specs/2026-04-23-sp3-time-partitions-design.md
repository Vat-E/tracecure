# SP3 — Time-partitioned segments, compaction, WAL rotation, search time filter

**Status:** accepted design, awaiting implementation plan
**Date:** 2026-04-23
**Sub-project of:** M1 (real storage; see `PLAN.md` §4.2 and `docs/decisions/ADR-002-m1-writer-knobs-revision.md`)
**Builds on:** [SP1](2026-04-23-sp1-sift-storage-design.md) + SP2 cutover

## Goal

Round out M1 storage's middle layer: segments get time-partitioned, a background compactor merges small segments within a closed partition, the WAL rotates after each flush so it stops growing unboundedly, and the search API gains optional `from`/`to` time-range filters so the partition layout actually cuts read cost.

Drop-in from SP2's perspective: `Store::open`, `append`, `search`, `subscribe` signatures don't change. `search` grows two optional parameters.

## Non-goals (deliberately deferred)

- **SQLite catalog** — SP4. SP3 keeps using the filesystem as the catalog.
- **Time-based retention / deletion** — SP4. Nothing gets deleted by wall-clock age in SP3; compaction only shrinks file count.
- **Tiered compaction (L0 → L1 → L2)** — post-M1. Overkill for single-node.
- **Per-stream partition granularity or partition-size policy** — post-M1.
- **Label index / inverted postings** — SP5.
- **UI surfacing of the time filter** — post-M1. Only the CLI + HTTP API expose it.
- **Cross-partition compaction** — never. Partition boundary is a hard cut.

## Scope

1. **Time partitioning** — segments grouped into 1 h windows (`duration_trunc`).
2. **Background compactor** — merges small segments in closed partitions.
3. **WAL rotation** — new WAL file after each successful flush; old WAL deleted atomically.
4. **Search time-range filter** — `from` / `to` on `SearchQuery`; pruning in storage.
5. **Orphan sweep on open** — leftovers from crashes during compaction or rotation get cleaned up.

## Architecture

Three new pieces join `Store`:

```
                    ┌────────────────────────────────────────────┐
                    │                   Store                     │
                    │  ┌──────────┐  ┌──────────┐  ┌───────────┐ │
                    │  │   WAL    │  │ Memtable │  │  Segments │ │
                    │  │  writer  │  │          │  │   (many)  │ │
                    │  └──────────┘  └──────────┘  └───────────┘ │
                    │        │             │            │  ▲     │
                    │        │       flush │            │  │     │
                    │        │             ▼            │  │ compact
                    │        │      ┌──────────────┐    │  │     │
                    │        └────▶ │ rotate WAL   │    │  │     │
                    │               └──────────────┘    │  │     │
                    │                                   ▼  │     │
                    │        ┌──────────────────────────────┐    │
                    │        │ background Compactor task    │    │
                    │        │   ticks every 5 min;         │    │
                    │        │   merges closed partitions   │    │
                    │        └──────────────────────────────┘    │
                    └────────────────────────────────────────────┘
```

### 3.1 Partition identity

```rust
pub struct PartitionConfig {
    /// Granularity; 1h default. Smaller values exist for tests.
    pub granularity: Duration,
    /// Minimum time past a partition's end before it's eligible for
    /// compaction. Guards against late-arriving records.
    pub grace: Duration,
}

impl Default for PartitionConfig {
    fn default() -> Self {
        Self {
            granularity: Duration::from_secs(3600),
            grace: Duration::from_secs(30 * 60),
        }
    }
}

fn partition_id(ts: DateTime<Utc>, granularity: Duration) -> DateTime<Utc>;
fn partition_label(id: DateTime<Utc>, granularity: Duration) -> String;
```

`partition_id` floors the timestamp; `partition_label` formats it as a filesystem-safe string. For 1 h granularity the label is `2026-04-23T12`; for sub-second granularity we use `2026-04-23T12:03:45.100`. Colons in filenames are fine on Linux/macOS; Windows forbids them, so the formatter substitutes underscores when the granularity is sub-minute.

### 3.2 Segment naming

Flat under `segments/`, partition prefix then sequence:

```
{data_dir}/segments/
├─ 2026-04-23T11-0000000001.parquet
├─ 2026-04-23T11-0000000002.parquet     # compacted result
├─ 2026-04-23T12-0000000003.parquet
└─ 2026-04-23T12-0000000004.compacting.parquet   # in-flight, deleted by orphan sweep on crash
```

The global `next_seq` counter continues unchanged from SP1 — it's monotonic across all partitions. Reading the filename gives you both the partition (via the prefix) and the write order (via the sequence).

`SegmentHandle` gains a `partition: DateTime<Utc>` field. The value is derived at write time from the records' timestamps (guarded assertion: all records in one segment belong to the same partition) and cached in the handle so compactor queries don't have to parse the filename.

### 3.3 Flush path

`flush_now` changes:

```
flush_now()
  ├─ acquire flush permit
  ├─ swap memtable → owned Vec<Record>
  ├─ fsync WAL through current
  ├─ group Vec<Record> by partition_id
  │     └─ for each group → write one segment:
  │           write_segment(group, segments/{partition_label}-{next_seq}.parquet)
  ├─ acquire segment_set write lock:
  │     └─ push each new SegmentHandle (in partition-then-seq order)
  ├─ drop the lock
  ├─ rotate the WAL (§3.5)
  └─ release flush permit
```

Multi-partition flushes are rare but correct: typical memtable holds ~minute of traffic and is within one 1 h partition. Retroactive backfills of old records produce multi-partition flushes.

### 3.4 Compactor task

Spawned by `Store::open`, lives for the lifetime of the Store. Configurable via `StoreConfig::compactor`:

```rust
pub struct CompactorConfig {
    /// How often the compactor wakes up and checks for work.
    pub tick_interval: Duration,
    /// Disable entirely (tests that want to control compaction manually).
    pub enabled: bool,
}

impl Default for CompactorConfig {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_secs(5 * 60),
            enabled: true,
        }
    }
}
```

Each tick runs `compact_once`:

```
compact_once()
  ├─ snapshot segment_set (read lock, clone Vec<SegmentHandle>)
  ├─ group handles by partition
  ├─ for each partition (skip the active one):
  │     └─ eligibility checks:
  │          - partition_end + grace ≤ now?
  │          - len(handles) ≥ 2?
  │     └─ if eligible: merge (§3.4.1)
  └─ done
```

Active partition: `partition_id(Utc::now(), granularity)`. Never compacts the active one — new records are landing there.

#### 3.4.1 Merge one partition

```
merge(partition, handles):
  ├─ seq = next_seq.fetch_add(1)
  ├─ tmp_path = segments/{partition_label}-{seq}.compacting.parquet
  ├─ final_path = segments/{partition_label}-{seq}.parquet
  ├─ write all records from handles[0..n] into tmp_path
  │     (parallel read batches, single-threaded write)
  ├─ std::fs::rename(tmp_path, final_path)                 // atomic publish
  ├─ acquire segment_set write lock:
  │     - remove the old handles
  │     - insert the new handle
  ├─ drop the lock
  ├─ for old_handle in old_handles:
  │     └─ fs::remove_file(old_handle.path)
  │        on error (Windows: file still open by a reader): log warn,
  │        leave as orphan. Next Store::open sweeps it.
  └─ done
```

The atomic rename is the commit. Readers who snapshotted the segment set before the swap keep using the old files until they finish their scan; after the swap, new snapshots see the merged file.

**Idempotency:** if a record lands in a compacted partition after the merge, it creates a new segment file. Next compactor tick sees len(handles) == 2 and merges again. Compaction is self-healing.

### 3.5 WAL rotation

Post-flush, after the new handles are registered:

```
rotate_wal()
  ├─ old_writer = self.wal (take ownership or Arc swap)
  ├─ next_seq_wal = self.next_seq.load()            // reuses the same counter
  ├─ new_path = wal/{next_seq_wal}.log
  ├─ new_writer = WalWriter::open(new_path, cfg).await?
  ├─ self.wal = Arc::new(new_writer)
  ├─ atomic_write(wal/current, next_seq_wal.to_string())
  ├─ old_writer.close().await?                       // final fsync, join ticker
  ├─ fs::remove_file(old_writer.path())
  └─ done
```

Tricky part: `self.wal` is shared via `Arc` with `append` callers. Use an `ArcSwap` or a lock around the `Arc<WalWriter>` slot so the rotation can replace it atomically. An `append` that already grabbed the old `Arc<WalWriter>` finishes writing to the old buffer; the old ticker's final flush catches it before close.

On crash mid-rotation:
- Crash after `new_writer` opens but before `current` is updated: `wal/current` still points at the old file. Orphan sweep finds the new file, deletes it. Safe — no records were acked to users via the new file yet.
- Crash after `current` is updated: new file is authoritative. Old file is an orphan. Sweep deletes it.

### 3.6 Search with time-range pruning

`sift-api::SearchQuery`:

```rust
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub to: Option<DateTime<Utc>>,
}
```

`sift-storage::Store::search` signature extends:

```rust
pub async fn search(
    &self,
    query: &str,
    limit: usize,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> anyhow::Result<(Vec<Record>, usize)>;
```

Scan path:

1. **Memtable:** iterate; skip records where `ts < from` or `ts >= to`; apply substring filter; collect.
2. **Segments:** iterate `seg_snapshot`; for each handle:
   - If `from.is_some() && handle.max_ts < from`: skip, don't open the file.
   - If `to.is_some() && handle.min_ts >= to`: skip, don't open the file.
   - Otherwise: scan the parquet, apply both the time filter and the substring filter during the row loop.
3. Merge, sort newest-first, truncate.

`scanned_total` counts only rows that were **read** — pruned segments don't inflate the count. That keeps the number meaningful as a cost indicator.

CLI: `sift search --from 2026-04-23T00:00:00Z --to 2026-04-23T01:00:00Z 'needle'`.

### 3.7 Restart recovery (`Store::open` additions)

Between "list segments" and "replay WAL":

1. **Segment orphan sweep.** After loading valid parquet files into `segment_set`:
   - List any file matching `*.compacting.parquet` in `segments/` and delete it (interrupted compaction).
   - Optionally: log if the total non-compacting segments count is surprisingly high (>10k) — cheap ops signal.

2. **WAL orphan sweep.** After reading `wal/current`:
   - List `wal/*.log`.
   - Delete any file NOT named in `wal/current`.
   - If `wal/current` is missing but there's exactly one `wal/*.log`, treat it as the active one (atomic-write of the pointer never completed, but we can recover).
   - If `wal/current` is missing and there's no `*.log`, create `wal/0000000001.log`.

### 3.8 Error handling

| Failure | Behavior |
|---|---|
| Compaction tmp write fails (disk full) | Log error, remove tmp file, leave partition un-compacted. Next tick retries. |
| Compaction rename fails | Same as write fail. `rename` is atomic on POSIX/Windows, so partial state is impossible. |
| Old segment delete fails (Windows open handle) | Log warn, file stays. Next `Store::open` sweeps it if the name matches `*.compacting.parquet`; otherwise it stays as a dead segment in `segment_set`. **Known MVP gap:** compacted-away segments whose delete failed stick around until the next restart. Acceptable in SP3; SP4's catalog will track "superseded" state explicitly. |
| WAL rotation partial | Orphan sweep on next `open`. §3.5. |
| Compaction reads a corrupt segment | Skip this merge attempt; log error with segment path. Operator-level issue. |
| All compactors fail | Each tick re-tries; degrades to segment-per-flush growth until fixed. |

### 3.9 Concurrency model

- `Store` members:
  - `wal: ArcSwap<WalWriter>` (was `Arc<WalWriter>`; needed for rotation).
  - `memtable: Arc<Mutex<Memtable>>` (unchanged).
  - `segments: Arc<RwLock<Vec<SegmentHandle>>>` (unchanged).
  - `flush_permit: Arc<Semaphore>` (unchanged).
  - `next_seq: Arc<AtomicU64>` (unchanged).
  - **NEW:** `compactor_handle: Option<JoinHandle<()>>` — shut down on `Drop`.
- Compactor grabs `segments` read lock for snapshot, drops it, then write lock to commit. Never holds both at once.
- The flush path and the compactor can write to `segment_set` concurrently with each other — both take the write lock; the lock serialises them. No deadlock risk: neither acquires any other lock while holding `segment_set`.

### 3.10 Store shutdown

`Store::close` (new method, optional for tests — drop is still best-effort):

1. Cancel compactor task (`compactor_handle.abort(); .await`).
2. Final flush if memtable non-empty.
3. `wal.load().close().await?` — drain ticker, final fsync.

`Drop` impl aborts the compactor but doesn't wait. Graceful shutdown needs `close().await`.

## Crate structure

New files:

```
crates/sift-storage/src/
├─ partition.rs   # partition_id, partition_label, PartitionConfig
└─ compactor.rs   # CompactorConfig, compact_once, merge
```

Existing files modified:

- `store.rs` — `ArcSwap<WalWriter>`, `CompactorConfig`, compactor spawn, `rotate_wal`, new `search` signature, orphan sweep in `open_with`.
- `segment.rs` — `SegmentHandle.partition`, helper to write+register multiple segments per flush.
- `lib.rs` — re-export `PartitionConfig`, `CompactorConfig`.

## Dependencies

Add to `crates/sift-storage/Cargo.toml`:

- `arc-swap = "1"` — for the rotatable `WalWriter` slot.
- `chrono` — already there.

No other crate-level changes.

## Testing

### Unit

- `partition::tests::record_maps_to_expected_partition_id`
- `partition::tests::granularity_1s_rounds_correctly_around_midnight`
- `partition::tests::label_is_filesystem_safe_on_windows` (uses underscores instead of colons for sub-minute granularity)

### Integration (`tests/` under `sift-storage`)

- `flush_writes_one_segment_per_partition` — 100 ms partitions; seed records across 3 partitions; flush; observe 3 segment files with correct prefixes.
- `compactor_merges_closed_partition_only` — 100 ms partitions + 50 ms grace; push records into partition A, advance time, push into partition B (active). Tick the compactor. A has 1 merged segment; B untouched.
- `compactor_is_idempotent_on_late_records` — post-compaction, push a record with a timestamp back in the compacted partition (retroactive). Tick again. Back to 1 segment.
- `compactor_skips_single_segment_partition` — 1 segment partition stays untouched.
- `search_with_time_range_prunes_segments` — records in 3 partitions, search with a range covering only the middle; `scanned` reflects only that partition's rows.
- `search_without_time_range_preserves_sp2_behaviour` — all segments scanned.
- `search_memtable_time_filter` — pre-flush memtable, time range excludes one record.
- `wal_rotates_after_flush` — trigger flush; observe `wal/*.log` contains exactly the new file; `wal/current` points at it.
- `orphan_compacting_file_swept_on_open` — drop a bogus `…compacting.parquet` into `segments/`; reopen; file gone.
- `orphan_wal_swept_on_open` — drop an extra `wal/{N}.log` not pointed at by `current`; reopen; file gone.
- `records_survive_restart_across_wal_rotation` — append → flush (triggers rotation) → append more → restart → search finds all.
- `store_close_drains_memtable_and_joins_compactor` — stress-shutdown path.

### Out of scope for SP3 tests

- Real 1 h scale — too slow. Config-driven granularity handles this.
- Kill-9 between rename and lock acquire — hard to simulate cleanly; we cover it by construction (atomic rename + lock discipline).

## Migration note for SP4

SP4 adds the SQLite catalog. At that point, the filesystem-as-catalog pattern gets replaced: `SegmentHandle` lookups come from the catalog, not `fs::read_dir`; compaction state (ran/didn't-run per partition) becomes explicit; superseded-but-not-deleted segments get a tombstone row instead of living forever on disk. SP3's structure should make that swap mostly a `lib.rs` rewire.

## Open questions

None blocking. Everything converged in the brainstorm.

## References

- [SP1 design](2026-04-23-sp1-sift-storage-design.md) + [SP1 plan](../plans/2026-04-23-sp1-sift-storage.md).
- [ADR-001](../../decisions/ADR-001-m1-storage-format.md), [ADR-002](../../decisions/ADR-002-m1-writer-knobs-revision.md).
- `PLAN.md` §4.2.
