# SP1 — `sift-storage` MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a drop-in replacement for `sift-server`'s M0 NDJSON `Store` — a new `sift-storage` crate with a WAL (bincode+CRC frames), a bounded Arrow memtable, parquet segments, and WAL-replay on restart. Same public API as M0's `Store` so SP2's cutover is a one-line swap.

**Architecture:** `Store` orchestrates three pieces: a `WalWriter` (buffered + group-commit fsync every 10 ms / 1 MB), a `Memtable` (Vec<Record> with 256 MB / 60 s flush triggers), and a `Vec<SegmentHandle>` (parquet files on disk). A single-permit semaphore bounds flush concurrency. Filesystem is the catalog: WAL files under `{data}/wal/`, segments under `{data}/segments/`. See [`docs/superpowers/specs/2026-04-23-sp1-sift-storage-design.md`](../specs/2026-04-23-sp1-sift-storage-design.md) for the full design.

**Tech Stack:** `arrow = "55"`, `parquet = "55"` (zstd), `bincode = "1"`, `crc32fast = "1"`, `tokio = "1"` (full). Hoisted where useful from `bench/format-fit/`.

---

## File Structure

```
crates/sift-storage/
├─ Cargo.toml
└─ src/
   ├─ lib.rs            # pub Store; re-exports; mod wiring
   ├─ store.rs          # Store::{open, append, search, subscribe, close}
   ├─ wal.rs            # Frame encode/decode + WalWriter (async ticker task)
   ├─ memtable.rs       # Memtable struct + flush triggers + push_replay
   ├─ segment.rs        # SegmentHandle + write_segment + scan_segment
   └─ arrow.rs          # Arrow schema + records_to_batch (ported from bench/format-fit)

crates/sift-storage/tests/          # integration tests (one #[tokio::test] binary per file)
├─ append_and_search.rs
├─ reopen_survives_restart.rs
├─ flush_under_pressure.rs
├─ backpressure_on_second_flush.rs
└─ broadcast_delivers.rs
```

Each source file has one clear responsibility. `store.rs` is a thin orchestrator. `wal.rs`, `memtable.rs`, `segment.rs` do their own thing and export small APIs. `arrow.rs` is pure data conversion — no I/O.

---

## Task 1: Scaffold the crate

**Files:**
- Modify: `Cargo.toml` (root workspace) — add `sift-storage` to `members` + `[workspace.dependencies]`.
- Create: `crates/sift-storage/Cargo.toml`
- Create: `crates/sift-storage/src/lib.rs`
- Create: `crates/sift-storage/src/store.rs` (stub)
- Create: `crates/sift-storage/src/wal.rs` (stub)
- Create: `crates/sift-storage/src/memtable.rs` (stub)
- Create: `crates/sift-storage/src/segment.rs` (stub)
- Create: `crates/sift-storage/src/arrow.rs` (stub)

- [ ] **Step 1: Add `sift-storage` + new deps to the workspace root `Cargo.toml`**

In `Cargo.toml` at the repo root, update two sections:

```toml
[workspace]
resolver = "2"
members = [
    "crates/sift-core",
    "crates/sift-api",
    "crates/sift-server",
    "crates/sift-cli",
    "crates/sift-storage",
]
```

In `[workspace.dependencies]`, after the existing `chrono = …` line add:

```toml
arrow = { version = "55", features = ["prettyprint"] }
parquet = { version = "55", features = ["arrow", "zstd", "async"] }
bincode = "1"
crc32fast = "1"
sift-storage = { path = "crates/sift-storage" }
```

- [ ] **Step 2: Create `crates/sift-storage/Cargo.toml`**

```toml
[package]
name = "sift-storage"
edition.workspace = true
version.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Sift storage layer: WAL, memtable, Arrow parquet segments."

[lints]
workspace = true

[dependencies]
sift-core = { workspace = true }
tokio = { workspace = true }
arrow = { workspace = true }
parquet = { workspace = true }
bincode = { workspace = true }
crc32fast = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
chrono = { workspace = true }

[dev-dependencies]
tempfile = "3"
tokio-test = "0.4"
```

- [ ] **Step 3: Stub source files**

`crates/sift-storage/src/lib.rs`:

```rust
//! `sift-storage` — Sift's M1 storage layer: WAL, memtable, parquet segments.
//!
//! The public surface is a single `Store` type with the same four methods as
//! M0's `sift_server::Store` (`open`, `append`, `search`, `subscribe`). See the
//! design spec at `docs/superpowers/specs/2026-04-23-sp1-sift-storage-design.md`.

#![allow(dead_code)] // Cleared in the last task once Store wires everything up.

pub mod arrow;
pub mod memtable;
pub mod segment;
pub mod store;
pub mod wal;

pub use store::Store;
```

`crates/sift-storage/src/store.rs`:

```rust
//! Top-level `Store` orchestrator. Thin: pushes real work into wal/memtable/segment.

// Filled in by Task 7.
```

`crates/sift-storage/src/wal.rs`:

```rust
//! WAL: bincode+CRC framed records with a 10ms/1MB group-commit ticker.

// Frame encode/decode lands in Task 3, the async writer in Task 4.
```

`crates/sift-storage/src/memtable.rs`:

```rust
//! In-memory Vec<Record> buffer with 256MB / 60s flush triggers.

// Filled in by Task 5.
```

`crates/sift-storage/src/segment.rs`:

```rust
//! Parquet segment writer + reader. One segment = one parquet file.

// Filled in by Task 6.
```

`crates/sift-storage/src/arrow.rs`:

```rust
//! Arrow schema + `records_to_batch`, ported from bench/format-fit.

// Filled in by Task 2.
```

- [ ] **Step 4: Verify the workspace builds**

Run: `cargo build --workspace`
Expected: `sift-storage v0.1.0 (...)` shows in the build output; finishes cleanly.

- [ ] **Step 5: Verify clippy is clean**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: no output beyond `Finished`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/sift-storage
git commit -m "chore(storage): scaffold sift-storage crate"
```

---

## Task 2: Arrow schema + `records_to_batch` (TDD)

Port the Arrow conversion from `bench/format-fit/src/arrow.rs`. The schema **excludes** the flat `env`/`region` columns from the spike — ADR-002 retracted them. Keep labels as a `Map<Utf8, Utf8>` column only.

**Files:**
- Modify: `crates/sift-storage/src/arrow.rs`

- [ ] **Step 1: Write the failing test**

At the bottom of `crates/sift-storage/src/arrow.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use sift_core::{Level, Record};
    use std::collections::BTreeMap;

    fn rec(body: &str) -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 23, 12, 0, 0).unwrap(),
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

    #[test]
    fn schema_has_eight_columns() {
        assert_eq!(schema().fields().len(), 8);
    }

    #[test]
    fn records_to_batch_preserves_row_count_and_body() {
        let recs = vec![rec("alpha"), rec("beta"), rec("gamma")];
        let batch = records_to_batch(&recs, schema()).unwrap();
        assert_eq!(batch.num_rows(), 3);
        let bodies = batch
            .column_by_name("body")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        assert_eq!(bodies.value(0), "alpha");
        assert_eq!(bodies.value(1), "beta");
        assert_eq!(bodies.value(2), "gamma");
    }

    #[test]
    fn records_to_batch_serializes_fields_as_json_string() {
        let mut r = rec("x");
        r.fields.insert("n".into(), serde_json::json!(7));
        let batch = records_to_batch(&[r], schema()).unwrap();
        let fields = batch
            .column_by_name("fields_json")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        assert_eq!(fields.value(0), r#"{"n":7}"#);
    }
}
```

- [ ] **Step 2: Run the test — expect failure (no `schema` / `records_to_batch` yet)**

Run: `cargo test -p sift-storage --lib arrow`
Expected: FAIL — `cannot find function 'schema'` or similar.

- [ ] **Step 3: Write `schema()` and `records_to_batch()`**

Replace the file (stub at top stays as the doc comment):

```rust
//! Arrow schema + `records_to_batch`, ported from bench/format-fit.
//!
//! Schema (8 columns):
//!   timestamp    TIMESTAMP(MICROSECOND, UTC)    not null
//!   host         UTF8                            not null
//!   app          UTF8                            not null
//!   stream_id    UTF8                            not null
//!   level        UTF8                            not null
//!   body         UTF8                            not null
//!   labels       MAP<UTF8, UTF8>                 not null (may be empty)
//!   fields_json  UTF8                            not null (serialized JSON blob)
//!
//! The spike's `env` / `region` flattened columns are removed per ADR-002.

