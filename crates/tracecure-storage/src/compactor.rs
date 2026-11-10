//! Background compactor: merges small segments inside closed partitions and
//! deletes what retention says is too old.
//!
//! Both jobs are one pass over the closed partitions, because both end in
//! the same operation — replace a partition's segments with a rewritten one
//! (or with nothing) — and doing them together means a partition that needs
//! merging *and* pruning is read and written once, not twice.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinHandle;

use crate::partition::{partition_end, partition_id, PartitionConfig};
use crate::retention::{ExpiredRules, RetentionConfig};
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

/// Everything a compaction pass needs. Shared by the background loop and
/// `Store::trigger_compaction`, so a manual pass and a ticked one see the
/// same record of what has already been pruned.
pub struct Compaction {
    pub data_dir: PathBuf,
    pub partition: PartitionConfig,
    pub retention: RetentionConfig,
    pub segments: Arc<RwLock<Vec<SegmentHandle>>>,
    pub next_seq: Arc<AtomicU64>,
    /// Which retention rules each segment has already been rewritten for.
    ///
    /// Whether a segment still holds records of an expired stream cannot be
    /// told without reading it, and reading every old segment on every tick
    /// is not an option. So remember the answer: a segment whose entry
    /// equals the rules currently expired for its partition has nothing left
    /// to drop.
    ///
    /// In memory only. After a restart each old segment is read once more,
    /// found clean and recorded again; there is no catalog to keep it in
    /// (see ADR-003), and re-deriving it costs one pass.
    pruned: Mutex<HashMap<PathBuf, ExpiredRules>>,
}

impl Compaction {
    #[must_use]
    pub fn new(
        data_dir: PathBuf,
        partition: PartitionConfig,
        retention: RetentionConfig,
        segments: Arc<RwLock<Vec<SegmentHandle>>>,
        next_seq: Arc<AtomicU64>,
    ) -> Self {
        Self {
            data_dir,
            partition,
            retention,
            segments,
            next_seq,
            pruned: Mutex::new(HashMap::new()),
        }
    }

