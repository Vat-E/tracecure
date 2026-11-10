//! Criterion benches for the storage layer, on a synthetic corpus.
//!
//! ```text
//! cargo bench -p tracecure-storage                                  # 200k records, a smoke run
//! TRACECURE_BENCH_RECORDS=10000000 cargo bench -p tracecure-storage # the M1 corpus
//! ```
//!
//! The corpus is built once through the real write path — `Store::append`,
//! WAL, memtable, flush, compaction — and kept under the target directory,
//! keyed by its size, so later runs only pay to open it. Delete
//! `target/tmp/bench-corpus-*` to rebuild.
//!
//! What is measured:
//!
//! - `ingest/append`: `Store::append` throughput into a fresh store. This is
//!   WAL buffering plus the memtable push; fsync is the background ticker's
//!   job and the batch is too small to trigger a segment flush.
//! - `search/*`: `Store::search_filtered` over the whole corpus with
//!   `limit = 500`, one bench per shape of question the UI asks.
//!
//! The end-to-end write rate (append *and* flush to parquet) is not a
//! criterion bench — it happens once, while the corpus is built — so it is
//! printed then.

use std::collections::BTreeMap;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeZone, Utc};
use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use tracecure_core::{Level, Record};
use tracecure_storage::memtable::MemtableConfig;
use tracecure_storage::{CompactorConfig, NamePattern, SearchFilter, Store, StoreConfig};

const DEFAULT_RECORDS: usize = 200_000;
const SEARCH_LIMIT: usize = 500;
const INGEST_BATCH: usize = 50_000;
/// One record in this many carries the rare search term.
const NEEDLE_EVERY: usize = 100_000;
const NEEDLE: &str = "needle-in-the-haystack";

const APPS: &[&str] = &[
    "api",
    "worker",
    "scheduler",
    "ingest",
    "billing",
    "auth",
    "mailer",
    "cron",
    "ci",
    "sync",
    "etl",
    "webhook",
    "consumer",
    "producer",
    "gateway",
    "search",
    "index",
    "cache",
    "pubsub",
    "streamer",
    "notifier",
    "archiver",
    "metrics",
    "alerter",
    "flagd",
    "router",
    "dedup",
    "compactor",
    "gc",
    "util",
];
const WORDS: &[&str] = &[
    "request",
    "processed",
    "timeout",
    "retry",
    "success",
    "failure",
    "cache",
    "hit",
    "miss",
    "slow",
    "fast",
    "queued",
    "dispatched",
    "acknowledged",
    "rejected",
    "validated",
    "scheduled",
    "completed",
    "aborted",
    "resumed",
    "connection",
    "upstream",
    "payload",
    "session",
];
const LABEL_KEYS: &[&str] = &["env", "region", "cluster", "tier", "version", "tenant"];
const HOSTS: u64 = 500;
const STREAMS: u64 = 80;

/// `SplitMix64`: tiny, deterministic, and good enough to shape a corpus.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a>(&mut self, pool: &[&'a str]) -> &'a str {
        let i = usize::try_from(self.below(pool.len() as u64)).unwrap_or(0);
        pool[i]
    }
}

fn corpus_start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 23, 12, 0, 0).unwrap()
}

/// A stream of log-shaped records: ~500 hosts, 30 apps, 80 streams, mostly
/// `info`, bodies of 6–30 words, 2–4 labels, 0–3 fields. Timestamps advance
/// by 3 ms on average, so ten million records span about nine hours — nine
/// or ten one-hour partitions.
struct Corpus {
    rng: Rng,
    index: usize,
    offset_ms: i64,
}

impl Corpus {
    fn new(seed: u64) -> Self {
        Self {
            rng: Rng(seed),
            index: 0,
            offset_ms: 0,
        }
    }
}

impl Iterator for Corpus {
    type Item = Record;

    fn next(&mut self) -> Option<Record> {
        let rng = &mut self.rng;
        self.offset_ms += i64::try_from(rng.below(7)).unwrap_or(0);
        let level = match rng.below(100) {
            0..=1 => Level::Trace,
            2..=21 => Level::Debug,
            22..=91 => Level::Info,
            92..=96 => Level::Warn,
            97..=98 => Level::Error,
            _ => Level::Fatal,
        };

        let words = 6 + rng.below(25);
        let mut body = String::with_capacity(200);
        for w in 0..words {
            if w > 0 {
                body.push(' ');
            }
            body.push_str(rng.pick(WORDS));
        }
        if self.index % NEEDLE_EVERY == NEEDLE_EVERY / 2 {
            body.push(' ');
            body.push_str(NEEDLE);
        }

        let mut labels = BTreeMap::new();
        for _ in 0..2 + rng.below(3) {
            labels.insert(
                rng.pick(LABEL_KEYS).to_string(),
                format!("v{}", rng.below(12)),
            );
        }
        let mut fields = BTreeMap::new();
        let wanted = usize::try_from(rng.below(4)).unwrap_or(0);
        for key in ["duration_ms", "request_id", "cached"]
            .into_iter()
            .take(wanted)
        {
            let value = match key {
                "duration_ms" => serde_json::json!(rng.below(5_000)),
                "request_id" => serde_json::json!(format!("{:016x}", rng.next())),
                _ => serde_json::json!(rng.below(2) == 0),
            };
            fields.insert(key.to_string(), value);
        }

        let record = Record {
            timestamp: corpus_start() + chrono::Duration::milliseconds(self.offset_ms),
            host: format!("host-{:04}", rng.below(HOSTS)),
            app: rng.pick(APPS).to_string(),
            stream_id: format!("stream-{:03}", rng.below(STREAMS)),
            level,
            body,
            labels,
            fields,
            raw: None,
        };
        self.index += 1;
        Some(record)
    }
}

