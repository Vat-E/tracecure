# M1 Format-Fit Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Before committing to `arrow-rs` + `parquet` as the M1 on-disk format, write ~10M realistic log records through it and measure whether the numbers support building compaction + retention on top of this format — or whether we should pivot (raw-column Arrow, a custom format, or a different library).

**Architecture:** Standalone benchmark crate at `bench/format-fit/`, excluded from the main workspace. Generates synthetic records with seeded RNG, batches them into Arrow `RecordBatch`es, writes a single Parquet file with zstd compression, then measures: write throughput, write amplification, footer size, read throughput, and **two** label-filter timings — once against a `Map<String,String>` column (the schema-flexible case) and once against a flattened top-level column (the best-case pushdown). A final report + ADR captures the numbers and the yes/no decision for the M1 format.

**Tech Stack:** `arrow = "53"`, `parquet = "53"` (with `zstd` + `async` features), `rand` + `rand_chacha` for seeded generation, `clap` for a tiny CLI. Reuses `sift-core::{Record, Level}` for synthesis.

---

## File Structure

```
bench/
└─ format-fit/
   ├─ Cargo.toml
   ├─ README.md
   └─ src/
      ├─ main.rs      # CLI + orchestration; prints the report
      ├─ gen.rs       # Synthetic-record generator + tests
      ├─ arrow.rs     # Arrow schema + parquet writer + parquet reader
      └─ query.rs     # Label-filter benchmarks (Map column AND flattened column)
```

Responsibilities:

- `gen.rs` — pure synthesis, seeded, deterministic. No I/O. Emits `Vec<Record>` or streams `Record`s. Unit-tested for determinism.
- `arrow.rs` — converts `[Record]` → `RecordBatch` against a fixed `Schema`, writes to parquet, reads back. Unit-tested for round-trip fidelity on a small batch.
- `query.rs` — runs two filter variants on a built parquet file and returns timings + match counts. Unit-tested against a tiny fixture.
- `main.rs` — CLI. Accepts `--records N`, `--output PATH`, `--seed S`. Orchestrates gen → write → read → query and prints a plain-text report.

Target size: ~600 lines total across the four source files. The file is a throwaway; **DRY only within one file**.

---

## Dependencies to add

In `bench/format-fit/Cargo.toml` (new crate, not in workspace):

```toml
[package]
name = "format-fit"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
sift-core = { path = "../../crates/sift-core" }

arrow = { version = "53", features = ["prettyprint"] }
parquet = { version = "53", features = ["arrow", "zstd", "async"] }

chrono = { version = "0.4", default-features = false, features = ["serde", "clock"] }
serde_json = "1"

rand = "0.8"
rand_chacha = "0.3"

clap = { version = "4", features = ["derive"] }
anyhow = "1"
thiserror = "1"

# Not in workspace — keep this decision self-contained.

[dev-dependencies]
tempfile = "3"
```

The `bench/format-fit` directory goes under the existing root `Cargo.toml` `exclude = [...]` list so it doesn't participate in workspace builds.

---

## Task 1: Scaffold the bench crate

**Files:**
- Modify: `Cargo.toml` — add `bench/format-fit` to `exclude`.
- Create: `bench/format-fit/Cargo.toml`
- Create: `bench/format-fit/README.md`
- Create: `bench/format-fit/src/main.rs` (stub)
- Create: `bench/format-fit/src/gen.rs` (stub)
- Create: `bench/format-fit/src/arrow.rs` (stub)
- Create: `bench/format-fit/src/query.rs` (stub)

- [ ] **Step 1: Add the exclude entry in the root `Cargo.toml`**

Open `Cargo.toml` at the repo root. The `exclude` line currently reads:

```toml
exclude = ["ui/sift-ui"]
```

Change it to:

```toml
exclude = ["ui/sift-ui", "bench/format-fit"]
```

- [ ] **Step 2: Write `bench/format-fit/Cargo.toml`**

```toml
[package]
name = "format-fit"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
sift-core = { path = "../../crates/sift-core" }

arrow = { version = "53", features = ["prettyprint"] }
parquet = { version = "53", features = ["arrow", "zstd", "async"] }

chrono = { version = "0.4", default-features = false, features = ["serde", "clock"] }
serde_json = "1"

rand = "0.8"
rand_chacha = "0.3"

clap = { version = "4", features = ["derive"] }
anyhow = "1"
thiserror = "1"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Write `bench/format-fit/README.md`**

```markdown
# format-fit

One-shot benchmark answering a single question:

> Should Sift's M1 storage layer build on `arrow-rs` + `parquet`, or should we pivot to a different format?

See the M1 plan in `docs/superpowers/plans/2026-04-23-m1-format-fit-spike.md` for the full protocol and acceptance thresholds.

## Run

```
cargo run --release --manifest-path bench/format-fit/Cargo.toml -- \
  --records 10_000_000 \
  --output ./target/format-fit/corpus.parquet \
  --seed 42
```

Adds roughly 2–5 GB to `./target/format-fit/`. Clean with `rm -rf ./target/format-fit/`.

## Output

Plain text report on stdout with throughput, file size, write amplification, footer size, and two label-filter timings (Map column vs flattened column).

## Not shipped

This crate is **excluded from the workspace** (`Cargo.toml` root, `exclude = [...]`). It is a one-shot research tool. Do not depend on it from any shipping crate.
```

- [ ] **Step 4: Write minimal stub source files**

`bench/format-fit/src/main.rs`:

```rust
//! `format-fit` — one-shot benchmark: is arrow-rs + parquet the right M1 format?
//!
//! See plan at docs/superpowers/plans/2026-04-23-m1-format-fit-spike.md.