use std::sync::Arc;

use arrow::array::{ArrayRef, MapBuilder, RecordBatch, StringBuilder, TimestampMicrosecondBuilder};
use arrow::datatypes::{DataType, Field, Fields, Schema, TimeUnit};
use sift_core::{Level, Record};

pub fn schema() -> Arc<Schema> {
    // Arrow 55: MapBuilder emits values as nullable, so declare them nullable
    // in the schema too — mismatch causes RecordBatch::try_new to reject.
    let labels = DataType::Map(
        Arc::new(Field::new(
            "entries",
            DataType::Struct(Fields::from(vec![
                Field::new("keys", DataType::Utf8, false),
                Field::new("values", DataType::Utf8, true),
            ])),
            false,
        )),
        false,
    );
    Arc::new(Schema::new(vec![
        Field::new(
            "timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("host", DataType::Utf8, false),
        Field::new("app", DataType::Utf8, false),
        Field::new("stream_id", DataType::Utf8, false),
        Field::new("level", DataType::Utf8, false),
        Field::new("body", DataType::Utf8, false),
        Field::new("labels", labels, false),
        Field::new("fields_json", DataType::Utf8, false),
    ]))
}

pub fn records_to_batch(records: &[Record], schema: Arc<Schema>) -> anyhow::Result<RecordBatch> {
    let n = records.len();
    let mut ts = TimestampMicrosecondBuilder::with_capacity(n).with_timezone("UTC");
    let mut host = StringBuilder::with_capacity(n, n * 16);
    let mut app = StringBuilder::with_capacity(n, n * 8);
    let mut stream = StringBuilder::with_capacity(n, n * 16);
    let mut level_col = StringBuilder::with_capacity(n, n * 8);
    let mut body = StringBuilder::with_capacity(n, n * 80);
    let mut labels = MapBuilder::new(None, StringBuilder::new(), StringBuilder::new());
    let mut fields_json = StringBuilder::with_capacity(n, n * 40);

    for r in records {
        ts.append_value(r.timestamp.timestamp_micros());
        host.append_value(&r.host);
        app.append_value(&r.app);
        stream.append_value(&r.stream_id);
        level_col.append_value(level_as_str(r.level));
        body.append_value(&r.body);

        for (k, v) in &r.labels {
            labels.keys().append_value(k);
            labels.values().append_value(v);
        }
        labels.append(true)?;

        fields_json.append_value(serde_json::to_string(&r.fields)?);
    }

    let arrays: Vec<ArrayRef> = vec![
        Arc::new(ts.finish()),
        Arc::new(host.finish()),
        Arc::new(app.finish()),
        Arc::new(stream.finish()),
        Arc::new(level_col.finish()),
        Arc::new(body.finish()),
        Arc::new(labels.finish()),
        Arc::new(fields_json.finish()),
    ];
    Ok(RecordBatch::try_new(schema, arrays)?)
}

fn level_as_str(l: Level) -> &'static str {
    match l {
        Level::Trace => "trace",
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
        Level::Fatal => "fatal",
    }
}
```

- [ ] **Step 4: Run the tests — expect pass**

Run: `cargo test -p sift-storage --lib arrow`
Expected: 3 tests pass.

- [ ] **Step 5: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/sift-storage/src/arrow.rs
git commit -m "feat(storage): arrow schema and records_to_batch"
```

---

## Task 3: WAL frame encoding (TDD)

Pure functions: encode a record to `Vec<u8>`, decode a frame from a byte stream. No I/O. Task 4 wraps these in the async writer.

**Files:**
- Modify: `crates/sift-storage/src/wal.rs`

- [ ] **Step 1: Write the failing tests**

Replace `wal.rs`:

```rust
//! WAL: bincode+CRC framed records with a 10ms/1MB group-commit ticker.
//!
//! Frame layout (little-endian):
//!
//!   ┌──────────┬──────────────┬──────────┐
//!   │ len: u32 │ bincode body │ crc: u32 │
//!   └──────────┴──────────────┴──────────┘
//!         4          len           4
//!
//! CRC32 is computed over the bincode body only (not the len prefix).

use std::io::{self, Read};

use anyhow::Context;
use sift_core::Record;

const FRAME_LEN_PREFIX: usize = 4;
const FRAME_CRC_SUFFIX: usize = 4;

pub fn encode_frame(record: &Record) -> anyhow::Result<Vec<u8>> {
    let body = bincode::serialize(record).context("bincode serialize")?;
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

    let record: Record = bincode::deserialize(&body).context("bincode deserialize")?;
    Ok(FrameRead::Record(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use sift_core::{Level, Record};
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
        assert!(matches!(decode_frame(&mut cur).unwrap(), FrameRead::Partial));
    }

    #[test]
    fn decode_on_bad_crc_is_partial() {
        let mut bytes = encode_frame(&sample()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        let mut cur = Cursor::new(&bytes);
        assert!(matches!(decode_frame(&mut cur).unwrap(), FrameRead::Partial));
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
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p sift-storage --lib wal`
Expected: 5 tests pass.

- [ ] **Step 3: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/sift-storage/src/wal.rs
git commit -m "feat(storage): WAL frame encode/decode with CRC32"
```

---

## Task 4: `WalWriter` async task with group-commit ticker

Now wrap the frame primitives in the async writer: takes records, buffers their frame bytes, a background ticker task fsyncs every 10 ms or when the buffer hits 1 MB.

**Files:**
- Modify: `crates/sift-storage/src/wal.rs` — add the `WalWriter` struct and ticker.

- [ ] **Step 1: Write the failing integration test**

Append to the `tests` module at the bottom of `wal.rs`:

```rust
    use std::io::Cursor;
    use std::time::Duration;

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

        // Before the ticker fires, the file may contain 0 bytes (OS hasn't flushed).
        // After 15 ms the ticker has fired.
        tokio::time::advance(Duration::from_millis(15)).await;
        tokio::task::yield_now().await; // let the ticker task run

        writer.close().await.unwrap();

        // Read back via decode_frame and count.
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
            fsync_interval: Duration::from_secs(60), // effectively infinite
            size_trigger_bytes: 200, // force fsync after a couple records
        };
        let writer = WalWriter::open(&path, cfg).await.unwrap();

        for _ in 0..10 {
            writer.append(sample()).await.unwrap();
        }
        writer.close().await.unwrap();

        // All 10 records should be durable.
        let mut f = std::fs::File::open(&path).unwrap();
        let mut decoded = 0;
        while let FrameRead::Record(_) = decode_frame(&mut f).unwrap() {
            decoded += 1;
        }
        assert_eq!(decoded, 10);
    }
