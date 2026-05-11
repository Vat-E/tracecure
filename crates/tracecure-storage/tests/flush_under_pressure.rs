use std::time::Duration;

use chrono::Utc;
use tracecure_core::{Level, Record};
use tracecure_storage::{memtable::MemtableConfig, store::StoreConfig, wal::WalConfig, Store};
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
async fn flush_creates_segment_file() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StoreConfig {
        wal: WalConfig {
            fsync_interval: Duration::from_millis(5),
            size_trigger_bytes: 1024,
        },
        memtable: MemtableConfig {
            size_bytes: 512,
            age: Duration::from_secs(3600),
        },
        ..Default::default()
    };
    let store = Store::open_with(tmp.path(), cfg).await.unwrap();

    for i in 0..50 {
        store
            .append(rec(&format!("record number {i}")))
            .await
            .unwrap();
    }

    // Give the flush task time to produce the segment file.
    for _ in 0..40 {
        let segs = tokio::fs::read_dir(tmp.path().join("segments"))
            .await
            .unwrap();
        let count = read_dir_count(segs).await;
        if count > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let final_count = {
        let segs = tokio::fs::read_dir(tmp.path().join("segments"))
            .await
            .unwrap();
        read_dir_count(segs).await
    };
    assert!(
        final_count > 0,
        "expected at least one segment file, got {final_count}"
    );

    let (matches, _scanned) = store.search("record", 100, None, None).await.unwrap();
    assert!(
        matches.len() >= 50,
        "expected at least 50 matches, got {}",
        matches.len()
    );
}

async fn read_dir_count(mut rd: tokio::fs::ReadDir) -> usize {
    let mut n = 0usize;
    while let Ok(Some(_)) = rd.next_entry().await {
        n += 1;
    }
    n
}
