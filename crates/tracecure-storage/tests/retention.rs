//! Retention end to end: records go in, a compaction pass runs as of some
//! later time, and what is left on disk and in search is what the rules say.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use tracecure_core::{Level, Record};
use tracecure_storage::{
    compactor::CompactorConfig, memtable::MemtableConfig, partition::PartitionConfig,
    store::StoreConfig, wal::WalConfig, RetentionConfig, Store,
};

const DAY: Duration = Duration::from_secs(86_400);

fn rec(stream: &str, body: &str, ts: DateTime<Utc>) -> Record {
    Record {
        timestamp: ts,
        host: "h".into(),
        app: "a".into(),
        stream_id: stream.into(),
        level: Level::Info,
        body: body.into(),
        labels: BTreeMap::new(),
        fields: BTreeMap::new(),
        raw: None,
    }
}

/// One-hour partitions, a memtable so small every append flushes a segment,
/// and no background compactor — passes are run by hand, at a chosen time.
fn cfg(retention: RetentionConfig) -> StoreConfig {
    StoreConfig {
        wal: WalConfig::default(),
        memtable: MemtableConfig {
            size_bytes: 64,
            age: Duration::from_secs(3600),
        },
        partition: PartitionConfig {
            granularity: Duration::from_secs(3600),
            grace: Duration::from_millis(10),
        },
        compactor: CompactorConfig {
            tick_interval: Duration::from_secs(3600),
            enabled: false,
        },
        retention,
    }
}

fn rules(default: Option<Duration>, streams: &[(&str, Duration)]) -> RetentionConfig {
    RetentionConfig {
        default,
        per_stream: streams
            .iter()
            .map(|(s, d)| ((*s).to_string(), *d))
            .collect(),
    }
}

/// Start of the partition all the test records are written into.
fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 22, 10, 0, 0).unwrap()
}

/// `t0`'s partition ends an hour later; this is `days` after that.
fn days_after_partition_end(days: i64) -> DateTime<Utc> {
    t0() + chrono::Duration::hours(1) + chrono::Duration::days(days)
}

async fn segment_files(dir: &std::path::Path) -> Vec<String> {
    let mut rd = tokio::fs::read_dir(dir.join("segments")).await.unwrap();
    let mut names = Vec::new();
    while let Ok(Some(e)) = rd.next_entry().await {
        names.push(e.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    names
}

async fn bodies(store: &Store) -> Vec<String> {
    let (mut found, _) = store.search("", 1000, None, None).await.unwrap();
    found.sort_by(|a, b| a.body.cmp(&b.body));
    found.into_iter().map(|r| r.body).collect()
}

async fn write(store: &Store, records: &[(&str, &str)]) {
    for (i, (stream, body)) in records.iter().enumerate() {
        let ts = t0() + chrono::Duration::seconds(i64::try_from(i).unwrap());
        store.append(rec(stream, body, ts)).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
}

#[tokio::test]
async fn without_rules_nothing_is_deleted_however_old() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open_with(tmp.path(), cfg(RetentionConfig::default()))
        .await
        .unwrap();
    write(&store, &[("a", "one"), ("b", "two")]).await;

    let report = store
        .trigger_compaction_at(days_after_partition_end(3650))
        .await
        .unwrap();

    assert_eq!(report.records_expired, 0);
    assert_eq!(bodies(&store).await, ["one", "two"]);
}

#[tokio::test]
async fn a_partition_past_the_default_is_deleted_without_a_replacement() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open_with(tmp.path(), cfg(rules(Some(DAY * 7), &[])))
        .await
        .unwrap();
    write(&store, &[("a", "one"), ("b", "two"), ("a", "three")]).await;
    assert!(!segment_files(tmp.path()).await.is_empty());

    // Six days on: still inside the rule. (The pass may merge; it must not drop.)
    let early = store
        .trigger_compaction_at(days_after_partition_end(6))
        .await
        .unwrap();
    assert_eq!(early.records_expired, 0);
    assert_eq!(bodies(&store).await.len(), 3);

    // Seven days on: gone, and nothing written in its place.
    let late = store
        .trigger_compaction_at(days_after_partition_end(7))
        .await
        .unwrap();
    assert_eq!(late.records_expired, 3);
    assert!(segment_files(tmp.path()).await.is_empty());
    assert!(bodies(&store).await.is_empty());
}

#[tokio::test]
async fn a_stream_rule_drops_only_that_stream() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open_with(tmp.path(), cfg(rules(None, &[("noisy", DAY)])))
        .await
        .unwrap();
    write(
        &store,
        &[
            ("noisy", "n1"),
            ("keep", "k1"),
            ("noisy", "n2"),
            ("keep", "k2"),
        ],
    )
    .await;

    let report = store
        .trigger_compaction_at(days_after_partition_end(2))
        .await
        .unwrap();

    assert_eq!(report.records_expired, 2);
    assert_eq!(bodies(&store).await, ["k1", "k2"]);
    assert_eq!(
        segment_files(tmp.path()).await.len(),
        1,
        "the survivors are rewritten into one segment"
    );
}

