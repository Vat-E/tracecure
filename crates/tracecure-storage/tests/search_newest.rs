//! A search stops reading segments once it has the newest `limit` matches.
//! These tests pin that the shortcut never changes the answer — including
//! when segments overlap in time, which is the normal state between a flush
//! and the next compaction.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use tracecure_core::{Level, Record};
use tracecure_storage::memtable::MemtableConfig;
use tracecure_storage::{CompactorConfig, SearchFilter, Store, StoreConfig};

fn rec(minute: i64, second: i64) -> Record {
    Record {
        timestamp: Utc.with_ymd_and_hms(2026, 4, 23, 12, 0, 0).unwrap()
            + chrono::Duration::seconds(minute * 60 + second),
        host: "h".into(),
        app: "a".into(),
        stream_id: "s".into(),
        level: if second % 2 == 0 {
            Level::Info
        } else {
            Level::Error
        },
        body: format!("m{minute:03} s{second:02}"),
        labels: BTreeMap::new(),
        fields: BTreeMap::new(),
        raw: None,
    }
}

/// Every append becomes its own segment, and nothing merges them.
fn one_segment_per_record() -> StoreConfig {
    StoreConfig {
        memtable: MemtableConfig {
            size_bytes: 1,
            age: Duration::from_secs(3600),
        },
        compactor: CompactorConfig {
            enabled: false,
            ..CompactorConfig::default()
        },
        ..StoreConfig::default()
    }
}

#[tokio::test]
async fn the_newest_matches_are_found_whatever_order_segments_were_written_in() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open_with(tmp.path(), one_segment_per_record())
        .await
        .unwrap();

    // Three hours of records, written in an order that is neither ascending
    // nor descending, so the segments' time ranges interleave.
    let mut all = Vec::new();
    for i in 0..60i64 {
        let minute = (i * 37) % 180;
        let r = rec(minute, i % 60);
        store.append(r.clone()).await.unwrap();
        all.push(r);
    }
    all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    let bodies = |rs: &[Record]| rs.iter().map(|r| r.body.clone()).collect::<Vec<_>>();

    for limit in [1usize, 5, 59, 60, 500] {
        let (got, scanned) = store.search("", limit, None, None).await.unwrap();
        let want = &all[..limit.min(all.len())];
        assert_eq!(bodies(&got), bodies(want), "limit {limit}");
        // What the range covers, not what had to be read.
        assert_eq!(scanned, 60, "limit {limit}");
    }

    let errors = SearchFilter::from_params("", Some("error"), None, None, None).unwrap();
    let want: Vec<Record> = all
        .iter()
        .filter(|r| r.level == Level::Error)
        .take(4)
        .cloned()
        .collect();
    let (got, _) = store.search_filtered(&errors, 4, None, None).await.unwrap();
    assert_eq!(bodies(&got), bodies(&want));

    // A range in the middle: the newest segments are skipped by the range,
    // the oldest by the shortcut.
    let from = Utc.with_ymd_and_hms(2026, 4, 23, 13, 0, 0).unwrap();
    let to = Utc.with_ymd_and_hms(2026, 4, 23, 14, 0, 0).unwrap();
    let want: Vec<Record> = all
        .iter()
        .filter(|r| r.timestamp >= from && r.timestamp < to)
        .take(3)
        .cloned()
        .collect();
    let (got, _) = store.search("", 3, Some(from), Some(to)).await.unwrap();
    assert_eq!(bodies(&got), bodies(&want));

    store.close().await.unwrap();
}