mod arrow;
mod gen;
mod query;

fn main() -> anyhow::Result<()> {
    eprintln!("format-fit placeholder — wiring lands in Task 5");
    Ok(())
}
```

`bench/format-fit/src/gen.rs`:

```rust
//! Synthetic-record generator. Seeded, deterministic, schema-flexible.

// Filled in by Task 2.
```

`bench/format-fit/src/arrow.rs`:

```rust
//! Arrow schema + parquet round-trip.

// Filled in by Task 3.
```

`bench/format-fit/src/query.rs`:

```rust
//! Label-filter benchmarks: Map column vs flattened column.

// Filled in by Task 4.
```

- [ ] **Step 5: Verify it builds**

Run: `cargo build --manifest-path bench/format-fit/Cargo.toml`
Expected: compiles (lots of Arrow/Parquet deps downloaded; first build takes a while).

- [ ] **Step 6: Verify the workspace still builds without format-fit**

Run: `cargo build --workspace`
Expected: completes; format-fit is NOT compiled (excluded).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml bench/
git commit -m "chore(bench): scaffold format-fit spike crate"
```

---

## Task 2: Seeded synthetic-record generator (TDD)

**Files:**
- Modify: `bench/format-fit/src/gen.rs`

- [ ] **Step 1: Write `gen.rs` with a seeded generator and unit tests**

Replace the stub `gen.rs` with:

