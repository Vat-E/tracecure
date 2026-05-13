use chrono::{DateTime, TimeZone, Utc};
use std::collections::BTreeMap;
use tracecure_core::{Level, Record};
use tracecure_storage::Store;

fn rec(body: &str, ts: DateTime<Utc>) -> Record {
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
async fn search_finds_memtable_records() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).await.unwrap();

    let t0 = Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap();
    store.append(rec("hello world", t0)).await.unwrap();
    store
        .append(rec("goodbye", t0 + chrono::Duration::seconds(1)))
        .await
        .unwrap();

    let (matches, scanned) = store.search("hello", 10, None, None).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].body, "hello world");
    assert_eq!(scanned, 2);
}

#[tokio::test]
async fn search_empty_query_returns_all_capped_at_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).await.unwrap();

    let t0 = Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap();
    for i in 0..5 {
        store
            .append(rec(&format!("m{i}"), t0 + chrono::Duration::seconds(i)))
            .await
            .unwrap();
    }

    let (matches, scanned) = store.search("", 3, None, None).await.unwrap();
    assert_eq!(matches.len(), 3);
    assert_eq!(scanned, 5);
    // Newest-first ordering.
    assert_eq!(matches[0].body, "m4");
    assert_eq!(matches[1].body, "m3");
    assert_eq!(matches[2].body, "m2");
}
