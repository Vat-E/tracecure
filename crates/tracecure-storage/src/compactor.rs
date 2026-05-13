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
    tick.tick().await;

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
        merge_partition(
            data_dir,
            partition_cfg,
            partition,
            handles,
            segments,
            next_seq,
        )
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

    let inputs: Vec<PathBuf> = handles.iter().map(|h| h.path.clone()).collect();
    let inputs_for_blocking = inputs.clone();
    let records =
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<tracecure_core::Record>> {
            let mut acc: Vec<tracecure_core::Record> = Vec::new();
            for p in &inputs_for_blocking {
                let h = load_handle(p, granularity).context("reload during compaction")?;
                acc.extend(scan_segment(&h, "")?);
            }
            Ok(acc)
        })
        .await
        .context("compaction read task join")??;

    if records.is_empty() {
        return Ok(());
    }

    let partition_copy = partition;
    let granularity_copy = granularity;
    let tmp_path_for_write = tmp_path.clone();
    let new_handle = tokio::task::spawn_blocking(move || {
        write_segment(
            &records,
            &tmp_path_for_write,
            partition_copy,
            granularity_copy,
        )
    })
    .await
    .context("compaction write task join")??;

    tokio::fs::rename(&tmp_path, &final_path).await?;
    let published_handle = SegmentHandle {
        path: final_path,
        ..new_handle
    };

    let input_paths: BTreeSet<PathBuf> = inputs.into_iter().collect();
    {
        let mut segs = segments.write().await;
        segs.retain(|h| !input_paths.contains(&h.path));
        segs.push(published_handle);
    }

    for p in &input_paths {
        if let Err(e) = tokio::fs::remove_file(p).await {
            tracing::warn!(%e, "failed to remove superseded segment {}", p.display());
        }
    }

    Ok(())
}
