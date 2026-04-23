use std::time::Duration;

use chrono::{TimeZone, Utc};
use sift_core::{Level, Record};
use sift_storage::{
    memtable::MemtableConfig, partition::PartitionConfig, store::StoreConfig,
    wal::WalConfig, Store,
};
use std::collections::BTreeMap;

fn rec(body: &str, offset_secs: i64) -> Record {
    let base = Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap();
    Record {
        timestamp: base + chrono::Duration::seconds(offset_secs),
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
async fn time_range_prunes_segments() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StoreConfig {
        wal: WalConfig {
            fsync_interval: Duration::from_millis(5),
            size_trigger_bytes: 64 * 1024,
        },
        memtable: MemtableConfig {
            size_bytes: 64,
            age: Duration::from_secs(3600),
        },
        partition: PartitionConfig {
            granularity: Duration::from_secs(60),
            grace: Duration::from_secs(30),
        },
    };
    let store = Store::open_with(tmp.path(), cfg).await.unwrap();

    for i in 0..15 {
        let offset = i64::from(i * 12);
        store.append(rec(&format!("rec{i}"), offset)).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    let base = Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap();
    let (matches, scanned) = store
        .search(
            "",
            100,
            Some(base + chrono::Duration::seconds(60)),
            Some(base + chrono::Duration::seconds(120)),
        )
        .await
        .unwrap();

    for r in &matches {
        let offset = (r.timestamp - base).num_seconds();
        assert!(
            (60..120).contains(&offset),
            "record outside range: offset={offset}"
        );
    }
    assert!(scanned > 0 && scanned < 15, "scanned={scanned}");
}

#[tokio::test]
async fn search_without_time_range_preserves_sp2_behaviour() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).await.unwrap();

    for i in 0..5 {
        store.append(rec(&format!("m{i}"), i)).await.unwrap();
    }

    let (matches, scanned) = store.search("", 100, None, None).await.unwrap();
    assert_eq!(matches.len(), 5);
    assert_eq!(scanned, 5);
}