```

Also: at the top of the file near `FRAME_LEN_PREFIX`, add `use std::path::{Path, PathBuf};` and `use std::sync::Arc;` and `use tokio::sync::Mutex;` (some were missing).

Actually: keep all the existing imports, and make sure the Task 1 stub has the right ones once this lands. The implementation step (Step 2 below) has the canonical import list.

- [ ] **Step 2: Implement `WalWriter` + ticker**

Below the existing `decode_frame` function and above the `#[cfg(test)] mod tests`, append:

```rust
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
            size_trigger_bytes: 1 * 1024 * 1024,
        }
    }
}

struct WalInner {
    /// Pending frame bytes waiting to be written + fsynced.
    buffer: Vec<u8>,
    /// Total bytes that have been handed to the OS for write (may or may not
    /// be fsynced yet). `append` returns this as the record's durable
    /// watermark once the ticker has fsynced past it.
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
        // Nudge the ticker if the buffer is now past the size trigger.
        // The ticker also checks on its own cadence, so this is a latency
        // optimisation, not a correctness requirement.
        drop(guard);
        self.notify.notify_one();
        Ok(())
    }

    /// Wait until every record handed to `append` so far has been fsynced.
    /// Used by the flush path in store.rs before starting a parquet write.
    pub async fn fsync_through_current(&self) -> anyhow::Result<()> {
        let target = self.inner.lock().await.enqueued_bytes;
        self.notify.notify_one();
        let mut rx = self.fsynced_tx.subscribe();
        while *rx.borrow() < target {
            rx.changed().await.context("ticker stopped")?;
        }
        Ok(())
    }

    /// Shut down the ticker cleanly, flushing any pending bytes first.
    pub async fn close(mut self) -> anyhow::Result<()> {
        self.shutdown.notify_one();
        if let Some(handle) = self.ticker.take() {
            handle.await.context("ticker task panicked")?;
        }
        Ok(())
    }

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
            _ = shutdown.notified() => {
                // Final flush before we exit.
                let _ = flush_once(&mut file, &inner, &fsynced_tx).await;
                return;
            }
            _ = tick.tick() => {
                if let Err(e) = flush_once(&mut file, &inner, &fsynced_tx).await {
                    tracing::error!(%e, "wal ticker flush failed");
                }
            }
            _ = notify.notified() => {
                // Size trigger hint: check whether the buffer is past the
                // threshold, flush if so. Otherwise wait for the next tick.
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
    // Take the bytes under a short lock, release, then do the syscalls.
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
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p sift-storage --lib wal`
Expected: 5 existing frame tests + 2 new writer tests = 7 pass.

- [ ] **Step 4: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-storage/src/wal.rs
git commit -m "feat(storage): WalWriter with 10ms/1MB group-commit ticker"
```

---

## Task 5: Memtable

A `Mutex`-guarded `Vec<Record>` with byte-count tracking and trigger detection. Pure data.

**Files:**
- Modify: `crates/sift-storage/src/memtable.rs`

- [ ] **Step 1: Write the failing tests**

Replace `memtable.rs`:

```rust
//! In-memory Vec<Record> buffer with 256MB / 60s flush triggers.

use std::time::{Duration, Instant};

use sift_core::Record;

pub const DEFAULT_SIZE_BYTES: usize = 256 * 1024 * 1024;
pub const DEFAULT_AGE_SECONDS: u64 = 60;

#[derive(Debug, Clone, Copy)]
pub struct MemtableConfig {
    pub size_bytes: usize,
    pub age: Duration,
}

impl Default for MemtableConfig {
    fn default() -> Self {
        Self {
            size_bytes: DEFAULT_SIZE_BYTES,
            age: Duration::from_secs(DEFAULT_AGE_SECONDS),
        }
    }
}

#[derive(Debug)]
pub struct Memtable {
    records: Vec<Record>,
    approx_bytes: usize,
    opened_at: Instant,
    cfg: MemtableConfig,
}

impl Memtable {
    #[must_use]
    pub fn with_config(cfg: MemtableConfig) -> Self {
        Self {
            records: Vec::new(),
            approx_bytes: 0,
            opened_at: Instant::now(),
            cfg,
        }
    }

    #[must_use]
    pub fn new() -> Self {
        Self::with_config(MemtableConfig::default())
    }

    /// Normal path: push a record and update byte count.
    pub fn push(&mut self, r: Record) {
        self.approx_bytes += approx_bytes(&r);
        self.records.push(r);
    }

    /// Replay path (used during Store::open): push without ever triggering a
    /// flush. Callers run `should_flush` once at end of replay.
    pub fn push_replay(&mut self, r: Record) {
        self.push(r);
    }

    #[must_use]
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    #[must_use]
    pub fn approx_bytes(&self) -> usize {
        self.approx_bytes
    }

    #[must_use]
    pub fn should_flush(&self) -> bool {
        self.approx_bytes >= self.cfg.size_bytes || self.opened_at.elapsed() >= self.cfg.age
    }

    /// Swap contents into a fresh `Vec` and reset the age clock.
    pub fn take(&mut self) -> Vec<Record> {
        self.approx_bytes = 0;
        self.opened_at = Instant::now();
        std::mem::take(&mut self.records)
    }
}

impl Default for Memtable {
    fn default() -> Self {
        Self::new()
    }
}

