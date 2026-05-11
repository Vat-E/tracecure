use std::time::Duration;

use chrono::{TimeZone, Utc};
use sift_core::{Level, Record};
use sift_storage::{memtable::MemtableConfig, store::StoreConfig, wal::WalConfig, Store};
use std::collections::BTreeMap;

fn rec(body: &str, offset: i64) -> Record {
    Record {
        timestamp: Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap()
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

fn default_cfg() -> StoreConfig {
    StoreConfig {
        wal: WalConfig {
            fsync_interval: Duration::from_millis(5),
            size_trigger_bytes: 4096,
        },
        memtable: MemtableConfig {
            size_bytes: usize::MAX, // never auto-flush in this test
            age: Duration::from_secs(3600),
        },
        ..Default::default()
    }
}

#[tokio::test]
async fn records_survive_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let store = Store::open_with(tmp.path(), default_cfg()).await.unwrap();
        for i in 0..100 {
            store.append(rec(&format!("msg {i}"), i)).await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let store = Store::open_with(tmp.path(), default_cfg()).await.unwrap();
    let (matches, _scanned) = store.search("msg", 500, None, None).await.unwrap();
    assert_eq!(matches.len(), 100);
}

#[tokio::test]
async fn partial_trailing_frame_is_tolerated() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let store = Store::open_with(tmp.path(), default_cfg()).await.unwrap();
        for i in 0..5 {
            store.append(rec(&format!("ok {i}"), i)).await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let wal_path = tmp.path().join("wal").join("0000000001.log");
    let bytes = std::fs::read(&wal_path).unwrap();
    let mut truncated = bytes.clone();
    truncated.truncate(bytes.len() - 3);
    std::fs::write(&wal_path, truncated).unwrap();

    let store = Store::open_with(tmp.path(), default_cfg()).await.unwrap();
    let (matches, _) = store.search("ok", 100, None, None).await.unwrap();
    assert!(
        matches.len() >= 4,
        "expected >=4 records, got {}",
        matches.len()
    );
}