    fn pruned(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, ExpiredRules>> {
        // The map is only ever touched in short, panic-free critical
        // sections; if a panic elsewhere poisoned it, its contents are still
        // a valid (at worst incomplete) record.
        self.pruned
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// What one pass did. Returned so tests can assert on it and the loop can
/// log it; a pass that did nothing is all zeroes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompactionReport {
    /// Partitions rewritten into a single segment.
    pub partitions_rewritten: usize,
    /// Segment files removed: merge inputs and expired segments alike.
    pub segments_removed: usize,
    /// Records dropped because retention said they were too old.
    pub records_expired: u64,
}

impl CompactionReport {
    #[must_use]
    pub fn did_anything(&self) -> bool {
        *self != Self::default()
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

pub fn spawn_compactor(ctx: Arc<Compaction>, compactor_cfg: CompactorConfig) -> CompactorHandle {
    let shutdown = Arc::new(Notify::new());
    let shutdown_listener = Arc::clone(&shutdown);

    let task = if compactor_cfg.enabled {
        Some(tokio::spawn(run_loop(
            ctx,
            compactor_cfg,
            shutdown_listener,
        )))
    } else {
        None
    };

    CompactorHandle { task, shutdown }
}

async fn run_loop(ctx: Arc<Compaction>, compactor_cfg: CompactorConfig, shutdown: Arc<Notify>) {
    let mut tick = tokio::time::interval(compactor_cfg.tick_interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    tick.tick().await;

    loop {
        tokio::select! {
            () = shutdown.notified() => return,
            _ = tick.tick() => {
                match compact_once(&ctx).await {
                    Ok(report) if report.did_anything() => tracing::info!(
                        partitions_rewritten = report.partitions_rewritten,
                        segments_removed = report.segments_removed,
                        records_expired = report.records_expired,
                        "compaction pass"
                    ),
                    Ok(_) => {}
                    Err(e) => tracing::error!(%e, "compactor tick failed"),
                }
            }
        }
    }
}

/// One pass at the current time.
pub async fn compact_once(ctx: &Compaction) -> anyhow::Result<CompactionReport> {
    compact_once_at(ctx, Utc::now()).await
}

/// One pass as of `now`: for every closed partition, delete it outright if
/// retention has expired all of it, otherwise rewrite it if it has more than
/// one segment or holds records retention now wants gone.
pub async fn compact_once_at(
    ctx: &Compaction,
    now: DateTime<Utc>,
) -> anyhow::Result<CompactionReport> {
    let granularity = ctx.partition.granularity;
    let active = partition_id(now, granularity);
    let grace = chrono::Duration::from_std(ctx.partition.grace).unwrap_or(chrono::Duration::zero());

    let snapshot: Vec<SegmentHandle> = ctx.segments.read().await.clone();
    let mut by_partition: BTreeMap<DateTime<Utc>, Vec<SegmentHandle>> = BTreeMap::new();
    for h in snapshot {
        by_partition.entry(h.partition).or_default().push(h);
    }

    let mut report = CompactionReport::default();

    for (partition, handles) in by_partition {
        if partition == active {
            continue;
        }
        let end = partition_end(partition, granularity);
        if now < end + grace {
            continue;
        }

        // Everything in here is past every rule: no need to read it.
        if ctx.retention.fully_expired(end, now) {
            let rows: u64 = handles.iter().map(|h| h.row_count).sum();
            let paths: BTreeSet<PathBuf> = handles.into_iter().map(|h| h.path).collect();
            report.segments_removed += replace_segments(ctx, &paths, None).await;
            report.records_expired += rows;
            continue;
        }

        let expired = ctx.retention.expired_rules(end, now);
        let needs_prune = expired.as_ref().is_some_and(|rules| {
            let pruned = ctx.pruned();
            handles.iter().any(|h| pruned.get(&h.path) != Some(rules))
        });
        if handles.len() < 2 && !needs_prune {
            continue;
        }

        let outcome = rewrite_partition(ctx, partition, end, now, handles, expired).await?;
        report.partitions_rewritten += usize::from(outcome.rewritten);
        report.segments_removed += outcome.segments_removed;
        report.records_expired += outcome.records_expired;
    }
    Ok(report)
}

struct RewriteOutcome {
    /// False when the partition was read and found to need nothing.
    rewritten: bool,
    segments_removed: usize,
    records_expired: u64,
}

/// Read a partition's segments, drop what retention has expired, and replace
/// them with one segment — or with none, if nothing survived.
async fn rewrite_partition(
    ctx: &Compaction,
    partition: DateTime<Utc>,
    partition_end: DateTime<Utc>,
    now: DateTime<Utc>,
    handles: Vec<SegmentHandle>,
    expired: Option<ExpiredRules>,
) -> anyhow::Result<RewriteOutcome> {
    let granularity = ctx.partition.granularity;
    let inputs: Vec<PathBuf> = handles.iter().map(|h| h.path.clone()).collect();

    let inputs_for_blocking = inputs.clone();
    let retention = ctx.retention.clone();
    let (records, records_expired) = tokio::task::spawn_blocking(
        move || -> anyhow::Result<(Vec<tracecure_core::Record>, u64)> {
            let mut kept: Vec<tracecure_core::Record> = Vec::new();
            let mut dropped = 0u64;
            for p in &inputs_for_blocking {
                let h = load_handle(p, granularity).context("reload during compaction")?;
                for r in scan_segment(&h, "")? {
                    if retention.drops(&r.stream_id, partition_end, now) {
                        dropped += 1;
                    } else {
                        kept.push(r);
                    }
                }
            }
            Ok((kept, dropped))
        },
    )
    .await
    .context("compaction read task join")??;

    let input_paths: BTreeSet<PathBuf> = inputs.into_iter().collect();

    // One segment and nothing to drop: it is already what a rewrite would
    // produce. Record that it is clean and leave the file alone. Without
    // this, turning retention on — or restarting with it on — would rewrite
    // every old segment once just to learn it had nothing to remove.
    if input_paths.len() == 1 && records_expired == 0 {
        if let (Some(path), Some(rules)) = (input_paths.first(), expired) {
            ctx.pruned().insert(path.clone(), rules);
        }
        return Ok(RewriteOutcome {
            rewritten: false,
            segments_removed: 0,
            records_expired: 0,
        });
    }

    let replacement = if records.is_empty() {
        None
    } else {
        let seq = ctx.next_seq.fetch_add(1, Ordering::SeqCst);
        let segments_dir = ctx.data_dir.join("segments");
        let tmp_path = segments_dir.join(compacting_filename(partition, seq, granularity));
        let final_path = segments_dir.join(segment_filename(partition, seq, granularity));

        let tmp_path_for_write = tmp_path.clone();
        let new_handle = tokio::task::spawn_blocking(move || {
            write_segment(&records, &tmp_path_for_write, partition, granularity)
        })
        .await
        .context("compaction write task join")??;

        // Published by rename: a crash before this leaves a
        // `*.compacting.parquet` that the next open sweeps, and the inputs
        // untouched.
        tokio::fs::rename(&tmp_path, &final_path).await?;
        Some(SegmentHandle {
            path: final_path,
            ..new_handle
        })
    };

    if let (Some(handle), Some(rules)) = (&replacement, expired) {
        ctx.pruned().insert(handle.path.clone(), rules);
    }
    let segments_removed = replace_segments(ctx, &input_paths, replacement).await;

    Ok(RewriteOutcome {
        rewritten: true,
        segments_removed,
        records_expired,
    })
}

/// Swap `inputs` out of the catalog for `replacement` (if any), then delete
/// the input files. Returns how many files were removed.
///
/// The catalog changes first, under the write lock, so no new search can
/// pick up a segment that is about to disappear. A search already holding
/// the old list may still find a file gone; `Store::search` retries then.
async fn replace_segments(
    ctx: &Compaction,
    inputs: &BTreeSet<PathBuf>,
    replacement: Option<SegmentHandle>,
) -> usize {
    {
        let mut segs = ctx.segments.write().await;
        segs.retain(|h| !inputs.contains(&h.path));
        if let Some(handle) = replacement {
            segs.push(handle);
        }
    }
    {
        let mut pruned = ctx.pruned();
        for p in inputs {
            pruned.remove(p);
        }
    }

    let mut removed = 0;
    for p in inputs {
        match tokio::fs::remove_file(p).await {
            Ok(()) => removed += 1,
            Err(e) => tracing::warn!(%e, "failed to remove superseded segment {}", p.display()),
        }
    }
    removed
}
