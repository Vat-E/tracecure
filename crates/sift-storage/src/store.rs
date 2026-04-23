//! Top-level `Store` orchestrator. Thin: pushes real work into wal/memtable/segment.
//!
//! SP1 scope: `open`, `append`, `subscribe`. `search` and the flush path come
//! in Tasks 8 & 9.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use sift_core::Record;
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::memtable::{Memtable, MemtableConfig};
use crate::segment::SegmentHandle;
use crate::wal::{WalConfig, WalWriter};

const BROADCAST_CAPACITY: usize = 1024;

/// Tunables. Production defaults match the design spec; tests can lower
/// thresholds to exercise the flush path quickly.
#[derive(Debug, Clone, Default)]
pub struct StoreConfig {
    pub wal: WalConfig,
    pub memtable: MemtableConfig,
}

pub struct Store {
    #[allow(dead_code)] // read by search/flush in later tasks
    data_dir: PathBuf,
    wal: Arc<WalWriter>,
    memtable: Arc<Mutex<Memtable>>,
    #[allow(dead_code)] // read by search in Task 8, written by flush in Task 9
    segments: Arc<RwLock<Vec<SegmentHandle>>>,
    tx: broadcast::Sender<Record>,
}

impl Store {
    pub async fn open(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        Self::open_with(data_dir, StoreConfig::default()).await
    }

    pub async fn open_with(
        data_dir: impl AsRef<Path>,
        cfg: StoreConfig,
    ) -> anyhow::Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        tokio::fs::create_dir_all(data_dir.join("wal")).await?;
        tokio::fs::create_dir_all(data_dir.join("segments")).await?;

        let wal_path = data_dir.join("wal").join("0000000001.log");
        let wal = Arc::new(WalWriter::open(&wal_path, cfg.wal).await?);
        let memtable = Arc::new(Mutex::new(Memtable::with_config(cfg.memtable)));
        let segments = Arc::new(RwLock::new(Vec::new()));
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);

        Ok(Self {
            data_dir,
            wal,
            memtable,
            segments,
            tx,
        })
    }

    pub async fn append(&self, record: Record) -> anyhow::Result<()> {
        self.wal.append(record.clone()).await.context("wal append")?;
        self.memtable.lock().await.push(record.clone());
        let _ = self.tx.send(record);
        Ok(())
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Record> {
        self.tx.subscribe()
    }

    /// Search memtable + all segments for records whose `body` contains `query`.
    /// Empty query matches everything. Returns `(matches, scanned_total)` with
    /// matches sorted newest-first and truncated to `limit`.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<(Vec<Record>, usize)> {
        // Snapshot memtable and segment set under short locks.
        let mem_snapshot: Vec<Record> = self.memtable.lock().await.records().to_vec();
        let seg_snapshot: Vec<SegmentHandle> = self.segments.read().await.clone();

        let mut matches: Vec<Record> = mem_snapshot
            .iter()
            .filter(|r| query.is_empty() || r.body.contains(query))
            .cloned()
            .collect();

        let mut scanned = mem_snapshot.len();

        for handle in &seg_snapshot {
            scanned += usize::try_from(handle.row_count).unwrap_or(usize::MAX);
            let hits = tokio::task::spawn_blocking({
                let handle = handle.clone();
                let query = query.to_string();
                move || crate::segment::scan_segment(&handle, &query)
            })
            .await
            .context("scan task join")??;
            matches.extend(hits);
        }

        // Newest first.
        matches.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        matches.truncate(limit);

        Ok((matches, scanned))
    }
}
