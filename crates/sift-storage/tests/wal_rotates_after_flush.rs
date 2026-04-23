use std::time::Duration;

use chrono::Utc;
use sift_core::{Level, Record};
use sift_storage::{
    memtable::MemtableConfig, partition::PartitionConfig, store::StoreConfig,
    wal::WalConfig, Store,
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
async fn wal_rotates_after_flush() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StoreConfig {
        wal: WalConfig {
            fsync_interval: Duration::from_millis(5),
            size_trigger_bytes: 4096,
        },
        memtable: MemtableConfig {
            size_bytes: 256,
            age: Duration::from_secs(3600),
        },
        partition: PartitionConfig::default(),
    };
    let store = Store::open_with(tmp.path(), cfg).await.unwrap();

    for _ in 0..50 {
        store.append(rec("payload payload payload")).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    let current = tokio::fs::read_to_string(tmp.path().join("wal").join("current"))
        .await
        .unwrap();
    let current = current.trim();
    assert!(std::path::Path::new(current)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("log")));

    let mut rd = tokio::fs::read_dir(tmp.path().join("wal")).await.unwrap();
    let mut logs: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if std::path::Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))
        {
            logs.push(name);
        }
    }
    logs.sort();
    assert_eq!(logs.len(), 1, "expected exactly 1 WAL log, got {logs:?}");
    assert_eq!(logs[0], current);
}
