//! WAL: serde_json-bodied framed records with a 10ms/1MB group-commit ticker.
//!
//! Frame layout (little-endian):
//!
//!   ┌──────────┬─────────────────────┬──────────┐
//!   │ len: u32 │ `serde_json(record)`  │ crc: u32 │
//!   └──────────┴─────────────────────┴──────────┘
//!         4            len                4
//!
//! Why `serde_json` and not bincode: `Record` uses `#[serde(skip_serializing_if)]`
//! on empty label/field maps and on `Option` fields. Bincode is positional
//! (no field names on the wire) so skipped-on-write fields desync the
//! decoder. JSON is self-describing, which tolerates optional fields
//! round-tripping correctly. The JSON body is ~3-4× larger than bincode
//! would be, but the WAL is bounded by memtable size (256 MB default) so
//! this is not a pressure point.
//!
//! CRC32 is computed over the `serde_json` body only (not the len prefix).

use std::io::{self, Read};

use anyhow::Context;
use tracecure_core::Record;

const FRAME_LEN_PREFIX: usize = 4;
const FRAME_CRC_SUFFIX: usize = 4;

pub fn encode_frame(record: &Record) -> anyhow::Result<Vec<u8>> {
    let body = serde_json::to_vec(record).context("serde_json serialize")?;
    let len: u32 = body.len().try_into().context("record too large (>4 GiB)")?;
    let crc = crc32fast::hash(&body);
    let mut out = Vec::with_capacity(FRAME_LEN_PREFIX + body.len() + FRAME_CRC_SUFFIX);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc.to_le_bytes());
    Ok(out)
}

/// Outcome of trying to read one frame from `r`.
#[derive(Debug)]
pub enum FrameRead {
    Record(Record),
    /// Clean EOF: no bytes left at all. Normal at end of WAL.
    Eof,
    /// Partial or corrupt frame at the tail. Callers should stop replay here,
    /// log a warning, and NOT treat it as a fatal error.
    Partial,
}

pub fn decode_frame<R: Read>(r: &mut R) -> anyhow::Result<FrameRead> {
    let mut len_buf = [0u8; FRAME_LEN_PREFIX];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(FrameRead::Eof),
        Err(e) => return Err(e).context("read len"),
    }
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut body = vec![0u8; len];
    match r.read_exact(&mut body) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(FrameRead::Partial),
        Err(e) => return Err(e).context("read body"),
    }

    let mut crc_buf = [0u8; FRAME_CRC_SUFFIX];
    match r.read_exact(&mut crc_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(FrameRead::Partial),
        Err(e) => return Err(e).context("read crc"),
    }
    let expected = u32::from_le_bytes(crc_buf);
    let actual = crc32fast::hash(&body);
    if expected != actual {
        return Ok(FrameRead::Partial);
    }

    let record: Record = serde_json::from_slice(&body).context("serde_json deserialize")?;
    Ok(FrameRead::Record(record))
}

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{watch, Mutex, Notify};
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct WalConfig {
    /// Maximum time bytes sit in the buffer before a forced fsync.
    pub fsync_interval: Duration,
    /// Buffer size that forces an immediate fsync (whichever hits first wins).
    pub size_trigger_bytes: usize,
}

impl Default for WalConfig {
    fn default() -> Self {
        Self {
            fsync_interval: Duration::from_millis(10),
            size_trigger_bytes: 1024 * 1024,
        }
    }
}

struct WalInner {
    /// Pending frame bytes waiting to be written + fsynced.
    buffer: Vec<u8>,
    /// Total bytes that have been handed to the OS for write (may or may not
    /// be fsynced yet).
    enqueued_bytes: u64,
    /// Bytes the ticker has fsynced through. Monotonic; only the ticker
    /// increments it.
    fsynced_bytes: u64,
}

pub struct WalWriter {
    path: PathBuf,
    inner: Arc<Mutex<WalInner>>,
    notify: Arc<Notify>,
    fsynced_tx: watch::Sender<u64>,
    ticker: Option<JoinHandle<()>>,
    shutdown: Arc<Notify>,
}