```rust
//! Synthetic-record generator. Seeded, deterministic, schema-flexible.
//!
//! Design goals:
//! - Reproducible: same seed → same Vec<Record>.
//! - Wide-cardinality like real logs: ~500 hosts, ~30 apps, ~80 stream_ids.
//! - Body length matches a log-like distribution (p50 ≈ 80 chars, p99 ≈ 400).
//! - Labels: 3–6 keys per record drawn from a pool of 20 (low-cardinality,
//!   the use case the indexed label column is meant for).
//! - Fields: 0–8 keys per record, mixed scalar types (string, i64, bool).
//!
//! We keep the generator entirely in-memory; the 10M run produces ~2 GB of
//! owned Record values at peak. That's deliberate — we want the write
//! benchmark to include serialization cost, not generation cost.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, TimeZone, Utc};
use rand::Rng;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;
use sift_core::{Level, Record};

const HOST_POOL: usize = 500;
const APP_POOL: &[&str] = &[
    "api", "worker", "scheduler", "ingest", "billing", "auth", "mailer",
    "cron", "ci", "sync", "etl", "webhook", "consumer", "producer", "gateway",
    "search", "index", "cache", "pubsub", "streamer", "notifier", "archiver",
    "metrics", "alerter", "flagd", "router", "dedup", "compactor", "gc", "util",
];
const STREAM_POOL: usize = 80;

const LABEL_KEYS: &[&str] = &[
    "env", "region", "zone", "cluster", "pod", "node", "service", "tier",
    "version", "deploy", "tenant", "project", "team", "component", "runtime",
    "os", "arch", "shard", "replica", "role",
];

const FIELD_KEYS: &[&str] = &[
    "request_id", "user_id", "session_id", "duration_ms", "status_code",
    "bytes_in", "bytes_out", "error_code", "retries", "cached", "flagged",
    "upstream", "downstream", "db_query", "cache_key", "trace_id",
];

const BODY_WORDS: &[&str] = &[
    "request", "processed", "timeout", "retry", "success", "failure",
    "cache", "hit", "miss", "slow", "fast", "queued", "dispatched",
    "acknowledged", "rejected", "validated", "scheduled", "completed",
    "error", "warning", "info", "debug", "aborted", "resumed",
];

/// One call returns `n` records with timestamps in a contiguous ~1h window.
pub fn generate(seed: u64, n: usize) -> Vec<Record> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // Precompute pools.
    let hosts: Vec<String> = (0..HOST_POOL).map(|i| format!("host-{i:04}")).collect();
    let streams: Vec<String> = (0..STREAM_POOL).map(|i| format!("stream-{i:03}")).collect();

    let start: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 4, 23, 12, 0, 0).unwrap();
    let mut out = Vec::with_capacity(n);

    for i in 0..n {
        // Timestamp marches forward in ~3.6 ms increments on average across
        // 10M records → ~10h total window. Realistic-ish for a busy service.
        let ts_offset_ms: i64 = (i as i64) * 3 + rng.gen_range(0..6);
        let timestamp = start + Duration::milliseconds(ts_offset_ms);

        let host = hosts.choose(&mut rng).unwrap().clone();
        let app = APP_POOL.choose(&mut rng).unwrap().to_string();
        let stream_id = streams.choose(&mut rng).unwrap().clone();
        let level = gen_level(&mut rng);
        let body = gen_body(&mut rng);
        let labels = gen_labels(&mut rng);
        let fields = gen_fields(&mut rng);

        out.push(Record {
            timestamp,
            host,
            app,
            stream_id,
            level,
            body,
            labels,
            fields,
            raw: None,
        });
    }
    out
}

fn gen_level(rng: &mut ChaCha8Rng) -> Level {
    // Log-like distribution: mostly info, some debug, a smattering of warn/error.
    match rng.gen_range(0..100u8) {
        0..=5 => Level::Debug,
        6..=85 => Level::Info,
        86..=95 => Level::Warn,
        96..=99 => Level::Error,
        _ => unreachable!(),
    }
}

fn gen_body(rng: &mut ChaCha8Rng) -> String {
    // Word count log-normal-ish. p50 ~ 10 words, p99 ~ 60.
    let n_words: usize = 4 + (rng.gen_range(0u32..100u32) as usize).pow(2) / 160;
    let n_words = n_words.min(80);
    let mut parts = Vec::with_capacity(n_words);
    for _ in 0..n_words {
        parts.push(*BODY_WORDS.choose(rng).unwrap());
    }
    parts.join(" ")
}

fn gen_labels(rng: &mut ChaCha8Rng) -> BTreeMap<String, String> {
    let n = rng.gen_range(3..=6);
    let mut chosen: Vec<&&str> = LABEL_KEYS.choose_multiple(rng, n).collect();
    chosen.sort();
    let mut out = BTreeMap::new();
    for k in chosen {
        let v = format!("v{}", rng.gen_range(0..12));
        out.insert((*k).to_string(), v);
    }
    out
}

fn gen_fields(rng: &mut ChaCha8Rng) -> BTreeMap<String, serde_json::Value> {
    let n = rng.gen_range(0..=8);
    let mut chosen: Vec<&&str> = FIELD_KEYS.choose_multiple(rng, n).collect();
    chosen.sort();
    let mut out = BTreeMap::new();
    for k in chosen {
        let v = match rng.gen_range(0..3) {
            0 => serde_json::json!(rng.gen_range(0i64..1_000_000)),
            1 => serde_json::json!(rng.gen_bool(0.5)),
            2 => serde_json::json!(format!("s{}", rng.gen_range(0..1000))),
            _ => unreachable!(),
        };
        out.insert((*k).to_string(), v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_is_deterministic_for_a_given_seed() {
        let a = generate(42, 100);
        let b = generate(42, 100);
        assert_eq!(a.len(), b.len());
        assert_eq!(a.first().map(|r| &r.body), b.first().map(|r| &r.body));
        assert_eq!(a.last().map(|r| &r.body), b.last().map(|r| &r.body));
    }

    #[test]
    fn generate_varies_with_seed() {
        let a = generate(1, 100);
        let b = generate(2, 100);
        // Near-zero probability of identical body across 100 records if seeds differ.
        assert!(a.first().map(|r| &r.body) != b.first().map(|r| &r.body));
    }

    #[test]
    fn generate_produces_the_requested_count() {
        assert_eq!(generate(7, 1000).len(), 1000);
    }

    #[test]
    fn records_have_plausible_shape() {
        let recs = generate(99, 500);
        for r in &recs {
            assert!(!r.host.is_empty());
            assert!(!r.app.is_empty());
            assert!(!r.stream_id.is_empty());
            assert!(!r.body.is_empty());
            assert!(r.labels.len() >= 3 && r.labels.len() <= 6);
            assert!(r.fields.len() <= 8);
        }
    }

    #[test]
    fn timestamps_are_monotonically_non_decreasing() {
        let recs = generate(1, 10_000);
        for w in recs.windows(2) {
            assert!(w[0].timestamp <= w[1].timestamp);
        }
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --manifest-path bench/format-fit/Cargo.toml`
Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add bench/format-fit/src/gen.rs
git commit -m "feat(bench): seeded synthetic record generator for format-fit"
```

---

## Task 3: Arrow schema + parquet round-trip (TDD)

**Files:**
- Modify: `bench/format-fit/src/arrow.rs`

- [ ] **Step 1: Write `arrow.rs` with round-trip test**

Replace the stub with:

```rust
//! Arrow schema for Record and parquet writer/reader.
//!
//! Schema (all nullable flags match the intent):
//!   timestamp    TIMESTAMP(MICROSECOND, UTC)   not null
//!   host         UTF8                           not null
//!   app          UTF8                           not null
//!   stream_id    UTF8                           not null
//!   level        UTF8                           not null
//!   body         UTF8                           not null
//!   labels       MAP<UTF8, UTF8>                not null (may be empty)
//!   fields_json  UTF8                           not null (serialized JSON)
//!   env          UTF8                           nullable (flattened label for pushdown test)
//!   region       UTF8                           nullable (flattened label for pushdown test)
//!
//! `labels` is the full schema-flexible map. `env` and `region` are flattened
//! top-level columns sourced from `labels["env"]` and `labels["region"]` —
//! they exist to measure parquet predicate-pushdown vs map-column scan in
//! the query benchmark. `fields_json` is a serialized blob for now; a real
//! implementation would likely use nested types, but JSON strings are a
//! lower-bound baseline.

use std::sync::Arc;

