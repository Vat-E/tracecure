use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use sift_core::Record;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, Mutex};

const BROADCAST_CAPACITY: usize = 1024;
const SEARCH_MAX_LIMIT: usize = 10_000;

/// File-backed record store for M0. Appends NDJSON to a single file and
/// publishes new records on a broadcast channel for live-tail subscribers.
///
/// This is a throwaway backend; M1 replaces it with a WAL + Arrow segments.
#[derive(Clone)]
pub struct Store {
    inner: Arc<StoreInner>,
}

struct StoreInner {
    path: PathBuf,
    writer: Mutex<File>,
    tx: broadcast::Sender<Record>,
}

impl Store {
    pub async fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("opening {}", path.display()))?;
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Ok(Self {
            inner: Arc::new(StoreInner { path, writer: Mutex::new(file), tx }),
        })
    }

    /// Append a single record as one NDJSON line and broadcast it.
    /// Returns an error if the record cannot be serialized or written.
    ///
    /// Durability note: `flush` pushes tokio's buffer to the OS kernel; it does
    /// NOT `fsync`. A crash between write and disk-sync can lose recent
    /// records. This is intentional for M0 — the M1 WAL adds group-commit
    /// fsync for real durability.
    pub async fn append(&self, record: Record) -> anyhow::Result<()> {
        let mut line = serde_json::to_vec(&record).context("serializing record")?;
        line.push(b'\n');
        let mut guard = self.inner.writer.lock().await;
        guard.write_all(&line).await.context("writing record")?;
        guard.flush().await.context("flushing record")?;
        drop(guard);
        // Best-effort broadcast. `Err(SendError)` means no receivers exist —
        // harmless. Slow receivers that fall more than BROADCAST_CAPACITY
        // behind surface RecvError::Lagged on their end; nothing reports it
        // here at the sender side.
        let _ = self.inner.tx.send(record);
        Ok(())
    }

    /// Scan the file and collect up to `limit` records whose `body` contains
    /// `query` as a case-sensitive substring (empty query matches everything).
    /// Returns `(matches, scanned_total)`; `matches` is ordered oldest-first
    /// and truncated to the most recent `limit` records. `scanned_total`
    /// counts records-shaped lines (including corrupt ones) but not blanks.
    pub async fn search(&self, query: &str, limit: usize) -> anyhow::Result<(Vec<Record>, usize)> {
        let limit = limit.min(SEARCH_MAX_LIMIT);
        let file = match tokio::fs::File::open(&self.inner.path).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Vec::new(), 0));
            }
            Err(e) => return Err(e.into()),
        };
        let mut reader = BufReader::new(file).lines();
        let mut matches: Vec<Record> = Vec::new();
        let mut scanned = 0usize;
        while let Some(line) = reader.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            scanned += 1;
            let rec: Record = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if query.is_empty() || rec.body.contains(query) {
                matches.push(rec);
            }
        }
        // Return the most recent `limit` records.
        if matches.len() > limit {
            let drop = matches.len() - limit;
            matches.drain(..drop);
        }
        Ok((matches, scanned))
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Record> {
        self.inner.tx.subscribe()
    }
}
