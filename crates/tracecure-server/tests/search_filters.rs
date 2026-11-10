//! `/v1/search` with the structured filters: `level`, `app`, `host`,
//! `stream_id`. The point of doing these on the server is that `limit`
//! applies *after* them — the tests that matter most here are the ones that
//! would pass on a client-side filter only by luck.

use std::fmt::Write as _;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use tracecure_api::SearchResponse;

async fn app_with(records: &[(&str, &str, &str, &str, &str)]) -> (tempfile::TempDir, axum::Router) {
    let tmp = tempfile::tempdir().unwrap();
    let state = tracecure_server::AppState::open(tmp.path()).await.unwrap();
    let app = tracecure_server::router(state);

    let mut ndjson = String::new();
    for (i, (app_name, host, stream, level, body)) in records.iter().enumerate() {
        writeln!(
            ndjson,
            r#"{{"timestamp":"2026-04-22T12:{:02}:{:02}Z","host":"{host}","app":"{app_name}","stream_id":"{stream}","level":"{level}","body":"{body}"}}"#,
            i / 60,
            i % 60,
        )
        .unwrap();
    }
    let res = app
        .clone()
        .oneshot(
            Request::post("/v1/ingest")
                .header("content-type", "application/x-ndjson")
                .body(Body::from(ndjson))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    (tmp, app)
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let res = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let body = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

async fn bodies(app: &axum::Router, uri: &str) -> Vec<String> {
    let (status, body) = get(app, uri).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    let resp: SearchResponse = serde_json::from_str(&body).unwrap();
    resp.records.into_iter().map(|r| r.body).collect()
}

const MIXED: &[(&str, &str, &str, &str, &str)] = &[
    ("billing", "edge-1", "billing-prod", "info", "b-info"),
    ("billing", "edge-2", "billing-prod", "error", "b-error"),
    ("web", "edge-1", "web-prod", "error", "w-error"),
    ("web", "edge-1", "web-dev", "warn", "w-warn"),
    ("payments-api", "edge-3", "pay-prod", "fatal", "p-fatal"),
];

#[tokio::test]
async fn level_keeps_only_the_named_levels() {
    let (_tmp, app) = app_with(MIXED).await;
    assert_eq!(
        bodies(&app, "/v1/search?level=error").await,
        ["w-error", "b-error"]
    );
    assert_eq!(
        bodies(&app, "/v1/search?level=error,fatal").await,
        ["p-fatal", "w-error", "b-error"]
    );
}

#[tokio::test]
async fn app_host_and_stream_filter_and_combine() {
    let (_tmp, app) = app_with(MIXED).await;
    assert_eq!(
        bodies(&app, "/v1/search?app=billing").await,
        ["b-error", "b-info"]
    );
    assert_eq!(
        bodies(&app, "/v1/search?host=edge-1").await,
        ["w-warn", "w-error", "b-info"]
    );
    assert_eq!(
        bodies(&app, "/v1/search?stream_id=web-dev").await,
        ["w-warn"]
    );
    // All of them at once, with text: a conjunction.
    assert_eq!(
        bodies(&app, "/v1/search?app=web&host=edge-1&level=error&q=error").await,
        ["w-error"]
    );
    assert!(bodies(&app, "/v1/search?app=web&level=fatal")
        .await
        .is_empty());
}

#[tokio::test]
async fn a_trailing_star_is_a_prefix_match() {
    let (_tmp, app) = app_with(MIXED).await;
    assert_eq!(bodies(&app, "/v1/search?app=pay*").await, ["p-fatal"]);
    assert_eq!(
        bodies(&app, "/v1/search?stream_id=web-*").await,
        ["w-warn", "w-error"]
    );
}

/// The reason these filters live on the server. Ten errors are buried under
/// two hundred newer info records; a client that fetched the newest 20 and
/// filtered them itself would find none.
#[tokio::test]
async fn limit_applies_after_the_filter_not_before() {
    let mut records: Vec<(&str, &str, &str, &str, &str)> = Vec::new();
    for _ in 0..10 {
        records.push(("a", "h", "s", "error", "old error"));
    }
    for _ in 0..200 {
        records.push(("a", "h", "s", "info", "newer noise"));
    }
    let (_tmp, app) = app_with(&records).await;

    let newest = bodies(&app, "/v1/search?limit=20").await;
    assert!(newest.iter().all(|b| b == "newer noise"));

    let errors = bodies(&app, "/v1/search?level=error&limit=20").await;
    assert_eq!(errors.len(), 10);
    assert!(errors.iter().all(|b| b == "old error"));
}

#[tokio::test]
async fn an_unknown_level_is_a_400_that_names_it() {
    let (_tmp, app) = app_with(MIXED).await;
    let (status, body) = get(&app, "/v1/search?level=error,loud").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("loud"), "{body}");
}

#[tokio::test]
async fn empty_filter_values_mean_any() {
    let (_tmp, app) = app_with(MIXED).await;
    assert_eq!(
        bodies(&app, "/v1/search?level=&app=&host=&stream_id=")
            .await
            .len(),
        MIXED.len()
    );
}

/// The filters must behave the same once records have left the memtable for
/// a segment — the segment scan tests them on columns, a different code path
/// from the in-memory one.
#[tokio::test]
async fn filters_apply_to_flushed_segments_too() {
    use std::time::Duration;
    use tracecure_storage::{memtable::MemtableConfig, StoreConfig};

    let tmp = tempfile::tempdir().unwrap();
    let config = StoreConfig {
        memtable: MemtableConfig {
            // Tiny, so every append flushes a segment.
            size_bytes: 64,
            age: Duration::from_secs(3600),
        },
        ..StoreConfig::default()
    };
    let state = tracecure_server::AppState::open_with(tmp.path(), config)
        .await
        .unwrap();
    let app = tracecure_server::router(state);

    let mut ndjson = String::new();
    for (i, (app_name, host, stream, level, body)) in MIXED.iter().enumerate() {
        writeln!(
            ndjson,
            r#"{{"timestamp":"2026-04-22T12:00:{i:02}Z","host":"{host}","app":"{app_name}","stream_id":"{stream}","level":"{level}","body":"{body}"}}"#,
        )
        .unwrap();
    }
    let res = app
        .clone()
        .oneshot(
            Request::post("/v1/ingest")
                .header("content-type", "application/x-ndjson")
                .body(Body::from(ndjson))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(200)).await;

    let segments = std::fs::read_dir(tmp.path().join("segments"))
        .unwrap()
        .count();
    assert!(segments >= 1, "expected flushed segments, found {segments}");

    assert_eq!(
        bodies(&app, "/v1/search?level=error,fatal&host=edge-*").await,
        ["p-fatal", "w-error", "b-error"]
    );
    assert_eq!(
        bodies(&app, "/v1/search?app=web&level=warn").await,
        ["w-warn"]
    );
}