use anyhow::Context;
use arrow::array::{
    ArrayRef, BooleanBuilder, MapBuilder, RecordBatch, StringBuilder,
    TimestampMicrosecondBuilder,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use sift_core::{Level, Record};
use std::fs::File;
use std::path::Path;

const BATCH_ROWS: usize = 8192;
const ZSTD_LEVEL: i32 = 3; // Default-ish; the balance between speed and ratio.

pub fn schema() -> Arc<Schema> {
    let labels = DataType::Map(
        Arc::new(Field::new(
            "entries",
            DataType::Struct(
                vec![
                    Field::new("keys", DataType::Utf8, false),
                    Field::new("values", DataType::Utf8, false),
                ]
                .into(),
            ),
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
        Field::new("env", DataType::Utf8, true),
        Field::new("region", DataType::Utf8, true),
    ]))
}

/// Convert a slice of Records into a single RecordBatch.
pub fn records_to_batch(records: &[Record], schema: Arc<Schema>) -> anyhow::Result<RecordBatch> {
    let mut ts = TimestampMicrosecondBuilder::with_capacity(records.len()).with_timezone("UTC");
    let mut host = StringBuilder::with_capacity(records.len(), records.len() * 16);
    let mut app = StringBuilder::with_capacity(records.len(), records.len() * 8);
    let mut stream = StringBuilder::with_capacity(records.len(), records.len() * 16);
    let mut level = StringBuilder::with_capacity(records.len(), records.len() * 8);
    let mut body = StringBuilder::with_capacity(records.len(), records.len() * 80);
    let key_builder = StringBuilder::new();
    let value_builder = StringBuilder::new();
    let mut labels = MapBuilder::new(None, key_builder, value_builder);
    let mut fields_json = StringBuilder::with_capacity(records.len(), records.len() * 40);
    let mut env = StringBuilder::with_capacity(records.len(), records.len() * 4);
    let mut region = StringBuilder::with_capacity(records.len(), records.len() * 4);

    for r in records {
        ts.append_value(r.timestamp.timestamp_micros());
        host.append_value(&r.host);
        app.append_value(&r.app);
        stream.append_value(&r.stream_id);
        level.append_value(level_as_str(r.level));
        body.append_value(&r.body);

        for (k, v) in &r.labels {
            labels.keys().append_value(k);
            labels.values().append_value(v);
        }
        labels.append(true)?;

        fields_json.append_value(serde_json::to_string(&r.fields)?);

        match r.labels.get("env") {
            Some(v) => env.append_value(v),
            None => env.append_null(),
        }
        match r.labels.get("region") {
            Some(v) => region.append_value(v),
            None => region.append_null(),
        }
    }

    let arrays: Vec<ArrayRef> = vec![
        Arc::new(ts.finish()),
        Arc::new(host.finish()),
        Arc::new(app.finish()),
        Arc::new(stream.finish()),
        Arc::new(level.finish()),
        Arc::new(body.finish()),
        Arc::new(labels.finish()),
        Arc::new(fields_json.finish()),
        Arc::new(env.finish()),
        Arc::new(region.finish()),
    ];
    let _ = BooleanBuilder::new(); // silences unused-import warnings if any
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

/// Write all records to a single parquet file using zstd compression. Returns
/// total bytes written on disk and the number of row groups.
pub fn write_parquet(records: &[Record], path: &Path) -> anyhow::Result<WriteStats> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    let schema = schema();
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(
            parquet::basic::ZstdLevel::try_new(ZSTD_LEVEL)?,
        ))
        .build();
    let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(props))?;

    let mut row_groups = 0usize;
    for chunk in records.chunks(BATCH_ROWS) {
        let batch = records_to_batch(chunk, schema.clone())?;
        writer.write(&batch)?;
        row_groups += 1;
    }
    let file_meta = writer.close()?;
    let size = std::fs::metadata(path)?.len();
    Ok(WriteStats {
        bytes_on_disk: size,
        row_groups_written: row_groups,
        row_group_meta_rows: file_meta.num_rows,
    })
}

#[derive(Debug)]
pub struct WriteStats {
    pub bytes_on_disk: u64,
    pub row_groups_written: usize,
    pub row_group_meta_rows: i64,
}