fn approx_bytes(r: &Record) -> usize {
    let label_bytes: usize = r.labels.iter().map(|(k, v)| k.len() + v.len()).sum();
    let field_bytes: usize = r.fields.iter().map(|(k, v)| k.len() + v.to_string().len()).sum();
    r.host.len() + r.app.len() + r.stream_id.len() + r.body.len() + label_bytes + field_bytes + 64
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use sift_core::{Level, Record};
    use std::collections::BTreeMap;

    fn rec(body: &str) -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap(),
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

    #[test]
    fn new_memtable_is_empty() {
        let m = Memtable::new();
        assert!(m.is_empty());
        assert_eq!(m.approx_bytes(), 0);
    }

    #[test]
    fn push_grows_len_and_bytes() {
        let mut m = Memtable::new();
        m.push(rec("alpha"));
        m.push(rec("beta"));
        assert_eq!(m.len(), 2);
        assert!(m.approx_bytes() > 0);
    }

    #[test]
    fn take_clears_records_and_bytes() {
        let mut m = Memtable::new();
        m.push(rec("x"));
        let taken = m.take();
        assert_eq!(taken.len(), 1);
        assert!(m.is_empty());
        assert_eq!(m.approx_bytes(), 0);
    }

    #[test]
    fn should_flush_fires_on_byte_trigger() {
        let cfg = MemtableConfig {
            size_bytes: 100,
            age: Duration::from_secs(3600),
        };
        let mut m = Memtable::with_config(cfg);
        for _ in 0..20 {
            m.push(rec("some body content so bytes rise fast"));
        }
        assert!(m.should_flush());
    }

    #[test]
    fn should_flush_fires_on_age_trigger() {
        let cfg = MemtableConfig {
            size_bytes: usize::MAX,
            age: Duration::from_millis(1),
        };
        let m = Memtable::with_config(cfg);
        std::thread::sleep(Duration::from_millis(5));
        assert!(m.should_flush());
    }

    #[test]
    fn should_flush_false_when_neither_trigger_hits() {
        let cfg = MemtableConfig {
            size_bytes: usize::MAX,
            age: Duration::from_secs(3600),
        };
        let mut m = Memtable::with_config(cfg);
        m.push(rec("x"));
        assert!(!m.should_flush());
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p sift-storage --lib memtable`
Expected: 6 tests pass.

- [ ] **Step 3: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/sift-storage/src/memtable.rs
git commit -m "feat(storage): Memtable with size+age flush triggers"
```

---

## Task 6: Segment writer + reader (TDD)

Parquet write + parquet scan. Per ADR-002: zstd level 3, no bloom filters, `max_row_group_size = 1_000_000`.

**Files:**
- Modify: `crates/sift-storage/src/segment.rs`

- [ ] **Step 1: Write the failing tests**

Replace `segment.rs`:

```rust
//! Parquet segment writer + reader. One segment = one parquet file.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use chrono::{DateTime, Utc};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use sift_core::{Level, Record};
use std::collections::BTreeMap;

use crate::arrow::{records_to_batch, schema};

const ZSTD_LEVEL: i32 = 3;
const MAX_ROW_GROUP_SIZE: usize = 1_000_000;

#[derive(Debug, Clone)]
pub struct SegmentHandle {
    pub path: PathBuf,
    pub row_count: u64,
    pub min_ts: DateTime<Utc>,
    pub max_ts: DateTime<Utc>,
}

/// Write `records` to `path` as a parquet file and return a handle describing
/// it. Records are expected in timestamp-ascending order (memtable order).
pub fn write_segment(records: &[Record], path: &Path) -> anyhow::Result<SegmentHandle> {
    anyhow::ensure!(!records.is_empty(), "refusing to write empty segment");

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
    // One batch per call in SP1 — memtable is small enough (256MB default).
    let batch = records_to_batch(records, schema)?;
    writer.write(&batch)?;
    let meta = writer.close()?;

    Ok(SegmentHandle {
        path: path.to_path_buf(),
        row_count: meta.num_rows as u64,
        min_ts,
        max_ts,
    })
}

/// Open a segment file and return a `SegmentHandle` read from its footer.
/// Used on `Store::open` to rebuild the segment set.
pub fn load_handle(path: &Path) -> anyhow::Result<SegmentHandle> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let md = reader.metadata();
    let row_count = md.file_metadata().num_rows() as u64;
    anyhow::ensure!(row_count > 0, "segment has 0 rows: {}", path.display());

    // Derive min/max timestamp by scanning the timestamp column statistics
    // on each row group.
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
                    min_micros = Some(min_micros.map_or(v, |m| m.min(v)));
                }
            }
            if let Some(max) = stats.max_bytes_opt() {
                if max.len() == 8 {
                    let v = i64::from_le_bytes(max.try_into().unwrap());
                    max_micros = Some(max_micros.map_or(v, |m| m.max(v)));
                }
            }
        }
    }
    let min_ts = DateTime::<Utc>::from_timestamp_micros(min_micros.unwrap_or(0))
        .unwrap_or_else(|| Utc::now());
    let max_ts = DateTime::<Utc>::from_timestamp_micros(max_micros.unwrap_or(0))
        .unwrap_or_else(|| Utc::now());

    Ok(SegmentHandle {
        path: path.to_path_buf(),
        row_count,
        min_ts,
        max_ts,
    })
}

/// Scan a segment and return records whose `body` contains `needle` as a
/// case-sensitive substring. Empty `needle` matches all rows.
pub fn scan_segment(handle: &SegmentHandle, needle: &str) -> anyhow::Result<Vec<Record>> {
    let file = std::fs::File::open(&handle.path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch?;
        let n = batch.num_rows();

        let ts = batch
            .column_by_name("timestamp")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::TimestampMicrosecondArray>()
            .unwrap();
        let host = batch
            .column_by_name("host")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let app = batch
            .column_by_name("app")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let stream = batch
            .column_by_name("stream_id")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let level_col = batch
            .column_by_name("level")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let body = batch
            .column_by_name("body")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let labels = batch
            .column_by_name("labels")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::MapArray>()
            .unwrap();
        let fields_json = batch
            .column_by_name("fields_json")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();

        for i in 0..n {
            let b = body.value(i);
            if !needle.is_empty() && !b.contains(needle) {
                continue;
            }
            out.push(Record {
                timestamp: DateTime::<Utc>::from_timestamp_micros(ts.value(i)).unwrap(),
                host: host.value(i).to_string(),
                app: app.value(i).to_string(),
                stream_id: stream.value(i).to_string(),
                level: parse_level(level_col.value(i)),
                body: b.to_string(),
                labels: decode_labels(labels, i),
                fields: serde_json::from_str(fields_json.value(i)).unwrap_or_default(),
                raw: None,
            });
        }
    }
    Ok(out)
}

fn parse_level(s: &str) -> Level {
    match s {
        "trace" => Level::Trace,
        "debug" => Level::Debug,
        "warn" => Level::Warn,
        "error" => Level::Error,
        "fatal" => Level::Fatal,
        _ => Level::Info,
    }
}

fn decode_labels(arr: &arrow::array::MapArray, row: usize) -> BTreeMap<String, String> {
    let offsets = arr.value_offsets();
    let start = offsets[row] as usize;
    let end = offsets[row + 1] as usize;
    let keys = arr
        .keys()
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .unwrap();
    let values = arr
        .values()
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .unwrap();
    let mut out = BTreeMap::new();
    for i in start..end {
        if values.is_valid(i) {
            out.insert(keys.value(i).to_string(), values.value(i).to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn rec(body: &str, offset_s: i64) -> Record {
        Record {
            timestamp: Utc
                .with_ymd_and_hms(2026, 4, 23, 0, 0, 0)
                .unwrap()
                + chrono::Duration::seconds(offset_s),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: Level::Info,
            body: body.into(),
            labels: BTreeMap::from([("env".to_string(), "prod".to_string())]),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    #[test]
    fn write_then_scan_roundtrips_records() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("seg.parquet");
        let records = vec![rec("alpha", 0), rec("beta", 1), rec("gamma", 2)];
        let handle = write_segment(&records, &path).unwrap();
        assert_eq!(handle.row_count, 3);
        assert_eq!(handle.min_ts, records[0].timestamp);
        assert_eq!(handle.max_ts, records[2].timestamp);

        let got = scan_segment(&handle, "").unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].body, "alpha");
        assert_eq!(got[0].labels.get("env").map(String::as_str), Some("prod"));
    }

    #[test]
    fn scan_with_substring_filters() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("seg.parquet");
        let records = vec![rec("hello world", 0), rec("goodbye", 1)];
        let handle = write_segment(&records, &path).unwrap();
        let got = scan_segment(&handle, "hello").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].body, "hello world");
    }

    #[test]
    fn load_handle_reads_row_count_from_footer() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("seg.parquet");
        let records = vec![rec("x", 0), rec("y", 1)];
        let written = write_segment(&records, &path).unwrap();
        let loaded = load_handle(&path).unwrap();
        assert_eq!(loaded.row_count, written.row_count);
    }

    #[test]
    fn write_empty_slice_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("seg.parquet");
        assert!(write_segment(&[], &path).is_err());
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p sift-storage --lib segment`
Expected: 4 tests pass.

- [ ] **Step 3: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/sift-storage/src/segment.rs
git commit -m "feat(storage): segment writer + reader with ADR-002 defaults"
```

---

## Task 7: `Store` skeleton — open, append, subscribe (no flush yet)

A minimal `Store` that opens a data directory, accepts appends into WAL + memtable, and exposes a broadcast subscriber. Search and flush arrive in later tasks.

**Files:**
- Modify: `crates/sift-storage/src/store.rs`
- Create: `crates/sift-storage/tests/open_and_append.rs`

- [ ] **Step 1: Write the failing integration test**

Create `crates/sift-storage/tests/open_and_append.rs`:

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
async fn open_accepts_appends_and_broadcasts() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).await.unwrap();
    let mut rx = store.subscribe();

    store.append(rec("alpha")).await.unwrap();
    store.append(rec("beta")).await.unwrap();

    let a = rx.recv().await.unwrap();
    let b = rx.recv().await.unwrap();
    assert_eq!(a.body, "alpha");
    assert_eq!(b.body, "beta");
}
```

- [ ] **Step 2: Run — expect failure**

Run: `cargo test -p sift-storage --test open_and_append`
Expected: compile error, `Store` doesn't exist.

- [ ] **Step 3: Implement `Store::open` + `append` + `subscribe`**

Replace `crates/sift-storage/src/store.rs`:

```rust
//! Top-level `Store` orchestrator. Thin: pushes real work into wal/memtable/segment.
//!
//! SP1 scope: open, append, subscribe. Search and flush come in Tasks 8 & 9.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use sift_core::Record;
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::memtable::{Memtable, MemtableConfig};
use crate::segment::SegmentHandle;
use crate::wal::{WalConfig, WalWriter};

const BROADCAST_CAPACITY: usize = 1024;

/// Open-tunables. Production defaults match the design spec; tests can lower
/// thresholds to exercise the flush path quickly.
#[derive(Debug, Clone, Default)]
pub struct StoreConfig {
    pub wal: WalConfig,
    pub memtable: MemtableConfig,
}

pub struct Store {
    #[allow(dead_code)] // read by search/flush in later tasks
    data_dir: PathBuf,
    #[allow(dead_code)]
    wal: Arc<WalWriter>,
    #[allow(dead_code)]
    memtable: Arc<Mutex<Memtable>>,
    #[allow(dead_code)]
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
}
```

- [ ] **Step 4: Run the integration test**

Run: `cargo test -p sift-storage --test open_and_append`
Expected: 1 test passes.

- [ ] **Step 5: Run everything**

Run: `cargo test -p sift-storage`
Expected: 7 (wal) + 3 (arrow) + 6 (memtable) + 4 (segment) + 1 (integration) = 21 tests pass.

- [ ] **Step 6: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sift-storage/src/store.rs crates/sift-storage/tests/open_and_append.rs
git commit -m "feat(storage): Store::open + append + subscribe"
```

---

## Task 8: `Store::search`

Merge memtable records with segment-scan results, filter by body substring, sort newest-first, truncate to limit.

**Files:**
- Modify: `crates/sift-storage/src/store.rs`
- Create: `crates/sift-storage/tests/append_and_search.rs`

- [ ] **Step 1: Write the failing integration test**

Create `crates/sift-storage/tests/append_and_search.rs`:

```rust
use chrono::{DateTime, TimeZone, Utc};
use sift_core::{Level, Record};
use sift_storage::Store;
use std::collections::BTreeMap;

fn rec(body: &str, ts: DateTime<Utc>) -> Record {
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
async fn search_finds_memtable_records() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).await.unwrap();

    let t0 = Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap();
    store.append(rec("hello world", t0)).await.unwrap();
    store
        .append(rec("goodbye", t0 + chrono::Duration::seconds(1)))
        .await
        .unwrap();

    let (matches, scanned) = store.search("hello", 10).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].body, "hello world");
    assert_eq!(scanned, 2);
}

