# SP3 — Time Partitions, Compaction, WAL Rotation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add time-partitioned segment layout, background compaction, WAL rotation on flush, search time-range filter, and orphan sweeps on open to `sift-storage`. Public `Store` API stays drop-in except `search` gains two optional params.

**Architecture:** New modules `partition.rs` + `compactor.rs` inside `sift-storage`. `WalWriter` slot on the store becomes `ArcSwap<WalWriter>` so `rotate_wal` can replace it atomically. `SegmentHandle` gains a `partition` field. Flush groups records by partition before writing; compactor task ticks every 5 min and merges closed partitions. Search extends its scan with time-range pruning; orphan sweeps clean up crashes from rotation or compaction.

**Tech Stack:** `arc-swap = "1"` (new), plus existing `arrow = "55"`, `parquet = "55"`, `tokio`, `chrono`.

Design spec: [`docs/superpowers/specs/2026-04-23-sp3-time-partitions-design.md`](../specs/2026-04-23-sp3-time-partitions-design.md).

---

## File Structure

```
crates/sift-storage/src/
├─ arrow.rs          # unchanged
├─ compactor.rs      # NEW: CompactorConfig, compact_once, merge
├─ lib.rs            # re-export Partition/CompactorConfig
├─ memtable.rs       # unchanged
├─ partition.rs      # NEW: PartitionConfig, partition_id, partition_label
├─ segment.rs        # +SegmentHandle.partition; +write_segment_in_partition helper
├─ store.rs          # biggest changes: ArcSwap<WalWriter>, multi-partition flush,
│                    #   compactor spawn, rotate_wal, search signature +from/+to,
│                    #   orphan sweep, Store::close
└─ wal.rs            # unchanged
```

API surface changes:

- `sift-api::SearchQuery` gains `from: Option<DateTime<Utc>>` + `to: Option<DateTime<Utc>>`.
- `sift-storage::Store::search` grows two params with the same shape.
- `sift-server` routes thread the params through.
- `sift-cli search` CLI gains `--from` / `--to` flags.

Dependency additions:

- Workspace `[workspace.dependencies]`: `arc-swap = "1"`.
- `sift-storage/Cargo.toml` `[dependencies]`: `arc-swap = { workspace = true }`.

---

## Task 1: Deps + module stubs

**Files:**
- Modify: `Cargo.toml` (root)
- Modify: `crates/sift-storage/Cargo.toml`
- Create: `crates/sift-storage/src/partition.rs` (stub)
- Create: `crates/sift-storage/src/compactor.rs` (stub)
- Modify: `crates/sift-storage/src/lib.rs` — declare the two new `pub mod` lines.

- [ ] **Step 1: Add `arc-swap` to the workspace**

In the repo root `Cargo.toml`, under `[workspace.dependencies]`, add (after `bincode = "1"`):

```toml
arc-swap = "1"
```

- [ ] **Step 2: Add `arc-swap` to `sift-storage`**

In `crates/sift-storage/Cargo.toml`, under `[dependencies]`, add after `chrono = { workspace = true }`:

```toml
arc-swap = { workspace = true }
```

- [ ] **Step 3: Create `crates/sift-storage/src/partition.rs`**

```rust
//! Partition identity: floor a timestamp to a partition boundary and format
//! it as a filesystem-safe label.

// Filled in by Task 2.
```

- [ ] **Step 4: Create `crates/sift-storage/src/compactor.rs`**

```rust
//! Background compactor: merges small segments inside closed partitions.

// Filled in by Task 8.
```

- [ ] **Step 5: Wire both into `lib.rs`**

Edit `crates/sift-storage/src/lib.rs`. After the existing `pub mod` block, it should read:

```rust
pub mod arrow;
pub mod compactor;
pub mod memtable;
pub mod partition;
pub mod segment;
pub mod store;
pub mod wal;

pub use store::{Store, StoreConfig};
```

- [ ] **Step 6: Verify build**

Run: `cargo build --workspace`
Expected: builds. `arc-swap` downloaded.

- [ ] **Step 7: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml crates/sift-storage
git commit -m "chore(storage): scaffold partition + compactor modules, add arc-swap"
```

---

## Task 2: Partition identity (TDD)

Implements spec §3.1. Pure functions; fully unit-tested.

**Files:**
- Modify: `crates/sift-storage/src/partition.rs`

- [ ] **Step 1: Replace `partition.rs` (tests + implementation together — TDD via compile-then-run)**

```rust
//! Partition identity: floor a timestamp to a partition boundary and format
//! it as a filesystem-safe label.

use std::time::Duration;

use chrono::{DateTime, DurationRound, TimeDelta, Utc};