/// Footer size in bytes, extracted by re-opening the file and inspecting the
/// parquet metadata trailer length.
pub fn footer_size(path: &Path) -> anyhow::Result<u64> {
    let file = File::open(path)?;
    let total = file.metadata()?.len();
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let meta_len = reader.metadata().file_metadata().created_by().map(|_| 0);
    // Parquet stores the footer length in the last 8 bytes (4 bytes LE length +
    // "PAR1" magic). Rather than re-reading those, use the parquet_metadata
    // length field if exposed — fall back to "reported total - first row
    // group's byte offset".
    let first_rg = reader.metadata().row_group(0).file_offset().unwrap_or(0);
    let data_end = reader
        .metadata()
        .row_groups()
        .iter()
        .map(|rg| rg.file_offset().unwrap_or(0) + rg.total_byte_size())
        .max()
        .unwrap_or(0);
    let footer = total.saturating_sub(data_end.max(first_rg) as u64);
    let _ = meta_len;
    Ok(footer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::generate;

    #[test]
    fn round_trip_small_batch_preserves_row_count_and_bodies() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tiny.parquet");
        let records = generate(1, 1024);

        let stats = write_parquet(&records, &path).unwrap();
        assert!(stats.bytes_on_disk > 0);

        let file = std::fs::File::open(&path).unwrap();
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap();
        let mut rows = 0usize;
        for batch in reader {
            rows += batch.unwrap().num_rows();
        }
        assert_eq!(rows, records.len());
    }

    #[test]
    fn schema_has_ten_columns() {
        assert_eq!(schema().fields().len(), 10);
    }

    #[test]
    fn footer_size_is_smaller_than_total() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("small.parquet");
        let records = generate(2, 2048);
        let stats = write_parquet(&records, &path).unwrap();
        let footer = footer_size(&path).unwrap();
        assert!(
            footer < stats.bytes_on_disk,
            "footer {footer} should be < file {}",
            stats.bytes_on_disk
        );
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --manifest-path bench/format-fit/Cargo.toml`
Expected: 5 (gen) + 3 (arrow) = 8 tests pass.

- [ ] **Step 3: Commit**

```bash
git add bench/format-fit/src/arrow.rs
git commit -m "feat(bench): arrow schema + parquet round-trip for format-fit"
```

---

## Task 4: Label-filter query benchmarks (TDD)

**Files:**
- Modify: `bench/format-fit/src/query.rs`

- [ ] **Step 1: Write `query.rs` with two filter variants**

```rust
//! Label-filter benchmarks. Compares two ways to find "env = prod" rows:
//!
//! A) **Map-column scan** — iterate the `labels` MAP column and check each
//!    record's entries. This is the schema-flexible path.
//! B) **Flattened-column predicate** — predicate pushdown on the `env` top-
//!    level column. This is what a real deployment would index if it knew
//!    `env` was high-value up front.
//!
//! The point of the comparison is to see how much perf we leave on the
//! table by keeping labels schema-flexible.

use std::fs::File;
use std::path::Path;
use std::time::{Duration, Instant};

use arrow::array::{Array, MapArray, StringArray};
use parquet::arrow::arrow_reader::{
    ArrowPredicate, ArrowReaderOptions, ParquetRecordBatchReaderBuilder, RowFilter,
};
use parquet::arrow::ProjectionMask;

pub struct QueryStats {
    pub scanned_rows: u64,
    pub matched_rows: u64,
    pub duration: Duration,
    pub bytes_read: u64,
}

/// Path A — scan every `labels` map entry looking for `(key, needle)`.
///
/// Projection is limited to the `labels` column so we measure just the cost
/// of reading and decoding that column, not the whole row.
pub fn label_filter_via_map(
    path: &Path,
    key: &str,
    needle: &str,
) -> anyhow::Result<QueryStats> {
    let file = File::open(path)?;
    let start = Instant::now();
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();
    let labels_idx = schema.index_of("labels")?;
    let mask = ProjectionMask::leaves(builder.parquet_schema(), [labels_idx]);
    let reader = builder.with_projection(mask).build()?;

    let mut scanned = 0u64;
    let mut matched = 0u64;
    for batch in reader {
        let batch = batch?;
        scanned += batch.num_rows() as u64;
        let col = batch
            .column(0)
            .as_any()
            .downcast_ref::<MapArray>()
            .expect("labels is a MapArray");
        for row in 0..col.len() {
            let entry_offsets = col.offsets();
            let start = entry_offsets[row] as usize;
            let end = entry_offsets[row + 1] as usize;
            let keys = col
                .keys()
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("keys utf8");
            let values = col
                .values()
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("values utf8");
            for i in start..end {
                if keys.value(i) == key && values.value(i) == needle {
                    matched += 1;
                    break;
                }
            }
        }
    }
    let duration = start.elapsed();
    let bytes_read = std::fs::metadata(path)?.len(); // coarse — entire file bytes
    Ok(QueryStats {
        scanned_rows: scanned,
        matched_rows: matched,
        duration,
        bytes_read,
    })
}

/// Path B — predicate pushdown on the flattened `env` column.
pub fn label_filter_via_flat(
    path: &Path,
    column: &str,
    needle: &str,
) -> anyhow::Result<QueryStats> {
    let file = File::open(path)?;
    let start = Instant::now();
    let opts = ArrowReaderOptions::new();
    let builder = ParquetRecordBatchReaderBuilder::try_new_with_options(file, opts)?;
    let schema = builder.schema().clone();
    let column_idx = schema.index_of(column)?;
    let projection = ProjectionMask::leaves(builder.parquet_schema(), [column_idx]);

    let needle = needle.to_string();
    let column_name = column.to_string();
    let predicate_projection =
        ProjectionMask::leaves(builder.parquet_schema(), [column_idx]);
    let predicate = ArrowPredicate::new(predicate_projection, move |batch| {
        let col = batch
            .column_by_name(&column_name)
            .expect("flattened column present")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("utf8");
        let mask: arrow::array::BooleanArray = (0..col.len())
            .map(|i| {
                if col.is_valid(i) {
                    Some(col.value(i) == needle)
                } else {
                    Some(false)
                }
            })
            .collect();
        Ok(mask)
    });
    let reader = builder
        .with_projection(projection)
        .with_row_filter(RowFilter::new(vec![Box::new(predicate)]))
        .build()?;

    let mut scanned = 0u64;
    let mut matched = 0u64;
    for batch in reader {
        let batch = batch?;
        // With pushdown, scanned == matched: the filter runs inside the reader.
        scanned += batch.num_rows() as u64;
        matched += batch.num_rows() as u64;
    }
    let duration = start.elapsed();
    let bytes_read = std::fs::metadata(path)?.len();
    Ok(QueryStats {
        scanned_rows: scanned,
        matched_rows: matched,
        duration,
        bytes_read,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow::write_parquet;
    use crate::gen::generate;

    #[test]
    fn map_and_flat_filters_agree_on_count() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agree.parquet");
        let records = generate(1, 5_000);
        write_parquet(&records, &path).unwrap();

        let a = label_filter_via_map(&path, "env", "v0").unwrap();
        let b = label_filter_via_flat(&path, "env", "v0").unwrap();
        assert_eq!(a.matched_rows, b.matched_rows);
    }

    #[test]
    fn filters_find_expected_values() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hits.parquet");
        let records = generate(7, 10_000);
        write_parquet(&records, &path).unwrap();

        // `env` is a label key, values are v0..v11 — at least v0 should hit.
        let stats = label_filter_via_map(&path, "env", "v0").unwrap();
        assert!(stats.matched_rows > 0);
        assert!(stats.scanned_rows == 10_000);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --manifest-path bench/format-fit/Cargo.toml`
Expected: 5 (gen) + 3 (arrow) + 2 (query) = 10 tests pass.

- [ ] **Step 3: Commit**

```bash
git add bench/format-fit/src/query.rs
git commit -m "feat(bench): map-column vs flattened-column label filter"
```

---

## Task 5: CLI orchestration + report printer

**Files:**
- Modify: `bench/format-fit/src/main.rs`

- [ ] **Step 1: Replace `main.rs`**

```rust
//! `format-fit` — one-shot benchmark: is arrow-rs + parquet the right M1 format?

mod arrow;
mod gen;
mod query;

use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "format-fit", about = "Sift M1 format-fit spike")]
struct Cli {
    /// How many synthetic records to generate and write.
    #[arg(long, default_value_t = 10_000_000)]
    records: usize,
    /// Parquet output path. Parent dirs created if missing.
    #[arg(long, default_value = "./target/format-fit/corpus.parquet")]
    output: PathBuf,
    /// RNG seed for deterministic runs.
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// How many times to run each query (timings are averaged).
    #[arg(long, default_value_t = 5)]
    query_iters: u32,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    println!("format-fit — Sift M1 format-fit spike");
    println!("records    = {}", cli.records);
    println!("output     = {}", cli.output.display());
    println!("seed       = {}", cli.seed);
    println!("query_iters= {}", cli.query_iters);
    println!();

    // 1) Generate.
    let t0 = Instant::now();
    let records = gen::generate(cli.seed, cli.records);
    let gen_dur = t0.elapsed();
    println!("[1/4] generate: {:.2}s", gen_dur.as_secs_f64());

    // Estimate the uncompressed in-memory size. Rough lower bound for write-
    // amplification: sum of body + labels + fields_json lengths. This is not
    // perfect (it doesn't count the ~30 bytes of fixed-width columns per row)
    // but it's the honest "what the user sent us" number.
    let logical_bytes: u64 = records
        .iter()
        .map(|r| {
            (r.host.len()
                + r.app.len()
                + r.stream_id.len()
                + r.body.len()
                + r.labels.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>()
                + r.fields.iter().map(|(k, v)| k.len() + v.to_string().len()).sum::<usize>()
                + 30) as u64
        })
        .sum();
    println!("       ~{:.2} GiB logical input", gib(logical_bytes));

    // 2) Write.
    let t0 = Instant::now();
    let stats = arrow::write_parquet(&records, &cli.output)?;
    let write_dur = t0.elapsed();
    println!(
        "[2/4] write:    {:.2}s → {:.2} MiB on disk ({} row groups)",
        write_dur.as_secs_f64(),
        stats.bytes_on_disk as f64 / 1024.0 / 1024.0,
        stats.row_groups_written
    );
    println!(
        "       throughput: {:.0} records/s, {:.1} MiB/s in, {:.1} MiB/s out",
        cli.records as f64 / write_dur.as_secs_f64(),
        logical_bytes as f64 / 1024.0 / 1024.0 / write_dur.as_secs_f64(),
        stats.bytes_on_disk as f64 / 1024.0 / 1024.0 / write_dur.as_secs_f64()
    );
    let amp = stats.bytes_on_disk as f64 / logical_bytes as f64;
    println!(
        "       write amplification: {:.2}× (on disk / logical input)",
        amp
    );

    // 3) Footer.
    let footer = arrow::footer_size(&cli.output)?;
    println!(
        "[3/4] footer:   {:.2} KiB ({:.2}% of file)",
        footer as f64 / 1024.0,
        footer as f64 * 100.0 / stats.bytes_on_disk as f64
    );

    // 4) Label filter timings, averaged.
    println!("[4/4] label filter `env = v0`  (avg of {} iters)", cli.query_iters);
    let (map_dur, map_matched) = bench_query(cli.query_iters, || {
        query::label_filter_via_map(&cli.output, "env", "v0")
    })?;
    println!(
        "       via Map column:          {:>8.2} ms  → {} matches",
        ms(map_dur),
        map_matched
    );
    let (flat_dur, flat_matched) = bench_query(cli.query_iters, || {
        query::label_filter_via_flat(&cli.output, "env", "v0")
    })?;
    println!(
        "       via flattened `env` col: {:>8.2} ms  → {} matches",
        ms(flat_dur),
        flat_matched
    );
    let speedup = map_dur.as_secs_f64() / flat_dur.as_secs_f64();
    println!("       speedup of flattened over map: {:.1}×", speedup);

    // Drop the in-memory records explicitly before exit so we don't dominate
    // the final timing with a Vec<Record> destructor.
    drop(records);

    println!();
    println!("see docs/m1-format-fit-report.md for the decision once populated.");
    Ok(())
}

fn bench_query<F, E>(iters: u32, mut run: F) -> anyhow::Result<(std::time::Duration, u64)>
where
    F: FnMut() -> Result<query::QueryStats, E>,
    E: Into<anyhow::Error>,
{
    let mut total = std::time::Duration::ZERO;
    let mut last_matched = 0u64;
    for _ in 0..iters {
        let stats = run().map_err(Into::into)?;
        total += stats.duration;
        last_matched = stats.matched_rows;
    }
    Ok((total / iters, last_matched))
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
```

- [ ] **Step 2: Verify the whole thing builds in release mode**

Run: `cargo build --release --manifest-path bench/format-fit/Cargo.toml`
Expected: compiles.

- [ ] **Step 3: Run a small smoke pass (100_000 records) end-to-end**

Run:
```
cargo run --release --manifest-path bench/format-fit/Cargo.toml -- --records 100000 --output ./target/format-fit/smoke.parquet
```

Expected: report prints with non-zero numbers; no panics; `smoke.parquet` exists.

- [ ] **Step 4: Commit**

```bash
git add bench/format-fit/src/main.rs
git commit -m "feat(bench): format-fit CLI orchestration and report printer"
```

---

## Task 6: Run the 10M benchmark and write the report

**Files:**
- Create: `docs/m1-format-fit-report.md`

- [ ] **Step 1: Run the full 10M benchmark**

Run:
```
cargo run --release --manifest-path bench/format-fit/Cargo.toml -- --records 10000000 --output ./target/format-fit/corpus.parquet --seed 42
```

Expected: prints a full report. This takes several minutes and writes several GB to `./target/format-fit/`. Capture the stdout.

- [ ] **Step 2: Write `docs/m1-format-fit-report.md` with the numbers**

Template — fill the bracketed sections from the actual run output:

```markdown
# M1 Format-Fit Report

**Spec:** `docs/superpowers/plans/2026-04-23-m1-format-fit-spike.md`
**Date:** 2026-04-23
**Hardware:** [paste: CPU, cores, RAM, disk type from `/proc/cpuinfo` or equivalent]
**Rust:** 1.94.1
**Crates:** arrow 53.x, parquet 53.x, zstd level 3

## Corpus

- 10,000,000 synthetic records, seed `42`.
- Generator: `bench/format-fit/src/gen.rs`. 500 hosts, 30 apps, 80 stream_ids, 20-key label pool, 16-key field pool.
- Logical (pre-serialization) size: **[X] GiB**.

## Results

| Metric | Value |
|---|---|
| Write time | **[X] s** |
| Write throughput (records/s) | **[X]** |
| Write throughput (MiB/s in / out) | **[X] / [Y]** |
| Bytes on disk | **[X] MiB** |
| Write amplification (disk / logical) | **[X]×** |
| Row groups written | **[N]** |
| Footer size | **[X] KiB** ([Y]% of file) |
| Label filter `env=v0` via Map column | **[X] ms** ([N] matches) |
| Label filter `env=v0` via flattened column | **[Y] ms** ([N] matches) |
| Flattened speedup | **[Z]×** |

Full stdout captured at [attach or link].

## Assessment against acceptance thresholds

| Criterion | Threshold | Actual | Pass? |
|---|---|---|---|
| Write amplification ≤ 0.6× | ≤ 0.6 | [X] | ✅ / ❌ |
| Footer size ≤ 2% of file | ≤ 2% | [X]% | ✅ / ❌ |
| Write throughput ≥ 50k rec/s | ≥ 50,000 | [X] | ✅ / ❌ |
| Flattened label filter ≤ 500 ms on 10M rows | ≤ 500 ms | [X] ms | ✅ / ❌ |
| Map-column label filter ≤ 5 s on 10M rows | ≤ 5 s | [X] ms | ✅ / ❌ |

## Decision

[Fill in: go / no-go / go-with-conditions. Reference the table above. If
"go", name any design adjustments — e.g. "flatten `env`, `region`, `tenant`
into top-level columns by default because the Map-column filter is 10×
slower than the flattened equivalent".]

## Follow-up actions

- [ ] If go: write ADR-001 "Adopt arrow-rs + parquet for M1 segments".
- [ ] If no-go: write ADR-001 documenting the pivot (to which format) and why.
- [ ] Delete `./target/format-fit/` after the numbers are captured.

## Reproduce

```
cargo run --release --manifest-path bench/format-fit/Cargo.toml -- \
  --records 10000000 \
  --output ./target/format-fit/corpus.parquet \
  --seed 42
```
```

- [ ] **Step 3: Commit the report**

```bash
git add docs/m1-format-fit-report.md
git commit -m "docs(m1): record format-fit spike results (10M records)"
```

---

## Task 7: ADR capturing the M1 format decision

**Files:**
- Create: `docs/decisions/ADR-001-m1-storage-format.md`

(Note: the repo doesn't have a `docs/decisions/` folder yet. ADRs also go into the vault at `Vault/Sift/20-Decisions/`; the repo copy is a small pointer / record for readers who aren't in the vault.)

- [ ] **Step 1: Create `docs/decisions/ADR-001-m1-storage-format.md`**

This task runs AFTER the report's "Decision" is filled in. Pick the version that matches:

### If go (arrow-rs + parquet adopted):

```markdown
# ADR-001: Adopt arrow-rs + parquet for M1 segments

**Status:** accepted
**Date:** 2026-04-23
**Context:** `docs/m1-format-fit-report.md`

## Context

M1 needs a column-oriented on-disk format for immutable segments. `PLAN.md` §4.2
proposed arrow-rs + parquet; the format-fit spike (`docs/superpowers/plans/
2026-04-23-m1-format-fit-spike.md`) measured whether the numbers support that
bet for log-shaped data.

## Decision

Adopt `arrow-rs` + `parquet` as the M1 segment format. Configuration knobs:

- Compression: zstd level 3.
- Row group size: 8,192 rows.
- Flatten `env`, `region`, and `tenant` (if present in the config) into
  top-level string columns; other labels stay in the schema-flexible
  `labels: Map<String,String>` column.
- Fields column: serialized JSON for M1. Revisit nested Arrow types at M2
  if query selectivity on fields becomes a bottleneck.

## Alternatives considered

- Custom columnar format — ruled out; we don't need the flexibility more than
  we need the ecosystem (Polars, DataFusion interop).
- Raw Arrow IPC files (no parquet) — smaller on disk for this corpus only if
  we add our own dictionary encoding; parquet handles this for us.

## Consequences

- The M1 plan can proceed with the WAL + memtable + parquet-segment design
  from `PLAN.md` §4.2.
- Flatten-by-convention requires a small config surface: which label keys to
  promote. Default `["env", "region", "tenant"]`, configurable per stream.

## References

- `docs/m1-format-fit-report.md` — measured numbers.
- `bench/format-fit/` — the spike itself.
```

### If no-go (pivot away from parquet):

```markdown
# ADR-001: Pivot away from parquet for M1 segments

**Status:** accepted
**Date:** 2026-04-23
**Context:** `docs/m1-format-fit-report.md`

## Context

[Name the failing metric(s) from the report: e.g. "write amplification of
1.4× is unacceptable for a storage layer aiming at weeks-of-retention" or
"Map-column label filter took 12 s on 10M rows which blows past the 5 s
acceptance threshold by 2.4×".]

## Decision

Adopt [chosen alternative — e.g. raw Arrow IPC with our own dictionary
encoding for the label column] for M1 segments.

## Alternatives considered

- Keep parquet and flatten every label into a top-level column. Rejected
  because [reason from the numbers].
- [Alternative 2]
- [Alternative 3]

## Consequences

- `PLAN.md` §4.2 needs an update to reflect the new format.
- Ecosystem interop (Polars, DataFusion) gets harder — we have to write our
  own TableProvider.

## References

- `docs/m1-format-fit-report.md` — measured numbers that forced the pivot.
```

- [ ] **Step 2: Mirror the ADR into the vault**

```
cp docs/decisions/ADR-001-m1-storage-format.md "C:/Users/vz/Programming/Vault/Sift/20-Decisions/ADR-001 M1 Storage Format.md"
```

Add frontmatter at the top of the vault copy matching the vault's ADR template:

```markdown
---
date: 2026-04-23
status: accepted
tags: [decision, m1, storage]
area: Server
superseded_by: ""
---
```

- [ ] **Step 3: Commit**

```bash
git add docs/decisions/ADR-001-m1-storage-format.md
git commit -m "docs: ADR-001 — M1 storage format (accepted)"
```

---

## Self-review

**Spec coverage (against `PLAN.md` §4.2's spike description):**

- "Write 10M real-ish records" — Tasks 2, 5, 6.
- "Measure write amplification" — reported in Task 5, captured in Task 6.
- "Measure footer size" — `arrow::footer_size` in Task 3, reported in Task 5, captured in Task 6.
- "Measure label-only query time" — Task 4 covers both Map-column and flattened variants; reported in Task 5, captured in Task 6.
- "If ugly, reconsider before compaction is built around the format" — Task 7 ADR.

**Intentional scope deviations from a hypothetical "full M1" plan:**

- No attempt at WAL, memtable, or multi-segment compaction in this spike. Those are M1 implementation, not format research.
- No DataFusion TableProvider. Point of the spike is the format fit; DataFusion interop is a given if we stay on parquet.
- No time-partitioning. The spike writes one file; partitioning is orthogonal to format fit.
- No text-index sidecar. That's M3's concern.

**Placeholder scan:** searched the plan for "TBD", "TODO", "implement later", "similar to" — none in task content. The report template (Task 6) has bracketed placeholders like `[X]`, but those are intentional — they're the real numbers the engineer fills in from the benchmark run, not unwritten plan content.

**Type consistency:**
- `WriteStats { bytes_on_disk, row_groups_written, row_group_meta_rows }` — defined in Task 3, used in Task 5.
- `QueryStats { scanned_rows, matched_rows, duration, bytes_read }` — defined in Task 4, used in Task 5.
- `generate(seed: u64, n: usize) -> Vec<Record>` — defined in Task 2, used in Tasks 3's test, 4's test, and 5.
- `write_parquet(records: &[Record], path: &Path) -> Result<WriteStats>` — Task 3, used in 4's test and Task 5.
- `label_filter_via_map` / `label_filter_via_flat` — defined in Task 4, used in Task 5's `bench_query`.

**Acceptance thresholds** (set in the plan, applied in Task 6's report):

| Criterion | Threshold | Rationale |
|---|---|---|
| Write amplification | ≤ 0.6× | Compressed logs usually sit around 3–6× input; 0.6× on disk means zstd is doing real work. If we're above 1.0× we're larger than the input, which makes parquet pointless for retention. |
| Footer size | ≤ 2% of file | Parquet metadata is notorious for ballooning on wide schemas; 2% keeps us in "don't worry about it" territory. |
| Write throughput | ≥ 50,000 rec/s | M0 ingest ceiling is ~5k rec/s; M1 aims at 25–30k/s with inline text indexing. 50k/s raw write throughput leaves headroom. |
| Flattened label filter | ≤ 500 ms on 10M rows | Interactive latency budget for a "tell me prod errors" query. |
| Map-column label filter | ≤ 5 s on 10M rows | Acceptable for ad-hoc schema-flexible queries; anything worse forces the flatten-everything approach. |

If any threshold fails by more than 2×, pivot (Task 7 alternate ADR). If it fails by less than 2× but more than 1×, go-with-conditions — document the workaround in the ADR.

---

Plan complete and saved to `docs/superpowers/plans/2026-04-23-m1-format-fit-spike.md`. Two execution options:

**1. Subagent-Driven (recommended)** — fresh subagent per task, review between tasks.

**2. Inline Execution** — I run the tasks in this session with checkpoints.

Which approach?
