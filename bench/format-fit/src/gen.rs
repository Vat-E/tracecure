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
use tracecure_core::{Level, Record};

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

/// One call returns `n` records with timestamps in a contiguous ~10h window.
pub fn generate(seed: u64, n: usize) -> Vec<Record> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // Precompute pools.
    let hosts: Vec<String> = (0..HOST_POOL).map(|i| format!("host-{i:04}")).collect();
    let streams: Vec<String> = (0..STREAM_POOL).map(|i| format!("stream-{i:03}")).collect();

    let start: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 4, 23, 12, 0, 0).unwrap();
    let mut out = Vec::with_capacity(n);
    let mut ts_offset_ms: i64 = 0;

    for _ in 0..n {
        // Timestamp marches forward in ~3.6 ms increments on average across
        // 10M records → ~10h total window. Realistic-ish for a busy service.
        // Accumulated to guarantee non-decreasing order.
        ts_offset_ms += rng.gen_range(0..7);
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