impl WalWriter {
    pub async fn open(path: impl AsRef<Path>, cfg: WalConfig) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .context("open WAL for append")?;

        let inner = Arc::new(Mutex::new(WalInner {
            buffer: Vec::with_capacity(cfg.size_trigger_bytes),
            enqueued_bytes: 0,
            fsynced_bytes: 0,
        }));
        let notify = Arc::new(Notify::new());
        let shutdown = Arc::new(Notify::new());
        let (fsynced_tx, _rx) = watch::channel(0u64);

        let ticker = tokio::spawn(run_ticker(
            file,
            Arc::clone(&inner),
            Arc::clone(&notify),
            Arc::clone(&shutdown),
            fsynced_tx.clone(),
            cfg,
        ));

        Ok(Self {
            path,
            inner,
            notify,
            fsynced_tx,
            ticker: Some(ticker),
            shutdown,
        })
    }

    /// Append a record to the in-memory buffer. Returns once bytes are in the
    /// buffer (durability is the ticker's job).
    pub async fn append(&self, record: Record) -> anyhow::Result<()> {
        let frame = encode_frame(&record)?;
        let mut guard = self.inner.lock().await;
        guard.buffer.extend_from_slice(&frame);
        guard.enqueued_bytes += frame.len() as u64;
        drop(guard);
        self.notify.notify_one();
        Ok(())
    }

    /// Wait until every record handed to `append` so far has been fsynced.
    pub async fn fsync_through_current(&self) -> anyhow::Result<()> {
        let target = self.inner.lock().await.enqueued_bytes;
        self.notify.notify_one();
        let mut rx = self.fsynced_tx.subscribe();
        while *rx.borrow() < target {
            rx.changed().await.context("ticker stopped")?;
        }
        Ok(())
    }

    /// Signal the ticker to shut down. Does not await completion; the ticker
    /// will drain on its next iteration and the join handle will complete once
    /// the Arc count drops to zero. Use `close` when you own `self` and want
    /// to await the drain; use this when you only have a shared reference.
    pub fn request_shutdown(&self) {
        self.shutdown.notify_one();
    }

    /// Shut down the ticker cleanly, flushing any pending bytes first.
    pub async fn close(mut self) -> anyhow::Result<()> {
        self.shutdown.notify_one();
        if let Some(handle) = self.ticker.take() {
            handle.await.context("ticker task panicked")?;
        }
        Ok(())
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

async fn run_ticker(
    mut file: File,
    inner: Arc<Mutex<WalInner>>,
    notify: Arc<Notify>,
    shutdown: Arc<Notify>,
    fsynced_tx: watch::Sender<u64>,
    cfg: WalConfig,
) {
    let mut tick = tokio::time::interval(cfg.fsync_interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    tick.tick().await; // drop the immediate first tick

    loop {
        tokio::select! {
            () = shutdown.notified() => {
                let _ = flush_once(&mut file, &inner, &fsynced_tx).await;
                return;
            }
            _ = tick.tick() => {
                if let Err(e) = flush_once(&mut file, &inner, &fsynced_tx).await {
                    tracing::error!(%e, "wal ticker flush failed");
                }
            }
            () = notify.notified() => {
                let over = inner.lock().await.buffer.len() >= cfg.size_trigger_bytes;
                if over {
                    if let Err(e) = flush_once(&mut file, &inner, &fsynced_tx).await {
                        tracing::error!(%e, "wal size-triggered flush failed");
                    }
                }
            }
        }
    }
}

async fn flush_once(
    file: &mut File,
    inner: &Arc<Mutex<WalInner>>,
    fsynced_tx: &watch::Sender<u64>,
) -> anyhow::Result<()> {
    let bytes = {
        let mut guard = inner.lock().await;
        if guard.buffer.is_empty() {
            return Ok(());
        }
        std::mem::take(&mut guard.buffer)
    };
    file.write_all(&bytes).await.context("WAL write_all")?;
    file.sync_data().await.context("WAL sync_data")?;
    let mut guard = inner.lock().await;
    guard.fsynced_bytes += bytes.len() as u64;
    let _ = fsynced_tx.send(guard.fsynced_bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use tracecure_core::{Level, Record};
    use std::collections::BTreeMap;
    use std::io::Cursor;

    fn sample() -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 23, 12, 0, 0).unwrap(),
            host: "box-1".into(),
            app: "api".into(),
            stream_id: "api".into(),
            level: Level::Warn,
            body: "roundtrip me".into(),
            labels: BTreeMap::from([("env".to_string(), "prod".to_string())]),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    #[test]
    fn encode_decode_roundtrips_single_frame() {
        let r = sample();
        let bytes = encode_frame(&r).unwrap();
        let mut cur = Cursor::new(&bytes);
        match decode_frame(&mut cur).unwrap() {
            FrameRead::Record(got) => assert_eq!(got, r),
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn decode_on_empty_input_is_eof() {
        let mut cur = Cursor::new(Vec::<u8>::new());
        assert!(matches!(decode_frame(&mut cur).unwrap(), FrameRead::Eof));
    }

    #[test]
    fn decode_on_truncated_body_is_partial() {
        let bytes = encode_frame(&sample()).unwrap();
        // Keep len prefix + half the body.
        let cutoff = FRAME_LEN_PREFIX + (bytes.len() - FRAME_LEN_PREFIX - FRAME_CRC_SUFFIX) / 2;
        let mut cur = Cursor::new(&bytes[..cutoff]);
        assert!(matches!(
            decode_frame(&mut cur).unwrap(),
            FrameRead::Partial
        ));
    }

    #[test]
    fn decode_on_bad_crc_is_partial() {
        let mut bytes = encode_frame(&sample()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        let mut cur = Cursor::new(&bytes);
        assert!(matches!(
            decode_frame(&mut cur).unwrap(),
            FrameRead::Partial
        ));
    }

    #[test]
    fn multiple_frames_decode_in_order() {
        let mut buf = Vec::new();
        for i in 0..3 {
            let mut r = sample();
            r.body = format!("msg {i}");
            buf.extend(encode_frame(&r).unwrap());
        }
        let mut cur = Cursor::new(buf);
        for i in 0..3 {
            match decode_frame(&mut cur).unwrap() {
                FrameRead::Record(r) => assert_eq!(r.body, format!("msg {i}")),
                other => panic!("expected Record, got {other:?}"),
            }
        }
        assert!(matches!(decode_frame(&mut cur).unwrap(), FrameRead::Eof));
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn writer_fsyncs_on_ticker() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.log");
        let cfg = WalConfig {
            fsync_interval: Duration::from_millis(10),
            size_trigger_bytes: 1_000_000,
        };
        let writer = WalWriter::open(&path, cfg).await.unwrap();

        writer.append(sample()).await.unwrap();
        writer.append(sample()).await.unwrap();

        tokio::time::advance(Duration::from_millis(15)).await;
        tokio::task::yield_now().await;

        writer.close().await.unwrap();

        let mut f = std::fs::File::open(&path).unwrap();
        let mut decoded = 0;
        loop {
            match decode_frame(&mut f).unwrap() {
                FrameRead::Record(_) => decoded += 1,
                FrameRead::Eof => break,
                FrameRead::Partial => panic!("partial frame in clean write"),
            }
        }
        assert_eq!(decoded, 2);
    }

    #[tokio::test]
    async fn writer_fsyncs_on_size_trigger() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("b.log");
        let cfg = WalConfig {
            fsync_interval: Duration::from_secs(60),
            size_trigger_bytes: 200,
        };
        let writer = WalWriter::open(&path, cfg).await.unwrap();

        for _ in 0..10 {
            writer.append(sample()).await.unwrap();
        }
        writer.close().await.unwrap();

        let mut f = std::fs::File::open(&path).unwrap();
        let mut decoded = 0;
        while let FrameRead::Record(_) = decode_frame(&mut f).unwrap() {
            decoded += 1;
        }
        assert_eq!(decoded, 10);
    }
}