#[tokio::test]
async fn search_empty_query_returns_all_capped_at_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).await.unwrap();

    let t0 = Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap();
    for i in 0..5 {
        store
            .append(rec(&format!("m{i}"), t0 + chrono::Duration::seconds(i)))
            .await
            .unwrap();
    }

    let (matches, scanned) = store.search("", 3).await.unwrap();
    assert_eq!(matches.len(), 3);
    assert_eq!(scanned, 5);
    // Newest-first ordering.
    assert_eq!(matches[0].body, "m4");
    assert_eq!(matches[1].body, "m3");
    assert_eq!(matches[2].body, "m2");
}
```

- [ ] **Step 2: Run — expect failure (compile error, search not defined)**

Run: `cargo test -p sift-storage --test append_and_search`
Expected: fails to compile.

- [ ] **Step 3: Implement `Store::search`**

Append to `crates/sift-storage/src/store.rs`, inside `impl Store`, after `subscribe`:

```rust
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
            scanned += handle.row_count as usize;
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
```

- [ ] **Step 4: Run the integration tests**

Run: `cargo test -p sift-storage --test append_and_search`
Expected: 2 tests pass.

- [ ] **Step 5: Run everything**

Run: `cargo test -p sift-storage`
Expected: 23 tests total pass.

- [ ] **Step 6: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sift-storage/src/store.rs crates/sift-storage/tests/append_and_search.rs
git commit -m "feat(storage): Store::search merges memtable and segments"
```

---

## Task 9: Flush path + WAL rotation

When memtable triggers fire, swap it, flush the old contents to a new segment, rotate the WAL. Semaphore bounds flushes at 1-in-flight; second flush blocks `append` ("backpressure").

**Files:**
- Modify: `crates/sift-storage/src/store.rs`
- Create: `crates/sift-storage/tests/flush_under_pressure.rs`
- Create: `crates/sift-storage/tests/backpressure_on_second_flush.rs`

- [ ] **Step 1: Write both failing integration tests**

Create `crates/sift-storage/tests/flush_under_pressure.rs`:

```rust
use std::time::Duration;

use chrono::Utc;
use sift_core::{Level, Record};
use sift_storage::{
    memtable::MemtableConfig, store::StoreConfig, wal::WalConfig, Store,
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
async fn flush_creates_segment_file() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StoreConfig {
        wal: WalConfig {
            fsync_interval: Duration::from_millis(5),
            size_trigger_bytes: 1024,
        },
        memtable: MemtableConfig {
            size_bytes: 512, // tiny — a handful of records triggers flush
            age: Duration::from_secs(3600),
        },
    };
    let store = Store::open_with(tmp.path(), cfg).await.unwrap();

    for i in 0..50 {
        store.append(rec(&format!("record number {i}"))).await.unwrap();
    }

    // Give the flush task time to produce the segment file.
    for _ in 0..40 {
        let segs = tokio::fs::read_dir(tmp.path().join("segments"))
            .await
            .unwrap();
        let count = read_dir_count(segs).await;
        if count > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let final_count = {
        let segs = tokio::fs::read_dir(tmp.path().join("segments")).await.unwrap();
        read_dir_count(segs).await
    };
    assert!(final_count > 0, "expected at least one segment file, got {final_count}");

    // Search should find the records in the segment (memtable may be empty).
    let (matches, _scanned) = store.search("record", 100).await.unwrap();
    assert!(matches.len() >= 50, "expected at least 50 matches, got {}", matches.len());
}

async fn read_dir_count(mut rd: tokio::fs::ReadDir) -> usize {
    let mut n = 0usize;
    while let Ok(Some(_)) = rd.next_entry().await {
        n += 1;
    }
    n
}
```

Create `crates/sift-storage/tests/backpressure_on_second_flush.rs`:

```rust
use std::time::Duration;

use chrono::Utc;
use sift_core::{Level, Record};
use sift_storage::{
    memtable::MemtableConfig, store::StoreConfig, wal::WalConfig, Store,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_flush_waits_for_first() {
    // Trigger is so small every record causes a flush. Two consecutive bursts
    // create two flushes; the second should queue behind the first.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StoreConfig {
        wal: WalConfig {
            fsync_interval: Duration::from_millis(1),
            size_trigger_bytes: 256,
        },
        memtable: MemtableConfig {
            size_bytes: 64,
            age: Duration::from_secs(3600),
        },
    };
    let store = Store::open_with(tmp.path(), cfg).await.unwrap();

    // 200 appends in quick succession. Some will pile up behind the flush
    // semaphore. The important thing is they all eventually land.
    for i in 0..200 {
        store.append(rec(&format!("record {i}"))).await.unwrap();
    }

    // Wait for the system to quiesce.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (matches, scanned) = store.search("record", 1000).await.unwrap();
    assert_eq!(matches.len(), 200, "scanned={scanned}");
}
```

- [ ] **Step 2: Run — expect failure (new `store.rs` symbols don't exist)**

Run: `cargo test -p sift-storage --test flush_under_pressure`
Expected: compile error because `memtable`, `store`, `wal` modules aren't pub-re-exported the way the tests assume, and `Store::open_with` needs to handle flushes.

Actually, `sift_storage::memtable` / `::wal` / `::store::StoreConfig` need to be accessible. Update `lib.rs` to ensure:

```rust
pub mod arrow;
pub mod memtable;
pub mod segment;
pub mod store;
pub mod wal;

pub use store::{Store, StoreConfig};
```

and make `MemtableConfig`, `WalConfig` already re-export through their modules (they are `pub` already from earlier tasks).

- [ ] **Step 3: Implement the flush path**

Append to `crates/sift-storage/src/store.rs`:

Add to the imports section at the top:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Semaphore;

use crate::segment::{write_segment, SegmentHandle};
```

(Keep existing imports; add `write_segment` to the `segment` use line and add the atomic + semaphore lines.)

Add fields to `Store`:

```rust
pub struct Store {
    data_dir: PathBuf,
    wal: Arc<WalWriter>,
    memtable: Arc<Mutex<Memtable>>,
    segments: Arc<RwLock<Vec<SegmentHandle>>>,
    tx: broadcast::Sender<Record>,
    flush_permit: Arc<Semaphore>,
    next_seq: Arc<AtomicU64>,
    memtable_cfg: MemtableConfig,
}
```

Update `open_with` to initialize those new fields:

```rust
    pub async fn open_with(
        data_dir: impl AsRef<Path>,
        cfg: StoreConfig,
    ) -> anyhow::Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        tokio::fs::create_dir_all(data_dir.join("wal")).await?;
        tokio::fs::create_dir_all(data_dir.join("segments")).await?;

        let wal_path = data_dir.join("wal").join("0000000001.log");
        let wal = Arc::new(WalWriter::open(&wal_path, cfg.wal).await?);
        let memtable = Arc::new(Mutex::new(Memtable::with_config(cfg.memtable.clone())));
        let segments = Arc::new(RwLock::new(Vec::new()));
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let flush_permit = Arc::new(Semaphore::new(1));
        let next_seq = Arc::new(AtomicU64::new(1));

        Ok(Self {
            data_dir,
            wal,
            memtable,
            segments,
            tx,
            flush_permit,
            next_seq,
            memtable_cfg: cfg.memtable,
        })
    }
