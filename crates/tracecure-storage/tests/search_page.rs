//! Paging through a search: every record exactly once, newest first,
//! whatever the page size and however many records share a timestamp.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use tracecure_core::{Level, Record};
use tracecure_query::Query;
use tracecure_storage::memtable::MemtableConfig;
use tracecure_storage::{CompactorConfig, SearchFilter, Store, StoreConfig};

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 23, 12, 0, 0).unwrap()
}

fn rec(n: usize, second: i64) -> Record {
    Record {
        timestamp: t0() + chrono::Duration::seconds(second),
        host: "h".into(),
        app: "a".into(),
        stream_id: "s".into(),
        level: Level::Info,
        body: format!("r{n:03}"),
        labels: BTreeMap::from([(
            "parity".to_string(),
            if n.is_multiple_of(2) { "even" } else { "odd" }.to_string(),
        )]),
        fields: BTreeMap::new(),
        raw: None,
    }
}

/// Flush often, so the records end up spread over segments and the memtable.
fn small_segments() -> StoreConfig {
    StoreConfig {
        memtable: MemtableConfig {
            size_bytes: 2_000,
            age: Duration::from_secs(3600),
        },
        compactor: CompactorConfig {
            enabled: false,
            ..CompactorConfig::default()
        },
        ..StoreConfig::default()
    }
}

async fn all_pages(store: &Store, filter: &SearchFilter, limit: usize) -> Vec<Vec<Record>> {
    let mut pages = Vec::new();
    let mut before = None;
    loop {
        let page = store
            .search_page(filter, limit, None, None, before)
            .await
            .unwrap();
        let done = page.next_before.is_none();
        if let Some(next) = page.next_before {
            assert!(before.is_none_or(|b| next < b), "the cursor must move back");
            before = Some(next);
        }
        if !page.records.is_empty() {
            pages.push(page.records);
        }
        if done {
            return pages;
        }
        assert!(pages.len() < 1_000, "paging does not terminate");
    }
}

#[tokio::test]
async fn pages_cover_every_record_once_even_through_shared_timestamps() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open_with(tmp.path(), small_segments())
        .await
        .unwrap();

    // 120 records; seconds 10..=14 each hold a burst of ten that share one
    // timestamp, so page boundaries are bound to land inside a burst.
    let mut n = 0usize;
    let mut expected = Vec::new();
    for second in 0..70i64 {
        let burst = if (10..15).contains(&second) { 11 } else { 1 };
        for _ in 0..burst {
            let r = rec(n, second);
            store.append(r.clone()).await.unwrap();
            expected.push(r);
            n += 1;
        }
    }
    assert_eq!(expected.len(), 120);

    for limit in [1usize, 3, 7, 10, 50, 119, 120, 500] {
        let pages = all_pages(&store, &SearchFilter::default(), limit).await;
        let mut got: Vec<String> = pages.iter().flatten().map(|r| r.body.clone()).collect();
        // Newest first across the whole sequence of pages.
        let stamps: Vec<_> = pages.iter().flatten().map(|r| r.timestamp).collect();
        assert!(stamps.windows(2).all(|w| w[0] >= w[1]), "limit {limit}");
        got.sort();
        let mut want: Vec<String> = expected.iter().map(|r| r.body.clone()).collect();
        want.sort();
        assert_eq!(got, want, "limit {limit}: every record exactly once");
        // A page only exceeds the limit to finish a burst.
        assert!(pages.iter().all(|p| p.len() < limit + 11), "limit {limit}");
    }

    // The same through a query that filters on a label.
    let odd = SearchFilter::query(Query::parse(r#"{parity="odd"}"#).unwrap());
    let pages = all_pages(&store, &odd, 9).await;
    let got = pages.iter().flatten().count();
    assert_eq!(got, 60);
    assert!(pages.iter().flatten().all(|r| r.labels["parity"] == "odd"));

    store.close().await.unwrap();
}

#[tokio::test]
async fn a_short_page_is_the_last_one() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open_with(tmp.path(), small_segments())
        .await
        .unwrap();
    for n in 0..5 {
        store
            .append(rec(n, i64::try_from(n).unwrap()))
            .await
            .unwrap();
    }
    let page = store
        .search_page(&SearchFilter::default(), 10, None, None, None)
        .await
        .unwrap();
    assert_eq!(page.records.len(), 5);
    assert!(page.next_before.is_none());

    // `before` and `to` both bound the page; the earlier one wins.
    let page = store
        .search_page(
            &SearchFilter::default(),
            10,
            None,
            Some(t0() + chrono::Duration::seconds(4)),
            Some(t0() + chrono::Duration::seconds(2)),
        )
        .await
        .unwrap();
    let bodies: Vec<_> = page.records.iter().map(|r| r.body.as_str()).collect();
    assert_eq!(bodies, ["r001", "r000"]);
    store.close().await.unwrap();
}
