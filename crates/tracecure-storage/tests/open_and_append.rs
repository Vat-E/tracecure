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
async fn open_accepts_appends_and_broadcasts() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).await.unwrap();
    let mut rx = store.subscribe();

    store.append(rec("alpha")).await.unwrap();
    store.append(rec("beta")).await.unwrap();

    let a = rx.recv().await.unwrap();
    let b = rx.recv().await.unwrap();
    assert_eq!(a.body, "alpha");
    assert_eq!(b.body, "beta");
}
