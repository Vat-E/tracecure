//! Top-level `Store` orchestrator. Thin: pushes real work into wal/memtable/segment.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Context;
use arc_swap::ArcSwap;
use sift_core::Record;
use tokio::sync::{broadcast, Mutex, RwLock, Semaphore};

use crate::memtable::{Memtable, MemtableConfig};
use crate::partition::PartitionConfig;
use crate::segment::{load_handle, write_segment, SegmentHandle};
use crate::wal::{decode_frame, FrameRead, WalConfig, WalWriter};

const BROADCAST_CAPACITY: usize = 1024;

/// Tunables. Production defaults match the design spec; tests can lower
/// thresholds to exercise the flush path quickly.
#[derive(Debug, Clone, Default)]
pub struct StoreConfig {
    pub wal: WalConfig,
    pub memtable: MemtableConfig,
    pub partition: PartitionConfig,
}

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

impl Store {
    pub async fn open(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        Self::open_with(data_dir, StoreConfig::default()).await
    }

    pub async fn open_with(data_dir: impl AsRef<Path>, cfg: StoreConfig) -> anyhow::Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        tokio::fs::create_dir_all(data_dir.join("wal")).await?;
        tokio::fs::create_dir_all(data_dir.join("segments")).await?;

        // 1) Load existing segments (none on first open).
        let mut segment_list: Vec<SegmentHandle> = Vec::new();
        let segs_dir = data_dir.join("segments");
        let mut rd = tokio::fs::read_dir(&segs_dir).await?;
        while let Some(entry) = rd.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("parquet") {
                match load_handle(&path, cfg.partition.granularity) {
                    Ok(h) => segment_list.push(h),
                    Err(e) => tracing::error!(%e, "skipping unreadable segment {}", path.display()),
                }
            }
        }
        segment_list.sort_by(|a, b| a.path.cmp(&b.path));
        let next_seq_start: u64 = segment_list
            .iter()
            .filter_map(|h| {
                h.path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<u64>().ok())
            })
            .max()
            .unwrap_or(0)
            + 1;

        // 2) Replay the WAL (if any) into the memtable.
        let initial_seq: u64 = 1;
        let wal_path = data_dir.join("wal").join(format!("{initial_seq:010}.log"));
        let current_pointer = data_dir.join("wal").join("current");
        tokio::fs::write(&current_pointer, format!("{initial_seq:010}.log"))
            .await
            .ok();

        let mut replayed: Vec<Record> = Vec::new();
        if wal_path.exists() {
            let f = std::fs::File::open(&wal_path)?;
            let mut r = std::io::BufReader::new(f);
            loop {
                match decode_frame(&mut r)? {
                    FrameRead::Record(rec) => replayed.push(rec),
                    FrameRead::Eof => break,
                    FrameRead::Partial => {
                        tracing::warn!("stopping WAL replay at partial trailing frame");
                        break;
                    }
                }
            }
        }

        // 3) Open the WAL for append (creates if missing).
        let wal_cfg = cfg.wal.clone();
        let wal = Arc::new(ArcSwap::new(Arc::new(
            WalWriter::open(&wal_path, wal_cfg.clone()).await?,
        )));

        // 4) Populate the memtable with replayed records, bypassing triggers.
        let memtable = {
            let mut m = Memtable::with_config(cfg.memtable);
            for rec in replayed {
                m.push_replay(rec);
            }
            Arc::new(Mutex::new(m))
        };

        let segments = Arc::new(RwLock::new(segment_list));
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let flush_permit = Arc::new(Semaphore::new(1));
        let next_seq = Arc::new(AtomicU64::new(next_seq_start));

        let store = Self {
            data_dir,
            wal,
            memtable,
            segments,
            tx,
            flush_permit,
            next_seq,
            partition_cfg: cfg.partition,
            wal_cfg,
        };

        // 5) Post-replay: if WAL replay pushed the memtable past its size
        //    threshold, flush once synchronously before returning.
        if store.memtable.lock().await.should_flush() {
            store.flush_now().await?;
        }

        Ok(store)
    }

    pub async fn append(&self, record: Record) -> anyhow::Result<()> {
        self.wal
            .load()
            .append(record.clone())
            .await
            .context("wal append")?;
        let should_flush = {
            let mut mt = self.memtable.lock().await;
            mt.push(record.clone());
            mt.should_flush()
        };
        let _ = self.tx.send(record);

        if should_flush {
            self.flush_now().await?;
        }
        Ok(())
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Record> {
        self.tx.subscribe()
    }

    /// Search memtable + all segments for records whose `body` contains `query`.
    /// Empty query matches everything. Optionally restricts to `[from, to)`.
    /// Segments whose `[min_ts, max_ts]` don't intersect the range are skipped.
    /// Returns `(matches, scanned_total)` with matches sorted newest-first and
    /// truncated to `limit`.
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

        let mut scanned = mem_snapshot
            .iter()
            .filter(|r| in_range(r.timestamp))
            .count();

        for handle in &seg_snapshot {
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

    /// Acquire the single flush permit (blocks behind any in-flight flush —
    /// this is the backpressure signal), swap the memtable, group records by
    /// partition, and write one segment file per partition group.
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

        self.wal.load().fsync_through_current().await?;

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
            let filename = crate::segment::segment_filename(partition, seq, granularity);
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

        // Rotate the WAL: new file with the next seq number, swap Arc, write
        // pointer, close old writer, delete old file.
        let new_seq = self.next_seq.load(Ordering::SeqCst);
        let new_name = format!("{new_seq:010}.log");
        let new_path = self.data_dir.join("wal").join(&new_name);
        let new_writer = Arc::new(WalWriter::open(&new_path, self.wal_cfg.clone()).await?);
        let old_writer_arc = self.wal.swap(new_writer);

        let current = self.data_dir.join("wal").join("current");
        let tmp = self.data_dir.join("wal").join("current.tmp");
        tokio::fs::write(&tmp, &new_name).await?;
        tokio::fs::rename(&tmp, &current).await?;

        // Let any in-flight append on the old writer settle.
        tokio::task::yield_now().await;

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

        drop(permit);
        Ok(())
    }
}
