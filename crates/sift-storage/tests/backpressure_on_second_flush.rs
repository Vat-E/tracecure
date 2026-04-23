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

    for i in 0..200 {
        store.append(rec(&format!("record {i}"))).await.unwrap();
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    let (matches, scanned) = store.search("record", 1000).await.unwrap();
    assert_eq!(matches.len(), 200, "scanned={scanned}");
}
