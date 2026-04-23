use std::time::Duration;

use chrono::{TimeZone, Utc};
use sift_core::{Level, Record};
use sift_storage::{
    compactor::CompactorConfig, memtable::MemtableConfig, partition::PartitionConfig,
    store::StoreConfig, wal::WalConfig, Store,
};
use std::collections::BTreeMap;

fn rec_at(body: &str, ts: chrono::DateTime<Utc>) -> Record {
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
async fn compactor_merges_a_closed_partition() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StoreConfig {
        wal: WalConfig::default(),
        memtable: MemtableConfig {
            size_bytes: 64,
            age: Duration::from_secs(3600),
        },
        partition: PartitionConfig {
            granularity: Duration::from_secs(60),
            grace: Duration::from_millis(10),
        },
        compactor: CompactorConfig {
            tick_interval: Duration::from_secs(3600),
            enabled: false,
        },
    };
    let store = Store::open_with(tmp.path(), cfg).await.unwrap();

    let old = Utc.with_ymd_and_hms(2026, 4, 22, 0, 0, 0).unwrap();
    for i in 0..5 {
        store
            .append(rec_at(
                &format!("r{i}"),
                old + chrono::Duration::seconds(i * 2),
            ))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    let segs_dir = tmp.path().join("segments");
    let count_before = dir_count(&segs_dir).await;
    assert!(
        count_before >= 2,
        "expected >=2 pre-compact, got {count_before}"
    );

    store.trigger_compaction().await.unwrap();

    let count_after = dir_count(&segs_dir).await;
    assert_eq!(count_after, 1, "expected 1 segment post-compact");

    let (matches, _) = store
        .search(
            "",
            100,
            Some(old),
            Some(old + chrono::Duration::seconds(60)),
        )
        .await
        .unwrap();
    assert_eq!(matches.len(), 5);
}

async fn dir_count(p: &std::path::Path) -> usize {
    let mut rd = tokio::fs::read_dir(p).await.unwrap();
    let mut n = 0usize;
    while let Ok(Some(_)) = rd.next_entry().await {
        n += 1;
    }
    n
}