#[tokio::test]
async fn a_longer_stream_rule_outlives_the_default() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open_with(
        tmp.path(),
        cfg(rules(Some(DAY * 7), &[("audit", DAY * 90)])),
    )
    .await
    .unwrap();
    write(&store, &[("audit", "a1"), ("web", "w1"), ("web", "w2")]).await;

    // Past the default, short of the audit rule: only the others go.
    let mid = store
        .trigger_compaction_at(days_after_partition_end(10))
        .await
        .unwrap();
    assert_eq!(mid.records_expired, 2);
    assert_eq!(bodies(&store).await, ["a1"]);

    // Past the audit rule too.
    let end = store
        .trigger_compaction_at(days_after_partition_end(90))
        .await
        .unwrap();
    assert_eq!(end.records_expired, 1);
    assert!(segment_files(tmp.path()).await.is_empty());
}

#[tokio::test]
async fn a_pruned_partition_is_not_rewritten_again_on_the_next_pass() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open_with(tmp.path(), cfg(rules(None, &[("noisy", DAY)])))
        .await
        .unwrap();
    write(&store, &[("noisy", "n1"), ("keep", "k1"), ("keep", "k2")]).await;

    store
        .trigger_compaction_at(days_after_partition_end(2))
        .await
        .unwrap();
    let after_first = segment_files(tmp.path()).await;

    // Same rules, later: nothing new has expired, so the file must be the
    // same file — a rewrite would have given it a new sequence number.
    let second = store
        .trigger_compaction_at(days_after_partition_end(3))
        .await
        .unwrap();
    assert!(!second.did_anything(), "{second:?}");
    assert_eq!(segment_files(tmp.path()).await, after_first);
}

#[tokio::test]
async fn a_clean_single_segment_is_left_alone_after_a_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let retention = rules(None, &[("noisy", DAY)]);
    {
        let store = Store::open_with(tmp.path(), cfg(retention.clone()))
            .await
            .unwrap();
        write(&store, &[("noisy", "n1"), ("keep", "k1"), ("keep", "k2")]).await;
        store
            .trigger_compaction_at(days_after_partition_end(2))
            .await
            .unwrap();
        store.close().await.unwrap();
    }
    let before = segment_files(tmp.path()).await;
    assert_eq!(before.len(), 1);

    // A new process has forgotten what it pruned. It has to look at the
    // segment once — but finding nothing to drop, must not rewrite it.
    let store = Store::open_with(tmp.path(), cfg(retention)).await.unwrap();
    let report = store
        .trigger_compaction_at(days_after_partition_end(2))
        .await
        .unwrap();
    assert!(!report.did_anything(), "{report:?}");
    assert_eq!(segment_files(tmp.path()).await, before);
    assert_eq!(bodies(&store).await, ["k1", "k2"]);
}

#[tokio::test]
async fn recent_records_survive_a_short_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open_with(tmp.path(), cfg(rules(Some(DAY), &[])))
        .await
        .unwrap();
    let now = Utc::now();
    store.append(rec("a", "fresh", now)).await.unwrap();
    store
        .append(rec("a", "old", now - chrono::Duration::days(3)))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let report = store.trigger_compaction().await.unwrap();

    assert_eq!(report.records_expired, 1);
    assert_eq!(bodies(&store).await, ["fresh"]);
}

#[tokio::test]
async fn retention_survives_a_restart_with_the_data_it_left() {
    let tmp = tempfile::tempdir().unwrap();
    let retention = rules(Some(DAY * 7), &[("audit", DAY * 90)]);
    {
        let store = Store::open_with(tmp.path(), cfg(retention.clone()))
            .await
            .unwrap();
        write(&store, &[("audit", "a1"), ("web", "w1")]).await;
        store
            .trigger_compaction_at(days_after_partition_end(10))
            .await
            .unwrap();
        store.close().await.unwrap();
    }

    let store = Store::open_with(tmp.path(), cfg(retention)).await.unwrap();
    assert_eq!(bodies(&store).await, ["a1"]);
}