fn corpus_records() -> usize {
    std::env::var("TRACECURE_BENCH_RECORDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_RECORDS)
}

fn corpus_dir(records: usize) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("bench-corpus-{records}"))
}

/// A store that only does what it is told: no background compaction, and a
/// memtable that flushes on size alone.
fn quiet_config(memtable_age: Duration) -> StoreConfig {
    StoreConfig {
        memtable: MemtableConfig {
            size_bytes: 128 * 1024 * 1024,
            age: memtable_age,
        },
        compactor: CompactorConfig {
            enabled: false,
            ..CompactorConfig::default()
        },
        ..StoreConfig::default()
    }
}

const NEVER: Duration = Duration::from_secs(24 * 60 * 60);

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .map(|e| e.metadata().map_or(0, |m| m.len()))
            .sum()
    })
}

/// Build the corpus through the real write path, unless it is already there.
async fn ensure_corpus(records: usize) -> anyhow::Result<PathBuf> {
    let dir = corpus_dir(records);
    let marker = dir.join("complete");
    if marker.exists() {
        return Ok(dir);
    }
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    eprintln!("building a {records}-record corpus in {}", dir.display());

    let started = Instant::now();
    let store = Store::open_with(&dir, quiet_config(NEVER)).await?;
    for record in Corpus::new(42).take(records) {
        store.append(record).await?;
    }
    store.close().await?;
    // Whatever is still in the memtable is only in the WAL. Reopening with a
    // memtable that is always "old enough" replays it and flushes it.
    let store = Store::open_with(&dir, quiet_config(Duration::ZERO)).await?;
    let written = started.elapsed();

    let before = std::fs::read_dir(dir.join("segments"))?.count();
    let compaction_started = Instant::now();
    store.trigger_compaction().await?;
    let compacted = compaction_started.elapsed();
    store.close().await?;
    let after = std::fs::read_dir(dir.join("segments"))?.count();

    #[allow(clippy::cast_precision_loss)]
    let rate = records as f64 / written.as_secs_f64();
    eprintln!(
        "corpus: {records} records written and flushed in {:.1}s ({rate:.0} records/s); \
         compaction {before} -> {after} segments in {:.1}s; {} MiB on disk",
        written.as_secs_f64(),
        compacted.as_secs_f64(),
        dir_size(&dir.join("segments")) / (1024 * 1024),
    );
    std::fs::write(marker, format!("{records}\n"))?;
    Ok(dir)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

fn ingest(c: &mut Criterion) {
    let rt = runtime();
    let batch: Vec<Record> = Corpus::new(7).take(INGEST_BATCH).collect();

    let mut group = c.benchmark_group("ingest");
    group.throughput(Throughput::Elements(INGEST_BATCH as u64));
    group.sample_size(20);
    group.bench_function("append", |b| {
        b.iter_batched(
            || {
                let dir = tempfile::tempdir().expect("tempdir");
                let store = rt
                    .block_on(Store::open_with(dir.path(), quiet_config(NEVER)))
                    .expect("open store");
                (dir, store, batch.clone())
            },
            |(dir, store, records)| {
                rt.block_on(async {
                    for record in records {
                        store.append(record).await.expect("append");
                    }
                });
                // Returned so that closing and deleting are not timed.
                (dir, store)
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn only(pattern: &str) -> Vec<NamePattern> {
    vec![NamePattern::parse(pattern)]
}

/// A search to time: its name, the filter, and the `[from, to)` range.
type Case = (
    &'static str,
    SearchFilter,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

fn search(c: &mut Criterion) {
    let rt = runtime();
    let records = corpus_records();
    let dir = rt.block_on(ensure_corpus(records)).expect("build corpus");
    let store = rt
        .block_on(Store::open_with(&dir, quiet_config(NEVER)))
        .expect("open corpus");

    let start = corpus_start();
    let hour = chrono::Duration::hours(1);
    let any = SearchFilter::default();
    let cases: Vec<Case> = vec![
        ("newest", any.clone(), None, None),
        ("text_rare", SearchFilter::text(NEEDLE), None, None),
        (
            "text_absent",
            SearchFilter::text("no-such-term"),
            None,
            None,
        ),
        (
            "level_error",
            SearchFilter {
                levels: vec![Level::Error, Level::Fatal],
                ..any.clone()
            },
            None,
            None,
        ),
        (
            "one_app",
            SearchFilter {
                apps: only("billing"),
                ..any.clone()
            },
            None,
            None,
        ),
        (
            "one_stream",
            SearchFilter {
                streams: only("stream-017"),
                ..any.clone()
            },
            None,
            None,
        ),
        (
            "host_prefix",
            SearchFilter {
                hosts: only("host-04*"),
                ..any.clone()
            },
            None,
            None,
        ),
        ("one_hour", any.clone(), Some(start), Some(start + hour)),
        (
            "one_hour_text_absent",
            SearchFilter::text("no-such-term"),
            Some(start),
            Some(start + hour),
        ),
    ];

    let mut group = c.benchmark_group(format!("search/{records}"));
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));
    for (name, filter, from, to) in &cases {
        group.bench_function(*name, |b| {
            b.iter(|| {
                let found = rt
                    .block_on(store.search_filtered(filter, SEARCH_LIMIT, *from, *to))
                    .expect("search");
                black_box(found)
            });
        });
    }
    group.finish();

    rt.block_on(store.close()).expect("close corpus");
}

criterion_group!(benches, ingest, search);
criterion_main!(benches);
