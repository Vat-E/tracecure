//! Regression: the segment sequence counter must not restart at 1 on open.
//!
//! Segment files are named `{partition_label}-{seq:010}.parquet`, and the
//! label itself contains `-` (`2026-04-23T13`). `Store::open` recovers the
//! counter by parsing the file stem — parsing the *whole* stem can never
//! succeed, so `max()` is `None` and the counter silently restarts at 1.
//!
//! That is not cosmetic. A flush after restart writes
//! `…-0000000001.parquet` over the one already there, destroying every
//! record in it.

use std::time::Duration;

use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use tracecure_core::{Level, Record};
use tracecure_storage::{memtable::MemtableConfig, store::StoreConfig, wal::WalConfig, Store};

/// Fixed hour, so every record lands in the same partition and a restart
/// collides on the filename.
fn rec(body: &str, offset: i64) -> Record {
    Record {
        timestamp: Utc.with_ymd_and_hms(2026, 4, 23, 13, 0, 0).unwrap()
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

fn cfg() -> StoreConfig {
    StoreConfig {
        wal: WalConfig {
            fsync_interval: Duration::from_millis(5),
            size_trigger_bytes: 4096,
        },
        memtable: MemtableConfig {
            // Small, so appends force real flushes — `flush_now` is private.
            size_bytes: 256,
            age: Duration::from_secs(3600),
        },
        ..Default::default()
    }
}

// `u8` so it widens losslessly to both the `i64` record index and the `usize`
// counts it is compared against.
const PER_RUN: u8 = 50;

/// Append `PER_RUN` records tagged `label`, let them flush, close cleanly.
async fn run_once(dir: &std::path::Path, label: &str) {
    let store = Store::open_with(dir, cfg()).await.unwrap();
    for i in 0..PER_RUN {
        store.append(rec(label, i64::from(i))).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    store.close().await.unwrap();
}

async fn count_matching(dir: &std::path::Path, needle: &str) -> usize {
    let store = Store::open_with(dir, cfg()).await.unwrap();
    let (matches, _) = store.search(needle, 1000, None, None).await.unwrap();
    let n = matches.len();
    store.close().await.unwrap();
    n
}

/// The data-loss case: records written before a restart must survive a flush
/// that happens after it.
#[tokio::test]
async fn records_from_before_a_restart_are_not_overwritten() {
    let tmp = tempfile::tempdir().unwrap();

    run_once(tmp.path(), "first-run").await;
    let before = count_matching(tmp.path(), "first-run").await;
    assert_eq!(
        before,
        usize::from(PER_RUN),
        "baseline: all first-run records should be readable before the restart"
    );

    // Restart and flush into the SAME partition.
    run_once(tmp.path(), "second-run").await;

    let first_after = count_matching(tmp.path(), "first-run").await;
    let second_after = count_matching(tmp.path(), "second-run").await;

    assert_eq!(
        first_after,
        usize::from(PER_RUN),
        "records written before the restart were destroyed: {first_after} of {PER_RUN} survived. \
         The post-restart flush reused sequence numbers and overwrote existing segments."
    );
    assert_eq!(
        second_after,
        usize::from(PER_RUN),
        "second run's own records"
    );
}

/// The mechanism, checked directly: sequence numbers must never be reused.
#[tokio::test]
async fn segment_sequence_numbers_are_never_reused_across_restarts() {
    let tmp = tempfile::tempdir().unwrap();

    let mut seen: Vec<u64> = Vec::new();
    for run in 0..3 {
        run_once(tmp.path(), &format!("run-{run}")).await;

        let mut rd = tokio::fs::read_dir(tmp.path().join("segments"))
            .await
            .unwrap();
        let mut this_run: Vec<u64> = Vec::new();
        while let Some(e) = rd.next_entry().await.unwrap() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".parquet") {
                if let Some((_, seq)) = stem.rsplit_once('-') {
                    if let Ok(n) = seq.parse::<u64>() {
                        this_run.push(n);
                    }
                }
            }
        }
        seen = this_run;
    }

    let total = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(
        seen.len(),
        total,
        "duplicate sequence numbers on disk after restarts"
    );
    assert!(
        seen.first().copied().unwrap_or(0) >= 1,
        "sequence numbers start at 1"
    );
}