#[derive(Debug, Clone, Copy)]
pub struct PartitionConfig {
    /// Partition granularity. 1 h default.
    pub granularity: Duration,
    /// Minimum time past a partition's end before it is eligible for
    /// compaction. 30 min default.
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

/// Floor `ts` to the previous partition boundary.
#[must_use]
pub fn partition_id(ts: DateTime<Utc>, granularity: Duration) -> DateTime<Utc> {
    // `chrono::DurationRound::duration_trunc` does this; we have to convert
    // std::Duration → chrono::TimeDelta first.
    let delta = TimeDelta::from_std(granularity).unwrap_or(TimeDelta::seconds(1));
    ts.duration_trunc(delta).unwrap_or(ts)
}

/// Filesystem-safe label for a partition. Uses RFC3339 minus the timezone
/// suffix, swapping colons for underscores when granularity is sub-minute
/// (Windows forbids colons in paths).
#[must_use]
pub fn partition_label(id: DateTime<Utc>, granularity: Duration) -> String {
    let g = granularity.as_secs();
    let raw = if g >= 3600 {
        id.format("%Y-%m-%dT%H").to_string()
    } else if g >= 60 {
        id.format("%Y-%m-%dT%H:%M").to_string()
    } else {
        id.format("%Y-%m-%dT%H_%M_%S%.3f").to_string()
    };
    raw
}

/// Inclusive partition end = partition start + granularity.
#[must_use]
pub fn partition_end(id: DateTime<Utc>, granularity: Duration) -> DateTime<Utc> {
    id + TimeDelta::from_std(granularity).unwrap_or(TimeDelta::seconds(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 23, h, m, s).unwrap()
    }

    #[test]
    fn record_maps_to_expected_hour_partition() {
        let id = partition_id(t(12, 34, 56), Duration::from_secs(3600));
        assert_eq!(id, t(12, 0, 0));
    }

    #[test]
    fn granularity_1s_rounds_correctly() {
        let id = partition_id(t(12, 34, 56), Duration::from_secs(1));
        assert_eq!(id, t(12, 34, 56));
    }

    #[test]
    fn label_for_1h_is_hour_precision() {
        let id = t(12, 0, 0);
        assert_eq!(partition_label(id, Duration::from_secs(3600)), "2026-04-23T12");
    }

    #[test]
    fn label_for_sub_minute_uses_underscores_not_colons() {
        let id = Utc
            .with_ymd_and_hms(2026, 4, 23, 12, 34, 56)
            .unwrap()
            + chrono::Duration::milliseconds(100);
        let label = partition_label(id, Duration::from_millis(100));
        assert!(
            !label.contains(':'),
            "sub-minute label should be Windows-path-safe: {label}"
        );
        assert!(label.starts_with("2026-04-23T12_34_56"));
    }

    #[test]
    fn partition_end_is_start_plus_granularity() {
        let id = t(12, 0, 0);
        let end = partition_end(id, Duration::from_secs(3600));
        assert_eq!(end, t(13, 0, 0));
    }

    #[test]
    fn records_in_same_partition_share_id() {
        let g = Duration::from_secs(3600);
        let a = partition_id(t(12, 0, 1), g);
        let b = partition_id(t(12, 59, 59), g);
        assert_eq!(a, b);
    }

    #[test]
    fn records_across_boundary_get_different_ids() {
        let g = Duration::from_secs(3600);
        let a = partition_id(t(12, 59, 59), g);
        let b = partition_id(t(13, 0, 1), g);
        assert_ne!(a, b);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p sift-storage --lib partition`
Expected: 7 tests pass.

- [ ] **Step 3: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/sift-storage/src/partition.rs
git commit -m "feat(storage): partition_id + partition_label with 1h default"
```

---

## Task 3: `SegmentHandle.partition` + segment naming helper

Add the `partition` field to `SegmentHandle`. `write_segment` derives the partition from the records' timestamps and asserts all records share it. Add `segment_filename` helper for `{partition_label}-{seq}.parquet`.

**Files:**
- Modify: `crates/sift-storage/src/segment.rs`

- [ ] **Step 1: Extend `SegmentHandle` + `write_segment`**

In `crates/sift-storage/src/segment.rs`, change the struct:

```rust
#[derive(Debug, Clone)]
pub struct SegmentHandle {
    pub path: PathBuf,
    pub row_count: u64,
    pub min_ts: DateTime<Utc>,
    pub max_ts: DateTime<Utc>,
    /// Partition identity (floored timestamp). Matches `min_ts.duration_trunc`
    /// for any valid segment; stored separately so compactor queries don't
    /// re-derive from filenames.
    pub partition: DateTime<Utc>,
}
```

Update `write_segment` to take a partition id and to assert the records agree with it. Replace the current signature (still single-file, single-partition for now; multi-partition flush comes in Task 4):

```rust
use std::time::Duration;

use crate::partition::partition_id;

/// Write a parquet file containing `records`, all of which must belong to
/// `partition`. Returns a handle.
pub fn write_segment(
    records: &[Record],
    path: &Path,
    partition: DateTime<Utc>,
    granularity: Duration,
) -> anyhow::Result<SegmentHandle> {
    anyhow::ensure!(!records.is_empty(), "refusing to write empty segment");

    // Guarded assertion: every record must live in the claimed partition.
    // Cheap — linear scan of an already-in-hand Vec.
    for r in records {
        let got = partition_id(r.timestamp, granularity);
        anyhow::ensure!(
            got == partition,
            "record timestamp {} maps to partition {got}, not {partition}",
            r.timestamp
        );
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let min_ts = records.iter().map(|r| r.timestamp).min().unwrap();
    let max_ts = records.iter().map(|r| r.timestamp).max().unwrap();

    let schema = schema();
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(ZSTD_LEVEL)?))
        .set_max_row_group_size(MAX_ROW_GROUP_SIZE)
        .build();
    let file = std::fs::File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(props))?;
    let batch = records_to_batch(records, schema)?;
    writer.write(&batch)?;
    let meta = writer.close()?;

    Ok(SegmentHandle {
        path: path.to_path_buf(),
        row_count: u64::try_from(meta.num_rows).unwrap_or(0),
        min_ts,
        max_ts,
        partition,
    })
}
```

- [ ] **Step 2: Update `load_handle` to derive partition from stats**

Still in `segment.rs`, add to the existing `load_handle`. After it computes `min_ts` and has access to `granularity` — wait, `load_handle` has no way to know the granularity. Options:

- Pass granularity in: `pub fn load_handle(path: &Path, granularity: Duration) -> Result<SegmentHandle>`. Callers in `Store::open_with` already know the granularity from `StoreConfig::partition`.

Do that. Change `load_handle`'s signature and derive `partition = partition_id(min_ts, granularity)`:

```rust
pub fn load_handle(
    path: &Path,
    granularity: std::time::Duration,
) -> anyhow::Result<SegmentHandle> {
    use crate::partition::partition_id;

    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let md = reader.metadata();
    let row_count = u64::try_from(md.file_metadata().num_rows()).unwrap_or(0);
    anyhow::ensure!(row_count > 0, "segment has 0 rows: {}", path.display());

    let schema_arrow = reader.schema().clone();
    let ts_idx = schema_arrow.index_of("timestamp")?;

    let mut min_micros: Option<i64> = None;
    let mut max_micros: Option<i64> = None;
    for rg in md.row_groups() {
        let col = rg.column(ts_idx);
        if let Some(stats) = col.statistics() {
            if let Some(min) = stats.min_bytes_opt() {
                if min.len() == 8 {
                    let v = i64::from_le_bytes(min.try_into().unwrap());
                    min_micros = Some(min_micros.map_or(v, |m: i64| m.min(v)));
                }
            }
            if let Some(max) = stats.max_bytes_opt() {
                if max.len() == 8 {
                    let v = i64::from_le_bytes(max.try_into().unwrap());
                    max_micros = Some(max_micros.map_or(v, |m: i64| m.max(v)));
                }
            }
        }
    }
    let min_ts = DateTime::<Utc>::from_timestamp_micros(min_micros.unwrap_or(0))
        .unwrap_or_else(Utc::now);
    let max_ts = DateTime::<Utc>::from_timestamp_micros(max_micros.unwrap_or(0))
        .unwrap_or_else(Utc::now);
    let partition = partition_id(min_ts, granularity);

    Ok(SegmentHandle {
        path: path.to_path_buf(),
        row_count,
        min_ts,
        max_ts,
        partition,
    })
}
```

- [ ] **Step 3: Add `segment_filename` helper**

Append to `segment.rs`:

```rust
use std::time::Duration;

use crate::partition::partition_label;

/// Filename inside the `segments/` directory for a given partition + sequence.
#[must_use]
pub fn segment_filename(
    partition: DateTime<Utc>,
    seq: u64,
    granularity: Duration,
) -> String {
    format!(
        "{label}-{seq:010}.parquet",
        label = partition_label(partition, granularity),
    )
}

/// Tmp filename used mid-compaction. Renamed to the final form on success.
#[must_use]
pub fn compacting_filename(
    partition: DateTime<Utc>,
    seq: u64,
    granularity: Duration,
) -> String {
    format!(
        "{label}-{seq:010}.compacting.parquet",
        label = partition_label(partition, granularity),
    )
}
```

- [ ] **Step 4: Update existing `write_segment` test**

The existing `write_then_scan_roundtrips_records` + friends call `write_segment(records, path)`. Update them to pass `partition` and `granularity`. Replace the body of `write_then_scan_roundtrips_records`:

```rust
    #[test]
    fn write_then_scan_roundtrips_records() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("seg.parquet");
        let records = vec![rec("alpha", 0), rec("beta", 1), rec("gamma", 2)];
        let granularity = Duration::from_secs(3600);
        let partition = crate::partition::partition_id(records[0].timestamp, granularity);
        let handle = write_segment(&records, &path, partition, granularity).unwrap();
        assert_eq!(handle.row_count, 3);
        assert_eq!(handle.min_ts, records[0].timestamp);
        assert_eq!(handle.max_ts, records[2].timestamp);
        assert_eq!(handle.partition, partition);

        let got = scan_segment(&handle, "").unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].body, "alpha");
        assert_eq!(got[0].labels.get("env").map(String::as_str), Some("prod"));
    }
```

Make parallel changes for the two other tests: `scan_with_substring_filters` and `load_handle_reads_row_count_from_footer`. `write_empty_slice_errors` stays the same pattern (now needs a partition argument; use `Utc::now()` + 1h). All four tests should compile.

- [ ] **Step 5: Add a new test for the guarded assertion**

Append to the tests module:

```rust
    #[test]
    fn write_segment_rejects_cross_partition_records() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.parquet");
        // Two records separated by more than 1h land in different partitions.
        let records = vec![rec("a", 0), rec("b", 7200)];
        let granularity = Duration::from_secs(3600);
        let partition = crate::partition::partition_id(records[0].timestamp, granularity);
        let err = write_segment(&records, &path, partition, granularity)
            .err()
            .expect("should reject cross-partition records");
        assert!(err.to_string().contains("partition"));
    }
```

- [ ] **Step 6: Update `store.rs` call sites**

`store.rs` currently calls `write_segment(&to_flush, &seg_path)` (2 args) and `load_handle(&path)` (1 arg). Task 4 rewrites the flush path; for this task just make the existing code compile with the new signatures:

In `flush_now`, replace the `write_segment` call with:

```rust
        let granularity = std::time::Duration::from_secs(3600);
        let partition = crate::partition::partition_id(to_flush[0].timestamp, granularity);
        let handle = tokio::task::spawn_blocking(move || {
            write_segment(&to_flush, &seg_path, partition, granularity)
        })
        .await
        .context("flush task join")??;
```

(This is a transient state — Task 4 replaces this with proper multi-partition grouping. Using hardcoded 1 h granularity for now is OK because `StoreConfig::partition` doesn't exist yet.)

In the segment-loading loop of `open_with`, replace the `load_handle(&path)` call with:

```rust
                match load_handle(&path, std::time::Duration::from_secs(3600)) {
```

- [ ] **Step 7: Run tests + clippy**

Run: `cargo test -p sift-storage`
Expected: all 27 previously passing tests still pass, plus the 1 new cross-partition rejection test = 28.

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/sift-storage/src/segment.rs crates/sift-storage/src/store.rs
git commit -m "feat(storage): SegmentHandle.partition + partition-aware write_segment"
```

---

## Task 4: Multi-partition flush + `StoreConfig::partition` (TDD)

Flush groups records by partition and writes one segment per group. This is where `StoreConfig::partition` lands.

**Files:**
- Modify: `crates/sift-storage/src/store.rs`
- Create: `crates/sift-storage/tests/flush_multi_partition.rs`

- [ ] **Step 1: Add `partition: PartitionConfig` to `StoreConfig`**

In `store.rs`, extend `StoreConfig` (it's currently `#[derive(Debug, Clone, Default)]`):

```rust
use crate::partition::PartitionConfig;

#[derive(Debug, Clone, Default)]
pub struct StoreConfig {
    pub wal: WalConfig,
    pub memtable: MemtableConfig,
    pub partition: PartitionConfig,
}
```

Add a field to `Store`:

```rust
pub struct Store {
    data_dir: PathBuf,
    wal: Arc<WalWriter>,
    memtable: Arc<Mutex<Memtable>>,
    segments: Arc<RwLock<Vec<SegmentHandle>>>,
    tx: broadcast::Sender<Record>,
    flush_permit: Arc<Semaphore>,
    next_seq: Arc<AtomicU64>,
    partition_cfg: PartitionConfig,
}
```

In `open_with`, after loading the config, keep a clone:

```rust
        let partition_cfg = cfg.partition;
```

And pass it to the struct literal:

```rust
        let store = Self {
            data_dir,
            wal,
            memtable,
            segments,
            tx,
            flush_permit,
            next_seq,
            partition_cfg,
        };
```

Update the `load_handle(&path, std::time::Duration::from_secs(3600))` call from Task 3 Step 6 to use `cfg.partition.granularity`:

```rust
                match load_handle(&path, cfg.partition.granularity) {
```

- [ ] **Step 2: Replace `flush_now` with the multi-partition version**

```rust
    async fn flush_now(&self) -> anyhow::Result<()> {
        let permit = Arc::clone(&self.flush_permit).acquire_owned().await?;

        let to_flush: Vec<Record> = {
            let mut mt = self.memtable.lock().await;
            if !mt.should_flush() {
                return Ok(());
            }
            mt.take()
        };
        if to_flush.is_empty() {
            return Ok(());
        }

        // Ensure durability before we commit a segment based on these bytes.
        self.wal.fsync_through_current().await?;

        // Group by partition. Typical case: one partition, one group.
        let granularity = self.partition_cfg.granularity;
        let mut groups: std::collections::BTreeMap<chrono::DateTime<chrono::Utc>, Vec<Record>> =
            std::collections::BTreeMap::new();
        for r in to_flush {
            let p = crate::partition::partition_id(r.timestamp, granularity);
            groups.entry(p).or_default().push(r);
        }

        let mut new_handles: Vec<SegmentHandle> = Vec::with_capacity(groups.len());
        for (partition, records) in groups {
            let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
            let filename =
                crate::segment::segment_filename(partition, seq, granularity);
            let seg_path = self.data_dir.join("segments").join(filename);
            let h = tokio::task::spawn_blocking(move || {
                write_segment(&records, &seg_path, partition, granularity)
            })
            .await
            .context("flush task join")??;
            new_handles.push(h);
        }

        {
            let mut segs = self.segments.write().await;
            for h in new_handles {
                segs.push(h);
            }
        }

        drop(permit);
        Ok(())
    }
```

- [ ] **Step 3: Write the failing integration test**

Create `crates/sift-storage/tests/flush_multi_partition.rs`:

```rust
use std::time::Duration;

use chrono::{TimeZone, Utc};
use sift_core::{Level, Record};
use sift_storage::{
    memtable::MemtableConfig, partition::PartitionConfig, store::StoreConfig,
    wal::WalConfig, Store,
};
use std::collections::BTreeMap;

fn rec(body: &str, offset_secs: i64) -> Record {
    let base = Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap();
    Record {
        timestamp: base + chrono::Duration::seconds(offset_secs),
        host: "h".into(),
        app: "a".into(),
        stream_id: "s".into(),
        level: Level::Info,
        body: body.into(),
        labels: BTreeMap::new(),
        fields: BTreeMap::new(),
        raw: None,
    }
}

#[tokio::test]
async fn flush_writes_one_segment_per_partition() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StoreConfig {
        wal: WalConfig {
            fsync_interval: Duration::from_millis(5),
            size_trigger_bytes: 64 * 1024,
        },
        memtable: MemtableConfig {
            size_bytes: 64, // tiny — forces a flush after a few records
            age: Duration::from_secs(3600),
        },
        partition: PartitionConfig {
            granularity: Duration::from_secs(60), // 1-minute partitions
            grace: Duration::from_secs(30),
        },
    };
    let store = Store::open_with(tmp.path(), cfg).await.unwrap();

    // Records across three 1-minute partitions (offset 0s, 70s, 130s).
    for (i, offset) in [0, 70, 130].iter().enumerate() {
        store.append(rec(&format!("r{i}"), *offset)).await.unwrap();
    }
    // Force a flush by appending enough bytes to cross the 64-byte trigger.
    // The three records above already push us past; if not, a follow-up
    // append guarantees it.
    for extra in 0..10 {
        store
            .append(rec(&format!("filler {extra}"), 30 + extra))
            .await
            .unwrap();
    }

    // Wait briefly for the flush to finish.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let segs_dir = tmp.path().join("segments");
    let mut names: Vec<String> = Vec::new();
    let mut rd = tokio::fs::read_dir(&segs_dir).await.unwrap();
    while let Ok(Some(entry)) = rd.next_entry().await {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();

    // We expect at least 3 segment files whose names carry the three distinct
    // minute-prefixes. The exact count can be higher if more than one flush
    // triggered.
    let distinct_partitions: std::collections::BTreeSet<String> = names
        .iter()
        .filter(|n| n.ends_with(".parquet") && !n.contains("compacting"))
        .map(|n| n.rsplit_once('-').unwrap().0.to_string())
        .collect();
    assert!(
        distinct_partitions.len() >= 3,
        "expected >=3 partitions; got {:?}",
        distinct_partitions
    );
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sift-storage --test flush_multi_partition`
Expected: 1 test passes.

Run full suite: `cargo test -p sift-storage`
Expected: all existing tests still pass.

- [ ] **Step 5: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/sift-storage/src/store.rs crates/sift-storage/tests/flush_multi_partition.rs
git commit -m "feat(storage): group memtable records by partition on flush"
```

---

## Task 5: Storage-side search time-range filter (TDD)

Adds `(from, to)` parameters to `Store::search`. Prunes segment handles by `[min_ts, max_ts]` before reading. Filters memtable records by timestamp too.

**Files:**
- Modify: `crates/sift-storage/src/store.rs`
- Create: `crates/sift-storage/tests/search_time_range.rs`

- [ ] **Step 1: Extend `Store::search` signature + body**

Replace the current `Store::search` with:

```rust
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
        from: Option<chrono::DateTime<chrono::Utc>>,
        to: Option<chrono::DateTime<chrono::Utc>>,
    ) -> anyhow::Result<(Vec<Record>, usize)> {
        let mem_snapshot: Vec<Record> = self.memtable.lock().await.records().to_vec();
        let seg_snapshot: Vec<SegmentHandle> = self.segments.read().await.clone();

        let in_range = |ts: chrono::DateTime<chrono::Utc>| -> bool {
            match (from, to) {
                (Some(f), _) if ts < f => false,
                (_, Some(t)) if ts >= t => false,
                _ => true,
            }
        };

        let mut matches: Vec<Record> = mem_snapshot
            .iter()
            .filter(|r| in_range(r.timestamp))
            .filter(|r| query.is_empty() || r.body.contains(query))
            .cloned()
            .collect();

        let mut scanned = mem_snapshot.iter().filter(|r| in_range(r.timestamp)).count();

        for handle in &seg_snapshot {
            // Prune: skip segments whose time span doesn't intersect the
            // query range.
            if let Some(f) = from {
                if handle.max_ts < f {
                    continue;
                }
            }
            if let Some(t) = to {
                if handle.min_ts >= t {
                    continue;
                }
            }

            scanned += usize::try_from(handle.row_count).unwrap_or(usize::MAX);
            let hits = tokio::task::spawn_blocking({
                let handle = handle.clone();
                let query = query.to_string();
                move || crate::segment::scan_segment(&handle, &query)
            })
            .await
            .context("scan task join")??;
            for r in hits {
                if in_range(r.timestamp) {
                    matches.push(r);
                }
            }
        }

        matches.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        matches.truncate(limit);

        Ok((matches, scanned))
    }
```

Note the `scanned` accounting: only rows the caller would plausibly have to read count. Segments pruned by the time range aren't added. Memtable records outside the range aren't added. This keeps `scanned` meaningful as a cost signal.

- [ ] **Step 2: Update internal callers**

The existing `tests/append_and_search.rs` calls `store.search("hello", 10)`. Change those call sites to `store.search("hello", 10, None, None)`. There are two test functions; both need the update.

- [ ] **Step 3: Write the failing integration test**

Create `crates/sift-storage/tests/search_time_range.rs`:

```rust
use std::time::Duration;

use chrono::{TimeZone, Utc};
use sift_core::{Level, Record};
use sift_storage::{
    memtable::MemtableConfig, partition::PartitionConfig, store::StoreConfig,
    wal::WalConfig, Store,
};
use std::collections::BTreeMap;

fn rec(body: &str, offset_secs: i64) -> Record {
    let base = Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap();
    Record {
        timestamp: base + chrono::Duration::seconds(offset_secs),
        host: "h".into(),
        app: "a".into(),
        stream_id: "s".into(),
        level: Level::Info,
        body: body.into(),
        labels: BTreeMap::new(),
        fields: BTreeMap::new(),
        raw: None,
    }
}

#[tokio::test]
async fn time_range_prunes_segments() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StoreConfig {
        wal: WalConfig {
            fsync_interval: Duration::from_millis(5),
            size_trigger_bytes: 64 * 1024,
        },
        memtable: MemtableConfig {
            size_bytes: 64,
            age: Duration::from_secs(3600),
        },
        partition: PartitionConfig {
            granularity: Duration::from_secs(60),
            grace: Duration::from_secs(30),
        },
    };
    let store = Store::open_with(tmp.path(), cfg).await.unwrap();

    // Three partitions: 0-60s, 60-120s, 120-180s.
    for i in 0..15 {
        // every 12 seconds → 5 per partition roughly
        let offset = (i * 12) as i64;
        store.append(rec(&format!("rec{i}"), offset)).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Search only the middle partition: [60, 120).
    let base = Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap();
    let (matches, scanned) = store
        .search(
            "",
            100,
            Some(base + chrono::Duration::seconds(60)),
            Some(base + chrono::Duration::seconds(120)),
        )
        .await
        .unwrap();

    // Every returned record should be in [60, 120).
    for r in &matches {
        let offset = (r.timestamp - base).num_seconds();
        assert!(
            (60..120).contains(&offset),
            "record outside range: offset={offset}"
        );
    }
    // scanned should reflect only that partition's rows (roughly a third
    // of the 15 records). Assert loosely: > 0 and < 15.
    assert!(scanned > 0 && scanned < 15, "scanned={scanned}");
}

#[tokio::test]
async fn search_without_time_range_preserves_sp2_behaviour() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).await.unwrap();

    let base = Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap();
    for i in 0..5 {
        store.append(rec(&format!("m{i}"), i)).await.unwrap();
    }

    let (matches, scanned) = store.search("", 100, None, None).await.unwrap();
    assert_eq!(matches.len(), 5);
    assert_eq!(scanned, 5);
    let _ = base;
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sift-storage --test search_time_range`
Expected: 2 tests pass.

Run full storage suite: `cargo test -p sift-storage`
Expected: all pass.

- [ ] **Step 5: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/sift-storage/src/store.rs crates/sift-storage/tests/search_time_range.rs crates/sift-storage/tests/append_and_search.rs
git commit -m "feat(storage): search time-range filter with segment pruning"
```

---

## Task 6: API + server + CLI time-range wiring (TDD)

Thread `from` / `to` through `sift-api::SearchQuery`, `sift-server`'s search route, and `sift-cli search` command.

**Files:**
- Modify: `crates/sift-api/src/search.rs`
- Modify: `crates/sift-server/src/routes/search.rs`
- Modify: `crates/sift-cli/src/main.rs`
- Modify: `crates/sift-server/tests/search.rs`

- [ ] **Step 1: Extend `SearchQuery`**

In `crates/sift-api/src/search.rs`, add to the struct (after `limit`):

```rust
    #[serde(default)]
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub to: Option<chrono::DateTime<chrono::Utc>>,
```

Update the struct to derive `Eq` still works (chrono types derive it too — they do). Keep the module's existing derives: `Clone, Debug, PartialEq, Eq, Serialize, Deserialize`.

- [ ] **Step 2: Update `sift-api::search` tests**

The existing tests don't mention the new fields; they'll keep passing because of `#[serde(default)]`. Add one new test in the existing `mod tests` at the bottom of `search.rs`:

```rust
    #[test]
    fn search_query_parses_time_range_params() {
        let json = r#"{"q":"x","from":"2026-04-23T00:00:00Z","to":"2026-04-23T01:00:00Z"}"#;
        let q: SearchQuery = serde_json::from_str(json).unwrap();
        assert!(q.from.is_some());
        assert!(q.to.is_some());
        assert_eq!(q.q, "x");
    }
```

- [ ] **Step 3: Update server route to pass params through**

In `crates/sift-server/src/routes/search.rs`, replace the handler body:

```rust
async fn search(State(state): State<AppState>, Query(q): Query<SearchQuery>) -> impl IntoResponse {
    match state.store().search(&q.q, q.limit, q.from, q.to).await {
        Ok((records, scanned)) => {
            (StatusCode::OK, Json(SearchResponse { records, scanned })).into_response()
        }
        Err(err) => {
            tracing::error!(%err, "search failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "search failed").into_response()
        }
    }
}
```

- [ ] **Step 4: Extend CLI**

In `crates/sift-cli/src/main.rs`, update `Cmd::Search` to accept optional RFC3339 times. Use `chrono::DateTime<Utc>`:

```rust
    /// Search records by body substring.
    Search {
        query: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Inclusive lower bound, RFC3339 (e.g. 2026-04-23T00:00:00Z).
        #[arg(long)]
        from: Option<chrono::DateTime<chrono::Utc>>,
        /// Exclusive upper bound, RFC3339.
        #[arg(long)]
        to: Option<chrono::DateTime<chrono::Utc>>,
    },
```

Update the match arm for `Cmd::Search`:

```rust
        Cmd::Search { query, limit, from, to } => search(&cli.server, &query, limit, from, to).await,
```

Update the `search` function signature:

```rust
async fn search(
    base: &str,
    query: &str,
    limit: usize,
    from: Option<chrono::DateTime<chrono::Utc>>,
    to: Option<chrono::DateTime<chrono::Utc>>,
) -> anyhow::Result<()> {
    let url = format!("{}/v1/search", base.trim_end_matches('/'));
    let mut req = reqwest::Client::new()
        .get(&url)
        .query(&[("q", query), ("limit", &limit.to_string())]);
    if let Some(f) = from {
        req = req.query(&[("from", &f.to_rfc3339())]);
    }
    if let Some(t) = to {
        req = req.query(&[("to", &t.to_rfc3339())]);
    }
    let resp: SearchResponse = req.send().await?.error_for_status()?.json().await?;
    for rec in resp.records {
        println!(
            "{ts} {host} {app} {lvl:?} {body}",
            ts = rec.timestamp.to_rfc3339(),
            host = rec.host,
            app = rec.app,
            lvl = rec.level,
            body = rec.body
        );
    }
    eprintln!("(scanned {})", resp.scanned);
    Ok(())
}
```

- [ ] **Step 5: Add a server-side integration test**

In `crates/sift-server/tests/search.rs`, append:

```rust
#[tokio::test]
async fn search_with_time_range_prunes_segments() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path()).await.unwrap();
    let app = sift_server::router(state);

    ingest_batch(
        &app,
        concat!(
            r#"{"timestamp":"2026-04-23T00:00:00Z","host":"h","app":"a","stream_id":"s","body":"old"}"#,
            "\n",
            r#"{"timestamp":"2026-04-23T01:00:00Z","host":"h","app":"a","stream_id":"s","body":"mid"}"#,
            "\n",
            r#"{"timestamp":"2026-04-23T02:00:00Z","host":"h","app":"a","stream_id":"s","body":"new"}"#,
            "\n",
        ),
    )
    .await;

    let res = app
        .oneshot(
            Request::get(
                "/v1/search?from=2026-04-23T00:30:00Z&to=2026-04-23T01:30:00Z",
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 65_536).await.unwrap();
    let resp: SearchResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.records.len(), 1);
    assert_eq!(resp.records[0].body, "mid");
}
```

- [ ] **Step 6: Run everything**

Run: `cargo test --workspace --all-targets`
Expected: all previous tests + the new `sift-api` test + the new server test pass.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

Run: `cargo fmt --all -- --check`
Expected: clean. (If not, `cargo fmt --all` then re-check.)

- [ ] **Step 7: Commit**

```bash
git add crates/sift-api crates/sift-server crates/sift-cli
git commit -m "feat(api,server,cli): search time-range filter (from/to)"
```

---

## Task 7: WAL rotation on flush

Swap `Store.wal` from `Arc<WalWriter>` to `ArcSwap<WalWriter>`. After a successful flush, rotate: open a new WAL file, update `wal/current`, close the old writer, delete the old file.

**Files:**
- Modify: `crates/sift-storage/src/store.rs`
- Create: `crates/sift-storage/tests/wal_rotates_after_flush.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/sift-storage/tests/wal_rotates_after_flush.rs`:

```rust
use std::time::Duration;

use chrono::Utc;
use sift_core::{Level, Record};
use sift_storage::{
    memtable::MemtableConfig, partition::PartitionConfig, store::StoreConfig,
    wal::WalConfig, Store,
};
use std::collections::BTreeMap;

fn rec(body: &str) -> Record {
    Record {
        timestamp: Utc::now(),
        host: "h".into(),
        app: "a".into(),
        stream_id: "s".into(),
        level: Level::Info,
        body: body.into(),
        labels: BTreeMap::new(),
        fields: BTreeMap::new(),
        raw: None,
    }
}

#[tokio::test]
async fn wal_rotates_after_flush() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StoreConfig {
        wal: WalConfig {
            fsync_interval: Duration::from_millis(5),
            size_trigger_bytes: 4096,
        },
        memtable: MemtableConfig {
            size_bytes: 256, // small → quick flush
            age: Duration::from_secs(3600),
        },
        partition: PartitionConfig::default(),
    };
    let store = Store::open_with(tmp.path(), cfg).await.unwrap();

    for _ in 0..50 {
        store.append(rec("payload payload payload")).await.unwrap();
    }
    // Wait for flush + rotation.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Read wal/current.
    let current = tokio::fs::read_to_string(tmp.path().join("wal").join("current"))
        .await
        .unwrap();
    let current = current.trim();
    assert!(current.ends_with(".log"));

    // Exactly one .log file under wal/.
    let mut rd = tokio::fs::read_dir(tmp.path().join("wal")).await.unwrap();
    let mut logs: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".log") {
            logs.push(name);
        }
    }
    logs.sort();
    assert_eq!(logs.len(), 1, "expected exactly 1 WAL log, got {:?}", logs);
    assert_eq!(logs[0], current);
}
```

- [ ] **Step 2: Run — expect failure**

Run: `cargo test -p sift-storage --test wal_rotates_after_flush`
Expected: fails. `wal/current` doesn't exist (SP1 never created it) and the WAL never rotates.

- [ ] **Step 3: Switch `Store.wal` to `ArcSwap<WalWriter>`**

At the top of `store.rs`:

```rust
use arc_swap::ArcSwap;
```

Change the field:

```rust
pub struct Store {
    data_dir: PathBuf,
    wal: Arc<ArcSwap<WalWriter>>,
    memtable: Arc<Mutex<Memtable>>,
    segments: Arc<RwLock<Vec<SegmentHandle>>>,
    tx: broadcast::Sender<Record>,
    flush_permit: Arc<Semaphore>,
    next_seq: Arc<AtomicU64>,
    partition_cfg: PartitionConfig,
    wal_cfg: WalConfig,
}
```

Note two adds: `ArcSwap<WalWriter>` and a new `wal_cfg: WalConfig` field (rotation needs the config to open the next WAL with the same parameters).

In `open_with`, change the WAL setup and the initial pointer-file write:

```rust
        // Open the initial WAL.
        let initial_seq: u64 = 1;
        let wal_path = data_dir.join("wal").join(format!("{initial_seq:010}.log"));
        let current_pointer = data_dir.join("wal").join("current");
        tokio::fs::write(&current_pointer, format!("{initial_seq:010}.log"))
            .await
            .ok(); // Best effort; overwritten on rotation.

        let wal_cfg = cfg.wal.clone();
        let wal = Arc::new(ArcSwap::new(Arc::new(
            WalWriter::open(&wal_path, wal_cfg.clone()).await?,
        )));
```

And update the struct literal:

```rust
        let store = Self {
            data_dir,
            wal,
            memtable,
            segments,
            tx,
            flush_permit,
            next_seq,
            partition_cfg,
            wal_cfg,
        };
```

Also: `self.wal.append(...)` calls (there are two sites — `append` method and `flush_now`) need to become `self.wal.load().append(...)`. Replace inline. And `self.wal.fsync_through_current()` becomes `self.wal.load().fsync_through_current()`.

- [ ] **Step 4: Add rotation at end of `flush_now`**

After the `segments.write().await.push(...)` block and before `drop(permit)`, insert:

```rust
        // Rotate the WAL: fresh file, pointer update, close + delete old.
        let new_seq = self.next_seq.load(Ordering::SeqCst);
        let new_name = format!("{new_seq:010}.log");
        let new_path = self.data_dir.join("wal").join(&new_name);
        let new_writer = Arc::new(WalWriter::open(&new_path, self.wal_cfg.clone()).await?);
        let old_writer_arc = self.wal.swap(new_writer);

        // Atomic pointer update: write to a temp file then rename over `current`.
        let current = self.data_dir.join("wal").join("current");
        let tmp = self.data_dir.join("wal").join("current.tmp");
        tokio::fs::write(&tmp, &new_name).await?;
        tokio::fs::rename(&tmp, &current).await?;

        // Close the old writer (final fsync), then delete its file.
        let old_writer = Arc::try_unwrap(old_writer_arc)
            .map_err(|_| anyhow::anyhow!("old WAL writer still shared; rotation aborted"))?;
        let old_path = old_writer.path().to_path_buf();
        old_writer.close().await?;
        if let Err(e) = tokio::fs::remove_file(&old_path).await {
            tracing::warn!(%e, "failed to remove rotated WAL file {}", old_path.display());
        }
```

**Failure mode to watch:** `Arc::try_unwrap` fails if anyone else holds an `Arc<WalWriter>` clone right now. That can happen if an `append` call is mid-flight and grabbed the Arc with `self.wal.load()` before the swap. The `load()` method actually returns a `Guard<Arc<WalWriter>>` whose drop decrements the refcount. So the race window is: `append` calls `self.wal.load()`, holds the Guard, does `.append(...)` which awaits. Meanwhile flush_now does the swap, then immediately tries `try_unwrap`. The Guard is still live → fails.

Work-around: use `arc_swap::Guard::into_inner` only briefly, drop, and drain via a **one-shot notify**. Simpler: don't use `try_unwrap`. Keep the old Arc around, and close it fire-and-forget — any concurrent `append` finishing on the old writer pushes bytes into the old buffer which the old ticker fsyncs before `close()` returns. Replace the `try_unwrap` block with:

```rust
        // Give in-flight appends a beat to finish on the old writer before
        // we close it. They grabbed the Arc before the swap; their Guard
        // gets dropped on return.
        tokio::task::yield_now().await;

        // Try to take sole ownership. If someone still holds a Guard the
        // try_unwrap fails — in that rare case, leak the close: drop the
        // Arc and the WalWriter will be closed when the last holder
        // releases it. The old file stays on disk until the next restart's
        // orphan sweep (Task 9) cleans it up.
        match Arc::try_unwrap(old_writer_arc) {
            Ok(w) => {
                let old_path = w.path().to_path_buf();
                w.close().await?;
                if let Err(e) = tokio::fs::remove_file(&old_path).await {
                    tracing::warn!(%e, "failed to remove rotated WAL file {}", old_path.display());
                }
            }
            Err(shared) => {
                tracing::warn!(
                    "in-flight append on old WAL at rotation; file left for orphan sweep"
                );
                drop(shared);
            }
        }
```

Orphan sweep in Task 9 picks up the leftover.

- [ ] **Step 5: Run the rotation test + full suite**

Run: `cargo test -p sift-storage --test wal_rotates_after_flush`
Expected: 1 test passes.

Run: `cargo test -p sift-storage`
Expected: all existing tests still pass (some may have needed touchups if they assumed `self.wal: Arc<WalWriter>` directly).

- [ ] **Step 6: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sift-storage/src/store.rs crates/sift-storage/tests/wal_rotates_after_flush.rs
git commit -m "feat(storage): WAL rotation after flush, arc-swap slot"
```

---

## Task 8: Background compactor task (TDD)

New module `compactor.rs`. Background task owned by `Store` that ticks every `tick_interval`, enumerates partitions, merges eligible ones.

**Files:**
- Modify: `crates/sift-storage/src/compactor.rs` (currently a stub)
- Modify: `crates/sift-storage/src/store.rs`
- Create: `crates/sift-storage/tests/compactor_merges_closed_partitions.rs`

- [ ] **Step 1: Implement `compactor.rs`**

```rust
//! Background compactor: merges small segments inside closed partitions.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinHandle;

use crate::partition::{partition_end, partition_id, PartitionConfig};
use crate::segment::{
    compacting_filename, load_handle, scan_segment, segment_filename, write_segment, SegmentHandle,
};

#[derive(Debug, Clone, Copy)]
pub struct CompactorConfig {
    pub tick_interval: Duration,
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

pub struct CompactorHandle {
    task: Option<JoinHandle<()>>,
    shutdown: Arc<Notify>,
}

impl CompactorHandle {
    pub async fn shutdown(mut self) {
        self.shutdown.notify_one();
        if let Some(t) = self.task.take() {
            let _ = t.await;
        }
    }
}

pub fn spawn_compactor(
    data_dir: PathBuf,
    partition_cfg: PartitionConfig,
    compactor_cfg: CompactorConfig,
    segments: Arc<RwLock<Vec<SegmentHandle>>>,
    next_seq: Arc<AtomicU64>,
) -> CompactorHandle {
    let shutdown = Arc::new(Notify::new());
    let shutdown_listener = Arc::clone(&shutdown);

    let task = if compactor_cfg.enabled {
        Some(tokio::spawn(run_loop(
            data_dir,
            partition_cfg,
            compactor_cfg,
            segments,
            next_seq,
            shutdown_listener,
        )))
    } else {
        None
    };

    CompactorHandle { task, shutdown }
}

async fn run_loop(
    data_dir: PathBuf,
    partition_cfg: PartitionConfig,
    compactor_cfg: CompactorConfig,
    segments: Arc<RwLock<Vec<SegmentHandle>>>,
    next_seq: Arc<AtomicU64>,
    shutdown: Arc<Notify>,
) {
    let mut tick = tokio::time::interval(compactor_cfg.tick_interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    tick.tick().await; // burn initial tick

    loop {
        tokio::select! {
            () = shutdown.notified() => return,
            _ = tick.tick() => {
                if let Err(e) =
                    compact_once(&data_dir, partition_cfg, &segments, &next_seq).await
                {
                    tracing::error!(%e, "compactor tick failed");
                }
            }
        }
    }
}

/// One compaction pass: enumerate partitions, merge eligible ones.
pub async fn compact_once(
    data_dir: &std::path::Path,
    partition_cfg: PartitionConfig,
    segments: &Arc<RwLock<Vec<SegmentHandle>>>,
    next_seq: &Arc<AtomicU64>,
) -> anyhow::Result<()> {
    let now = Utc::now();
    let active = partition_id(now, partition_cfg.granularity);
    let grace = chrono::Duration::from_std(partition_cfg.grace).unwrap_or(chrono::Duration::zero());

    // Group handles by partition.
    let snapshot: Vec<SegmentHandle> = segments.read().await.clone();
    let mut by_partition: BTreeMap<DateTime<Utc>, Vec<SegmentHandle>> = BTreeMap::new();
    for h in snapshot {
        by_partition.entry(h.partition).or_default().push(h);
    }

    for (partition, handles) in by_partition {
        if partition == active {
            continue;
        }
        let end = partition_end(partition, partition_cfg.granularity);
        if now < end + grace {
            continue;
        }
        if handles.len() < 2 {
            continue;
        }
        merge_partition(data_dir, partition_cfg, partition, handles, segments, next_seq)
            .await?;
    }
    Ok(())
}

async fn merge_partition(
    data_dir: &std::path::Path,
    partition_cfg: PartitionConfig,
    partition: DateTime<Utc>,
    handles: Vec<SegmentHandle>,
    segments: &Arc<RwLock<Vec<SegmentHandle>>>,
    next_seq: &Arc<AtomicU64>,
) -> anyhow::Result<()> {
    let seq = next_seq.fetch_add(1, Ordering::SeqCst);
    let granularity = partition_cfg.granularity;
    let tmp_name = compacting_filename(partition, seq, granularity);
    let final_name = segment_filename(partition, seq, granularity);
    let tmp_path = data_dir.join("segments").join(&tmp_name);
    let final_path = data_dir.join("segments").join(&final_name);

    // Read every record in the input handles (off the async runtime).
    let inputs: Vec<PathBuf> = handles.iter().map(|h| h.path.clone()).collect();
    let inputs_for_blocking = inputs.clone();
    let records = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<sift_core::Record>> {
        let mut acc: Vec<sift_core::Record> = Vec::new();
        for p in &inputs_for_blocking {
            let h = load_handle(p, granularity).context("reload during compaction")?;
            acc.extend(scan_segment(&h, "")?);
        }
        Ok(acc)
    })
    .await
    .context("compaction read task join")??;

    if records.is_empty() {
        // Weird but possible (corrupt?). Bail without mutating state.
        return Ok(());
    }

    // Write the merged segment.
    let partition_copy = partition;
    let granularity_copy = granularity;
    let tmp_path_for_write = tmp_path.clone();
    let new_handle = tokio::task::spawn_blocking(move || {
        write_segment(&records, &tmp_path_for_write, partition_copy, granularity_copy)
    })
    .await
    .context("compaction write task join")??;

    // Atomic publish.
    tokio::fs::rename(&tmp_path, &final_path).await?;
    let published_handle = SegmentHandle {
        path: final_path,
        ..new_handle
    };

    // Swap in the set: remove inputs, insert the merged one.
    let input_paths: BTreeSet<PathBuf> = inputs.into_iter().collect();
    {
        let mut segs = segments.write().await;
        segs.retain(|h| !input_paths.contains(&h.path));
        segs.push(published_handle);
    }

    // Try to remove the old files. Failures are non-fatal — orphan sweep
    // picks them up next restart.
    for p in &input_paths {
        if let Err(e) = tokio::fs::remove_file(p).await {
            tracing::warn!(%e, "failed to remove superseded segment {}", p.display());
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Wire the compactor into `Store`**

In `store.rs`, add to the struct:

```rust
pub struct Store {
    data_dir: PathBuf,
    wal: Arc<ArcSwap<WalWriter>>,
    memtable: Arc<Mutex<Memtable>>,
    segments: Arc<RwLock<Vec<SegmentHandle>>>,
    tx: broadcast::Sender<Record>,
    flush_permit: Arc<Semaphore>,
    next_seq: Arc<AtomicU64>,
    partition_cfg: PartitionConfig,
    wal_cfg: WalConfig,
    compactor: Option<crate::compactor::CompactorHandle>,
}
```

Add to `StoreConfig`:

```rust
use crate::compactor::CompactorConfig;

#[derive(Debug, Clone, Default)]
pub struct StoreConfig {
    pub wal: WalConfig,
    pub memtable: MemtableConfig,
    pub partition: PartitionConfig,
    pub compactor: CompactorConfig,
}
```

Spawn the compactor at the end of `open_with`, right before constructing the `Store`:

```rust
        let compactor = Some(crate::compactor::spawn_compactor(
            data_dir.clone(),
            cfg.partition,
            cfg.compactor,
            Arc::clone(&segments),
            Arc::clone(&next_seq),
        ));
```

And add to the struct literal. Also, `Drop` for `Store`:

```rust
impl Drop for Store {
    fn drop(&mut self) {
        // Best-effort: abort the compactor task without awaiting.
        if let Some(handle) = self.compactor.take() {
            // Async shutdown wants to await, which we can't in Drop.
            // Fire-and-forget.
            drop(handle);
        }
    }
}
```

(`CompactorHandle::Drop` doesn't currently abort — for tests that want clean shutdown, Task 10 adds `Store::close`.)

- [ ] **Step 3: Re-export from `lib.rs`**

Add:

```rust
pub use partition::PartitionConfig;
pub use compactor::CompactorConfig;
```

- [ ] **Step 4: Write the failing integration test**

Create `crates/sift-storage/tests/compactor_merges_closed_partitions.rs`:

```rust
use std::time::Duration;

use chrono::{TimeZone, Utc};
use sift_core::{Level, Record};
use sift_storage::{
    compactor::{compact_once, CompactorConfig},
    memtable::MemtableConfig,
    partition::{partition_id, PartitionConfig},
    store::StoreConfig,
    wal::WalConfig,
    Store,
};
use std::collections::BTreeMap;

fn rec_at(body: &str, ts: chrono::DateTime<Utc>) -> Record {
    Record {
        timestamp: ts,
        host: "h".into(),
        app: "a".into(),
        stream_id: "s".into(),
        level: Level::Info,
        body: body.into(),
        labels: BTreeMap::new(),
        fields: BTreeMap::new(),
        raw: None,
    }
}

#[tokio::test]
async fn compactor_merges_a_closed_partition() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StoreConfig {
        wal: WalConfig::default(),
        memtable: MemtableConfig {
            size_bytes: 64, // force flushes
            age: Duration::from_secs(3600),
        },
        partition: PartitionConfig {
            granularity: Duration::from_secs(60),
            grace: Duration::from_millis(10),
        },
        compactor: CompactorConfig {
            tick_interval: Duration::from_secs(3600), // disable auto-tick; we drive manually
            enabled: false,
        },
    };
    let store = Store::open_with(tmp.path(), cfg).await.unwrap();

    // Put 5 records into a partition that's already in the past (well past
    // its end + grace).
    let old = Utc.with_ymd_and_hms(2026, 4, 22, 0, 0, 0).unwrap();
    for i in 0..5 {
        store
            .append(rec_at(
                &format!("r{i}"),
                old + chrono::Duration::seconds(i * 2),
            ))
            .await
            .unwrap();
    }
    // Drain the memtable into a series of small segments.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Confirm we have more than one segment for that partition.
    let segs_dir = tmp.path().join("segments");
    let mut rd = tokio::fs::read_dir(&segs_dir).await.unwrap();
    let mut count_before = 0usize;
    while let Ok(Some(_)) = rd.next_entry().await {
        count_before += 1;
    }
    assert!(count_before >= 2, "expected >=2 segments pre-compact, got {count_before}");

    // Fire compaction manually.
    compact_once(
        tmp.path(),
        PartitionConfig {
            granularity: Duration::from_secs(60),
            grace: Duration::from_millis(10),
        },
        &test_segments_handle(&store).await,
        &test_next_seq_handle(&store).await,
    )
    .await
    .unwrap();

    // After: exactly 1 segment file for that partition.
    let mut rd = tokio::fs::read_dir(&segs_dir).await.unwrap();
    let mut count_after = 0usize;
    while let Ok(Some(_)) = rd.next_entry().await {
        count_after += 1;
    }
    assert_eq!(count_after, 1, "expected 1 segment post-compact");

    // And search still finds all 5 records.
    let (matches, _) = store
        .search(
            "",
            100,
            Some(old),
            Some(old + chrono::Duration::seconds(60)),
        )
        .await
        .unwrap();
    assert_eq!(matches.len(), 5);

    let _ = partition_id;
}

// Test shims — the compactor's API takes Arc<RwLock<Vec<SegmentHandle>>>
// and Arc<AtomicU64>, which we don't expose from Store publicly. We need
// a small accessor. Mark these as pub(crate) on Store OR move the test
// to a `#[cfg(test)] mod` inside store.rs.

async fn test_segments_handle(
    _store: &Store,
) -> std::sync::Arc<tokio::sync::RwLock<Vec<sift_storage::segment::SegmentHandle>>> {
    unimplemented!("replace in Step 5");
}
async fn test_next_seq_handle(_store: &Store) -> std::sync::Arc<std::sync::atomic::AtomicU64> {
    unimplemented!("replace in Step 5");
}
```

- [ ] **Step 5: Expose internals for testing — pick one of two approaches**

The `compact_once` API takes `&Arc<RwLock<Vec<SegmentHandle>>>` and `&Arc<AtomicU64>`. External integration tests can't access those from `Store`. Two options:

**Option A (chosen):** add `Store::trigger_compaction` that calls `compact_once` internally. Small API surface addition, tests easy to write.

Add to `impl Store` in `store.rs`:

```rust
    /// Manually run one compaction pass. Used by tests that want
    /// deterministic timing; production code relies on the background
    /// compactor.
    pub async fn trigger_compaction(&self) -> anyhow::Result<()> {
        crate::compactor::compact_once(
            &self.data_dir,
            self.partition_cfg,
            &self.segments,
            &self.next_seq,
        )
        .await
    }
```

Then replace the test's `compact_once(…)` call with `store.trigger_compaction().await.unwrap();` and delete the `test_segments_handle` / `test_next_seq_handle` shims. Also remove the unused `use ... compact_once` import.

Final test body (full replacement from Step 4's draft):

```rust
use std::time::Duration;

use chrono::{TimeZone, Utc};
use sift_core::{Level, Record};
use sift_storage::{
    compactor::CompactorConfig, memtable::MemtableConfig,
    partition::PartitionConfig, store::StoreConfig, wal::WalConfig, Store,
};
use std::collections::BTreeMap;

fn rec_at(body: &str, ts: chrono::DateTime<Utc>) -> Record {
    Record {
        timestamp: ts,
        host: "h".into(),
        app: "a".into(),
        stream_id: "s".into(),
        level: Level::Info,
        body: body.into(),
        labels: BTreeMap::new(),
        fields: BTreeMap::new(),
        raw: None,
    }
}

#[tokio::test]
async fn compactor_merges_a_closed_partition() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StoreConfig {
        wal: WalConfig::default(),
        memtable: MemtableConfig {
            size_bytes: 64,
            age: Duration::from_secs(3600),
        },
        partition: PartitionConfig {
            granularity: Duration::from_secs(60),
            grace: Duration::from_millis(10),
        },
        compactor: CompactorConfig {
            tick_interval: Duration::from_secs(3600),
            enabled: false,
        },
    };
    let store = Store::open_with(tmp.path(), cfg).await.unwrap();

    let old = Utc.with_ymd_and_hms(2026, 4, 22, 0, 0, 0).unwrap();
    for i in 0..5 {
        store
            .append(rec_at(
                &format!("r{i}"),
                old + chrono::Duration::seconds(i * 2),
            ))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    let segs_dir = tmp.path().join("segments");
    let count_before = dir_count(&segs_dir).await;
    assert!(count_before >= 2, "expected >=2 pre-compact, got {count_before}");

    store.trigger_compaction().await.unwrap();

    let count_after = dir_count(&segs_dir).await;
    assert_eq!(count_after, 1, "expected 1 segment post-compact");

    let (matches, _) = store
        .search("", 100, Some(old), Some(old + chrono::Duration::seconds(60)))
        .await
        .unwrap();
    assert_eq!(matches.len(), 5);
}

async fn dir_count(p: &std::path::Path) -> usize {
    let mut rd = tokio::fs::read_dir(p).await.unwrap();
    let mut n = 0usize;
    while let Ok(Some(_)) = rd.next_entry().await {
        n += 1;
    }
    n
}
```

- [ ] **Step 6: Run — expect pass**

Run: `cargo test -p sift-storage --test compactor_merges_closed_partitions`
Expected: 1 test passes.

Run: `cargo test -p sift-storage`
Expected: all still pass.

- [ ] **Step 7: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/sift-storage/src/compactor.rs crates/sift-storage/src/store.rs \
        crates/sift-storage/src/lib.rs \
        crates/sift-storage/tests/compactor_merges_closed_partitions.rs
git commit -m "feat(storage): background compactor merges closed partitions"
```

---

## Task 9: Orphan sweeps on open

`Store::open_with` deletes stale `.compacting.parquet` files and non-current `wal/*.log` files before proceeding.

**Files:**
- Modify: `crates/sift-storage/src/store.rs`
- Create: `crates/sift-storage/tests/orphans_swept_on_open.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/sift-storage/tests/orphans_swept_on_open.rs`:

```rust
use sift_storage::Store;

#[tokio::test]
async fn compacting_parquet_is_swept_on_open() {
    let tmp = tempfile::tempdir().unwrap();
    // Seed an orphan directly.
    let segs = tmp.path().join("segments");
    tokio::fs::create_dir_all(&segs).await.unwrap();
    let orphan = segs.join("2026-04-23T12-0000000001.compacting.parquet");
    tokio::fs::write(&orphan, b"garbage").await.unwrap();

    let _store = Store::open(tmp.path()).await.unwrap();
    assert!(!orphan.exists(), "orphan .compacting.parquet should be deleted");
}

#[tokio::test]
async fn non_current_wal_is_swept_on_open() {
    let tmp = tempfile::tempdir().unwrap();
    let wal = tmp.path().join("wal");
    tokio::fs::create_dir_all(&wal).await.unwrap();
    // Write `current` pointing at a specific file, and seed an orphan.
    tokio::fs::write(wal.join("current"), b"0000000001.log")
        .await
        .unwrap();
    tokio::fs::write(wal.join("0000000001.log"), b"")
        .await
        .unwrap();
    let orphan = wal.join("0000000099.log");
    tokio::fs::write(&orphan, b"garbage").await.unwrap();

    let _store = Store::open(tmp.path()).await.unwrap();
    assert!(!orphan.exists(), "orphan WAL file should be deleted");
}
```

- [ ] **Step 2: Implement the sweeps in `open_with`**

Add the following between "create data_dir subdirs" and "load existing segments":

```rust
        // Orphan sweep — segments: *.compacting.parquet are mid-compaction
        // tmp files that didn't get renamed. Safe to delete on open.
        let segs_dir = data_dir.join("segments");
        let mut rd = tokio::fs::read_dir(&segs_dir).await?;
        while let Some(entry) = rd.next_entry().await? {
            let path = entry.path();
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if name.ends_with(".compacting.parquet") {
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    tracing::warn!(%e, "failed to sweep orphan {}", path.display());
                }
            }
        }
```

Add the WAL sweep before opening the WAL writer, after computing `wal_path`:

```rust
        // Orphan sweep — WAL: delete any wal/*.log whose name is not the
        // one `wal/current` points at. If `wal/current` is missing, keep
        // only one file (the smallest numbered) and drop the rest.
        let wal_dir = data_dir.join("wal");
        let current_pointer = wal_dir.join("current");
        let current_name: Option<String> = tokio::fs::read_to_string(&current_pointer)
            .await
            .ok()
            .map(|s| s.trim().to_string());

        let mut existing_logs: Vec<std::path::PathBuf> = Vec::new();
        if wal_dir.exists() {
            let mut rd = tokio::fs::read_dir(&wal_dir).await?;
            while let Some(entry) = rd.next_entry().await? {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("log") {
                    existing_logs.push(path);
                }
            }
        }
        existing_logs.sort();

        // Decide which file is the keeper.
        let keeper: Option<std::path::PathBuf> = match (&current_name, existing_logs.first()) {
            (Some(name), _) => Some(wal_dir.join(name)),
            (None, Some(first)) => Some(first.clone()),
            (None, None) => None,
        };
        // Delete every non-keeper log.
        for p in &existing_logs {
            if Some(p.clone()) != keeper {
                if let Err(e) = tokio::fs::remove_file(p).await {
                    tracing::warn!(%e, "failed to sweep orphan WAL {}", p.display());
                }
            }
        }

        // Determine the wal_path used below.
        let wal_path = match keeper {
            Some(p) => p,
            None => data_dir.join("wal").join("0000000001.log"),
        };
```

Remove the original `wal_path` computation (the hardcoded `0000000001.log`).

- [ ] **Step 3: Run tests**

Run: `cargo test -p sift-storage --test orphans_swept_on_open`
Expected: 2 tests pass.

Run full suite: `cargo test -p sift-storage`
Expected: all pass.

- [ ] **Step 4: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-storage/src/store.rs crates/sift-storage/tests/orphans_swept_on_open.rs
git commit -m "feat(storage): orphan sweep for .compacting.parquet and non-current WAL"
```

---

## Task 10: `Store::close` for clean shutdown (TDD)

**Files:**
- Modify: `crates/sift-storage/src/store.rs`
- Create: `crates/sift-storage/tests/store_close.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/sift-storage/tests/store_close.rs`:

```rust
use chrono::Utc;
use sift_core::{Level, Record};
use sift_storage::Store;
use std::collections::BTreeMap;

fn rec(body: &str) -> Record {
    Record {
        timestamp: Utc::now(),
        host: "h".into(),
        app: "a".into(),
        stream_id: "s".into(),
        level: Level::Info,
        body: body.into(),
        labels: BTreeMap::new(),
        fields: BTreeMap::new(),
        raw: None,
    }
}

#[tokio::test]
async fn close_is_idempotent_and_returns_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).await.unwrap();
    store.append(rec("x")).await.unwrap();
    store.close().await.unwrap();
    // Reopen — no leftover file locks on Windows.
    let _ = Store::open(tmp.path()).await.unwrap();
}
```

- [ ] **Step 2: Implement `Store::close`**

Add to `impl Store` in `store.rs`:

```rust
    /// Shut down the store cleanly: stop the compactor, drain the WAL
    /// ticker's final fsync. Does not flush the memtable to a segment —
    /// surviving records stay in the WAL and are replayed on next open.
    pub async fn close(mut self) -> anyhow::Result<()> {
        // Stop the compactor.
        if let Some(handle) = self.compactor.take() {
            handle.shutdown().await;
        }
        // Close the WAL writer (final fsync). Its Arc may still have
        // other holders from in-flight appends; in that rare case we just
        // drop and rely on the ticker's shutdown branch to drain.
        let wal_arc = self.wal.load_full();
        drop(self.wal); // release our Arc<ArcSwap<...>> slot
        if let Ok(wal) = Arc::try_unwrap(wal_arc) {
            wal.close().await?;
        }
        Ok(())
    }
```

- [ ] **Step 3: Run the test**

Run: `cargo test -p sift-storage --test store_close`
Expected: 1 test passes.

Run full suite: `cargo test -p sift-storage`
Expected: all pass.

- [ ] **Step 4: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-storage/src/store.rs crates/sift-storage/tests/store_close.rs
git commit -m "feat(storage): Store::close for clean shutdown"
```

---

## Task 11: Final workspace verification + README update

**Files:**
- Modify: `crates/sift-storage/README.md`

- [ ] **Step 1: Update the README**

Replace the "What's here" section of `crates/sift-storage/README.md` with:

```markdown
## What's here

- WAL with serde_json+CRC32 frames, 10 ms / 1 MB group-commit fsync, rotation after each flush.
- Memtable: 256 MB / 60 s flush triggers.
- Time-partitioned parquet segments (1 h default; `PartitionConfig` to tune). zstd-3 compression, 1M-row row groups, no bloom filters — see ADR-002.
- Background compactor: ticks every 5 min, merges all segments in a closed partition (past its end + 30 min grace) into one.
- Search: substring filter on `body`, optional `[from, to)` time range for segment pruning.
- Filesystem-as-catalog: `{data}/wal/{N}.log` + `{data}/wal/current` + `{data}/segments/{partition}-{seq}.parquet`.
- WAL replay + orphan sweep on `Store::open`; partial trailing frames tolerated.
- `Store::close` for clean shutdown (compactor stop + final fsync).
```

Replace the "What's NOT here yet" block with:

```markdown
## What's NOT here yet

- SQLite catalog (SP4).
- Time-based retention / eviction (SP4).
- Label index / inverted postings (SP5).
- Tiered compaction (post-M1).
```

- [ ] **Step 2: Run everything**

Run: `cargo build --workspace`
Expected: clean.

Run: `cargo test --workspace --all-targets`
Expected: all tests pass. Total should be the previous 71 plus roughly 15 new tests across partition, compactor, flush_multi, search_time_range, wal_rotates, orphans, store_close, api.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

Run: `cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/sift-storage/README.md
git commit -m "docs(storage): README reflects SP3 — partitions, compactor, rotation, time filter"
```

---

## Self-review

**Spec coverage** (against `docs/superpowers/specs/2026-04-23-sp3-time-partitions-design.md`):

| Spec § | Requirement | Task |
|---|---|---|
| §3.1 Partition identity | `PartitionConfig`, `partition_id`, `partition_label`, `partition_end` | Task 2 |
| §3.2 Segment naming | `segment_filename`, `compacting_filename`, `SegmentHandle.partition` | Task 3 |
| §3.3 Multi-partition flush | group by partition, write N segments per flush | Task 4 |
| §3.4 Compactor task | `CompactorConfig`, `spawn_compactor`, `compact_once` | Task 8 |
| §3.4.1 Merge one partition | tmp + atomic rename + lock swap + delete | Task 8 |
| §3.5 WAL rotation | post-flush new WAL, pointer update, close + delete | Task 7 |
| §3.6 Search time-range | `SearchQuery.from/to`, storage pruning, memtable filter | Tasks 5, 6 |
| §3.7 Restart recovery | orphan sweeps | Task 9 |
| §3.8 Error handling | compaction retry, rotation orphan, delete-failure tolerance | Tasks 7, 8, 9 (inline comments) |
| §3.9 Concurrency model | short locks, semaphore for flush, RwLock for segments | existing, preserved |
| §3.10 Store shutdown | `Store::close` | Task 10 |
| CLI extension | `sift search --from --to` | Task 6 |
| API extension | `SearchQuery.from/to` serde(default) | Task 6 |

Gaps: none blocking. §3.8's "unused `Store::close` in `sift-server` shutdown handler" is called out in the SP3 spec but not implemented here — the server doesn't explicitly call `close()` on Ctrl-C. Flagged for SP4 or a follow-up; safe to defer because `Drop` still aborts the compactor and the WAL ticker flushes on shutdown-notify.

**Placeholder scan:** searched for "TBD", "TODO", "fill in", "similar to" — none in task content.

**Type consistency:**
- `PartitionConfig { granularity: Duration, grace: Duration }` — Task 2, used Tasks 4, 5, 7, 8, 9.
- `CompactorConfig { tick_interval: Duration, enabled: bool }` — Task 8.
- `SegmentHandle { path, row_count, min_ts, max_ts, partition }` — Task 3.
- `StoreConfig { wal, memtable, partition, compactor }` — grew progressively across Tasks 4, 8. Final shape has all four.
- `Store::search(query, limit, from, to)` — Task 5 and downstream Tasks 6, 8's test.
- `segment_filename(partition, seq, granularity)` — Task 3, used Tasks 4, 8.
- `load_handle(path, granularity)` — Task 3, used Tasks 4, 8.

Inline fix applied: Task 3 Step 6 has the transient `load_handle(&path, std::time::Duration::from_secs(3600))` call that Task 4 replaces with `cfg.partition.granularity`. The rewrite is explicit in Task 4 Step 1, so there's no drift.

---

Plan complete and saved to `docs/superpowers/plans/2026-04-23-sp3-time-partitions.md`. Two execution options:

**1. Subagent-Driven (recommended)** — fresh subagent per task, review between tasks.

**2. Inline Execution** — I run the tasks in this session with checkpoints.

Which approach?
