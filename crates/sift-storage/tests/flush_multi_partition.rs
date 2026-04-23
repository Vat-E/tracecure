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
async fn flush_writes_one_segment_per_partition() {
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

    for (i, offset) in [0, 70, 130].iter().enumerate() {
        store.append(rec(&format!("r{i}"), *offset)).await.unwrap();
    }
    for extra in 0..10 {
        store
            .append(rec(&format!("filler {extra}"), 30 + extra))
            .await
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(200)).await;

    let segs_dir = tmp.path().join("segments");
    let mut names: Vec<String> = Vec::new();
    let mut rd = tokio::fs::read_dir(&segs_dir).await.unwrap();
    while let Ok(Some(entry)) = rd.next_entry().await {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();

    let distinct_partitions: std::collections::BTreeSet<String> = names
        .iter()
        .filter(|n| n.ends_with(".parquet") && !n.contains("compacting"))
        .map(|n| n.rsplit_once('-').unwrap().0.to_string())
        .collect();
    assert!(
        distinct_partitions.len() >= 3,
        "expected >=3 partitions; got {distinct_partitions:?}"
    );
}