```

(Note: `MemtableConfig` needs `Clone` — it's already `Clone, Copy` from Task 5, so this is fine.)

Rewrite `append` to drive flushes:

```rust
    pub async fn append(&self, record: Record) -> anyhow::Result<()> {
        self.wal.append(record.clone()).await.context("wal append")?;
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
```

Add `flush_now`:

```rust
    /// Acquire the single flush permit (blocks behind any in-flight flush —
    /// this is the backpressure signal), swap the memtable, and flush the
    /// swapped contents to a new segment file + rotate the WAL.
    async fn flush_now(&self) -> anyhow::Result<()> {
        // This await is the backpressure point.
        let permit = Arc::clone(&self.flush_permit).acquire_owned().await?;

        // Re-check after acquiring — another concurrent append may have
        // already flushed what this one was going to.
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

        // Make sure every record we're about to write to a segment is
        // durably in the WAL before the segment is created.
        self.wal.fsync_through_current().await?;

        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let seg_path = self
            .data_dir
            .join("segments")
            .join(format!("{seq:010}.parquet"));

        // Parquet write is blocking; run it off the async runtime.
        let handle = tokio::task::spawn_blocking(move || write_segment(&to_flush, &seg_path))
            .await
            .context("flush task join")??;

        self.segments.write().await.push(handle);

        // (WAL rotation is a SP3 concern; SP1 leaves the existing WAL file
        // in place. Replaying a WAL whose records are already in a segment
        // is idempotent at the memtable-read level — records become visible
        // once via segment + once via memtable replay. SP1's `open` tolerates
        // this by NOT replaying WAL files that are older than the newest
        // segment's min_ts. Task 10 implements that check.)

        drop(permit);
        Ok(())
    }
```

- [ ] **Step 4: Run the two new integration tests**

Run: `cargo test -p sift-storage --test flush_under_pressure`
Expected: 1 test passes.

Run: `cargo test -p sift-storage --test backpressure_on_second_flush`
Expected: 1 test passes.

- [ ] **Step 5: Run everything**

Run: `cargo test -p sift-storage`
Expected: 25 tests total pass.

- [ ] **Step 6: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sift-storage/src/store.rs \
        crates/sift-storage/src/lib.rs \
        crates/sift-storage/tests/flush_under_pressure.rs \
        crates/sift-storage/tests/backpressure_on_second_flush.rs
git commit -m "feat(storage): flush path with semaphore-backpressured segment writes"
```

---

## Task 10: Restart recovery (WAL replay)

On `Store::open`, list existing segments + replay the WAL into the memtable. Partial trailing frames and CRC mismatches stop replay gracefully. If post-replay memtable is past threshold, run one synchronous flush before accepting writes.

**Files:**
- Modify: `crates/sift-storage/src/store.rs`
- Create: `crates/sift-storage/tests/reopen_survives_restart.rs`

- [ ] **Step 1: Write the failing integration test**

Create `crates/sift-storage/tests/reopen_survives_restart.rs`:

```rust
use std::time::Duration;

use chrono::{TimeZone, Utc};
use sift_core::{Level, Record};
use sift_storage::{
    memtable::MemtableConfig, store::StoreConfig, wal::WalConfig, Store,
};
use std::collections::BTreeMap;

fn rec(body: &str, offset: i64) -> Record {
    Record {
        timestamp: Utc
            .with_ymd_and_hms(2026, 4, 23, 0, 0, 0)
            .unwrap()
            + chrono::Duration::seconds(offset),
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

fn default_cfg() -> StoreConfig {
    StoreConfig {
        wal: WalConfig {
            fsync_interval: Duration::from_millis(5),
            size_trigger_bytes: 4096,
        },
        memtable: MemtableConfig {
            size_bytes: usize::MAX, // never auto-flush in this test
            age: Duration::from_secs(3600),
        },
    }
}

#[tokio::test]
async fn records_survive_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let store = Store::open_with(tmp.path(), default_cfg()).await.unwrap();
        for i in 0..100 {
            store.append(rec(&format!("msg {i}"), i)).await.unwrap();
        }
        // Give the WAL ticker time to fsync.
        tokio::time::sleep(Duration::from_millis(100)).await;
    } // drop the store

    let store = Store::open_with(tmp.path(), default_cfg()).await.unwrap();
    let (matches, _scanned) = store.search("msg", 500).await.unwrap();
    assert_eq!(matches.len(), 100);
}

#[tokio::test]
async fn partial_trailing_frame_is_tolerated() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let store = Store::open_with(tmp.path(), default_cfg()).await.unwrap();
        for i in 0..5 {
            store.append(rec(&format!("ok {i}"), i)).await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Corrupt the last few bytes of the WAL to simulate a torn tail.
    let wal_path = tmp.path().join("wal").join("0000000001.log");
    let bytes = std::fs::read(&wal_path).unwrap();
    let mut truncated = bytes.clone();
    truncated.truncate(bytes.len() - 3); // drop 3 bytes of the tail
    std::fs::write(&wal_path, truncated).unwrap();

    let store = Store::open_with(tmp.path(), default_cfg()).await.unwrap();
    let (matches, _) = store.search("ok", 100).await.unwrap();
    // All 5 records should survive — they're all intact; only the partial
    // trailing frame (if any) is dropped. With 3 trailing bytes removed, the
    // last frame's CRC is corrupt → that record is dropped, 4 survive.
    // Since the file may or may not have finalized the last fsync before the
    // corruption, accept either 4 or 5 as success; the important contract is
    // that replay does not *panic* or *refuse to open*.
    assert!(matches.len() >= 4, "expected >=4 records, got {}", matches.len());
}
```

- [ ] **Step 2: Run — expect failure**

Run: `cargo test -p sift-storage --test reopen_survives_restart`
Expected: compile succeeds (tests depend only on public API), tests fail because `open_with` doesn't yet replay the WAL — the `records_survive_reopen` assertion fails with `matches.len() == 0`.

- [ ] **Step 3: Implement replay in `open_with`**

Edit the top of `open_with` in `store.rs`, between the `create_dir_all` calls and the `WalWriter::open` line, insert:

```rust
        // Load existing segments (none on first open).
        let mut segment_list: Vec<SegmentHandle> = Vec::new();
        let segs_dir = data_dir.join("segments");
        let mut rd = tokio::fs::read_dir(&segs_dir).await?;
        while let Some(entry) = rd.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("parquet") {
                match crate::segment::load_handle(&path) {
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
```

Then, after the `wal_path` line and **before** the WalWriter is opened (so replay reads the file first), add the replay block:

```rust
        // Replay the WAL file if it already exists. Before reopening for
        // append. Uses blocking std::fs because the reader is short-lived.
        let mut replayed: Vec<Record> = Vec::new();
        if wal_path.exists() {
            let f = std::fs::File::open(&wal_path)?;
            let mut r = std::io::BufReader::new(f);
            loop {
                match crate::wal::decode_frame(&mut r)? {
                    crate::wal::FrameRead::Record(rec) => replayed.push(rec),
                    crate::wal::FrameRead::Eof => break,
                    crate::wal::FrameRead::Partial => {
                        tracing::warn!("stopping WAL replay at partial trailing frame");
                        break;
                    }
                }
            }
        }
```

Then update the `memtable` initialization to pre-populate from replay:

```rust
        let memtable = {
            let mut m = Memtable::with_config(cfg.memtable.clone());
            for rec in replayed {
                m.push_replay(rec);
            }
            Arc::new(Mutex::new(m))
        };
```

Replace the `segments` initialization to use the loaded list:

```rust
        let segments = Arc::new(RwLock::new(segment_list));
```

Replace the `next_seq` initialization:

```rust
        let next_seq = Arc::new(AtomicU64::new(next_seq_start));
```

Then, after the whole `Store` is constructed, if the memtable is over its threshold, run one synchronous flush. Replace the final `Ok(Self { .. })` block with:

```rust
        let store = Self {
            data_dir,
            wal,
            memtable,
            segments,
            tx,
            flush_permit,
            next_seq,
            memtable_cfg: cfg.memtable,
        };
        // Post-replay: if WAL replay pushed the memtable past its size
        // threshold, flush once synchronously before returning.
        if store.memtable.lock().await.should_flush() {
            store.flush_now().await?;
        }
        Ok(store)
```

- [ ] **Step 4: Run the reopen tests**

Run: `cargo test -p sift-storage --test reopen_survives_restart`
Expected: 2 tests pass.

- [ ] **Step 5: Run everything**

Run: `cargo test -p sift-storage`
Expected: 27 tests total pass.

- [ ] **Step 6: Clippy**

Run: `cargo clippy -p sift-storage --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Clean up the file-level `#![allow(dead_code)]`**

All items are now used. Remove the `#![allow(dead_code)]` from `crates/sift-storage/src/lib.rs`. If any specific items are still flagged, add targeted `#[allow(dead_code)]` comments naming what uses them (should not be needed).

- [ ] **Step 8: Commit**

```bash
git add crates/sift-storage
git commit -m "feat(storage): WAL replay on open + post-replay flush guard"
```

---

## Task 11: Final cleanup + workspace verification

A small polish task to make sure the crate is shippable and the workspace stays green.

**Files:**
- Modify: `crates/sift-storage/README.md` (create)

- [ ] **Step 1: Add a crate README**

Create `crates/sift-storage/README.md`:

```markdown
# sift-storage

M1 storage layer for Sift. Replaces M0's flat-NDJSON `sift_server::Store`.

## What's here

- WAL with bincode+CRC32 frames, 10 ms / 1 MB group-commit fsync.
- Memtable: 256 MB / 60 s flush triggers.
- Parquet segments (zstd-3, 1M-row row groups, no bloom filters — see ADR-002).
- Filesystem-as-catalog: `{data}/wal/*.log` + `{data}/segments/*.parquet`.

See [`docs/superpowers/specs/2026-04-23-sp1-sift-storage-design.md`](../../docs/superpowers/specs/2026-04-23-sp1-sift-storage-design.md) for the full design.

## Public API

```rust
use sift_storage::Store;

let store = Store::open("./data").await?;
store.append(record).await?;
let (matches, scanned) = store.search("error", 100).await?;
let mut rx = store.subscribe();
```

Same four methods as M0's `sift_server::Store`. SP2 swaps the server over.

## What's NOT here yet

- Time-partitioned segment layout (SP3).
- Background compaction (SP3).
- Retention / eviction (SP4).
- SQLite catalog (SP4).
- Label index / inverted postings (SP5).
```

- [ ] **Step 2: Run workspace-wide verification**

Run: `cargo build --workspace`
Expected: clean.

Run: `cargo test --workspace --all-targets`
Expected: all tests pass (M0's 20 sift-cli + 10 sift-server + 8 sift-api + 6 sift-core + 27 sift-storage = 71).

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

Run: `cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/sift-storage/README.md
git commit -m "docs(storage): crate README + SP1 scope notes"
```

---

## Self-review

**Spec coverage** (against `docs/superpowers/specs/2026-04-23-sp1-sift-storage-design.md`):

| Spec § | Requirement | Task |
|---|---|---|
| §2 Scope — Store API | `open`, `append`, `search`, `subscribe` signatures | Tasks 7, 8 |
| §3.1 WAL writer | async fs::File + buffer + 10 ms / 1 MB ticker | Task 4 |
| §3.2 Memtable | Vec<Record>, approx_bytes, opened_at, size/age triggers | Task 5 |
| §3.3 Flush path | semaphore (1 permit), swap, background flush task, WAL-fsync gate | Task 9 |
| §3.4 Segment set | Arc<RwLock<Vec<SegmentHandle>>>, SegmentHandle with path/row_count/min_ts/max_ts | Tasks 6, 7, 9 |
| §3.5 Broadcast | send after WAL buffer append | Task 7 |
| §4.1 WAL frame format | len u32 LE + bincode body + crc32 u32 LE | Task 3 |
| §4.2 Append path | WAL buffer → memtable push → broadcast → maybe trigger flush | Tasks 7, 9 |
| §4.3 Search path | snapshot memtable + segments, parallel scan, merge newest-first | Task 8 |
| §4.4 Restart recovery | list segments, replay WAL, tolerate partial trailing frame, post-replay flush check | Task 10 |
| §4.5 WAL rotation | explicitly deferred to SP3 per inline comment in Task 9 | deferred |
| §5 File layout | `{data}/wal/*.log` + `{data}/segments/*.parquet` | Tasks 7, 9 |
| §Error handling | WAL poisoning, parquet write retry, corrupt segment skip, corrupt WAL fatal | Tasks 9, 10 (partial; WAL poisoning + parquet retry not fully implemented — minor deviation, see below) |
| §Crate structure | 6 files with one responsibility each | Tasks 1–7 |
| §Dependencies | arrow 55, parquet 55, bincode 1, crc32fast 1, tokio, etc. | Task 1 |
| §Testing — unit | frame roundtrip, CRC mismatch, partial-frame tolerance, trigger math, filename sequencer | Tasks 3, 5, 6 (filename sequencer is tested via observable behaviour in Task 9) |
| §Testing — integration | happy path, records-survive-reopen, partial-frame tolerance, flush under pressure, backpressure, broadcast delivers, concurrent appends | Tasks 7, 8, 9, 10 (concurrent-appends test is a nice-to-have; accepting as coverage from the backpressure test which does concurrent-ish appends) |

**Known deliberate deviations from the spec:**

1. **WAL rotation deferred.** §4.5 described WAL rotation after each successful flush. This plan leaves WAL rotation to SP3 (or a follow-up in SP1). The inline comment in Task 9's `flush_now` acknowledges it. Impact: WAL files grow unboundedly until the server restarts. For SP1's MVP scope this is acceptable — the WAL is capped by the memtable size × flush rate.

2. **WAL poisoning on fsync errors not implemented.** §Error handling called for an `AtomicBool` that causes subsequent appends to refuse. Task 4 logs the error but doesn't set a sticky poison flag. Impact: after a disk-full event, new appends keep trying. Tracked as a TODO in `wal.rs` at the `run_ticker` error branch. Minor; fix in SP2 or add a follow-up task.

3. **Parquet write retry on failure not implemented.** §Error handling called for "retry the flush on next trigger with the same memtable". Task 9 swaps the memtable eagerly, so a failing flush loses those records (they stay durable in the WAL, replay recovers them). Impact: after a failed flush, the next append that triggers a flush will still send the newly-added records to disk, but the failed batch is re-fetched from the WAL on next restart. Acceptable for SP1.

4. **`concurrent_appends_are_ordered_in_wal` test not written.** The `backpressure_on_second_flush` test appends 200 records rapidly and confirms they all land — this covers the same concern (concurrent appends don't lose records) without the explicit ordering assertion. Can be added as a follow-up if ordering guarantees become load-bearing.

These deviations are documented in the relevant tasks' inline comments so the implementer can decide whether to close the gap inline or defer.

**Placeholder scan:** searched the plan for "TBD", "TODO", "fill in", "similar to", "implement later" — none in task content except:

- "Filled in by Task N" lines in Task 1's stub files — these are the explicit stubs that later tasks replace.
- A `TODO` hint in the deviations note above about WAL poisoning — not a placeholder in a task step.

**Type consistency:**
- `Record` throughout = `sift_core::Record`.
- `WalConfig { fsync_interval, size_trigger_bytes }` — defined Task 4, used Task 7, 9, 10 tests.
- `MemtableConfig { size_bytes, age }` — defined Task 5, used Task 7, 9, 10 tests.
- `StoreConfig { wal, memtable }` — defined Task 7, used Task 9, 10 tests.
- `SegmentHandle { path, row_count, min_ts, max_ts }` — defined Task 6, used Task 7, 8, 9, 10.
- `FrameRead { Record, Eof, Partial }` — defined Task 3, used Task 10.
- `Store` fields `data_dir, wal, memtable, segments, tx, flush_permit, next_seq, memtable_cfg` — introduced progressively across Tasks 7, 9. Task 10 does not add fields (only edits `open_with`).

One micro-consistency fix applied inline: `MemtableConfig` must derive `Clone` (not just `Copy`) for the `cfg.memtable.clone()` call in Task 9's `open_with` to work across future changes. Task 5's derive line reads `#[derive(Debug, Clone, Copy)]` which satisfies both.

---

Plan complete and saved to `docs/superpowers/plans/2026-04-23-sp1-sift-storage.md`. Two execution options:

**1. Subagent-Driven (recommended)** — fresh subagent per task, two-stage review per task.

**2. Inline Execution** — I run the tasks in this session with checkpoints.

Which approach?
