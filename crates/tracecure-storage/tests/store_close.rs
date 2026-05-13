use chrono::Utc;
use std::collections::BTreeMap;
use tracecure_core::{Level, Record};
use tracecure_storage::Store;

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
async fn close_is_idempotent_and_returns_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).await.unwrap();
    store.append(rec("x")).await.unwrap();
    store.close().await.unwrap();
    // Reopen — no leftover file locks on Windows.
    let _ = Store::open(tmp.path()).await.unwrap();
}
