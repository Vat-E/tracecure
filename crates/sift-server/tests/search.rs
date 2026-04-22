use std::fmt::Write as _;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sift_api::{IngestResponse, SearchResponse};
use tower::ServiceExt;

async fn ingest_batch(app: &axum::Router, ndjson: &str) {
    let res = app
        .clone()
        .oneshot(
            Request::post("/v1/ingest")
                .header("content-type", "application/x-ndjson")
                .body(Body::from(ndjson.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    let _: IngestResponse = serde_json::from_slice(&body).unwrap();
}

#[tokio::test]
async fn search_returns_matching_records_only() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("d.ndjson"))
        .await
        .unwrap();
    let app = sift_server::router(state);

    ingest_batch(
        &app,
        concat!(
            r#"{"timestamp":"2026-04-22T12:00:00Z","host":"h","app":"a","stream_id":"s","body":"hello world"}"#,
            "\n",
            r#"{"timestamp":"2026-04-22T12:00:01Z","host":"h","app":"a","stream_id":"s","body":"goodbye"}"#,
            "\n",
        ),
    )
    .await;

    let res = app
        .oneshot(
            Request::get("/v1/search?q=world&limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 65_536).await.unwrap();
    let resp: SearchResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.records.len(), 1);
    assert_eq!(resp.records[0].body, "hello world");
    assert_eq!(resp.scanned, 2);
}

#[tokio::test]
async fn search_empty_query_returns_all_records() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("d.ndjson"))
        .await
        .unwrap();
    let app = sift_server::router(state);

    ingest_batch(
        &app,
        concat!(
            r#"{"timestamp":"2026-04-22T12:00:00Z","host":"h","app":"a","stream_id":"s","body":"one"}"#,
            "\n",
            r#"{"timestamp":"2026-04-22T12:00:01Z","host":"h","app":"a","stream_id":"s","body":"two"}"#,
            "\n",
        ),
    )
    .await;

    let res = app
        .oneshot(Request::get("/v1/search").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 65_536).await.unwrap();
    let resp: SearchResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.records.len(), 2);
}

#[tokio::test]
async fn search_honors_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("d.ndjson"))
        .await
        .unwrap();
    let app = sift_server::router(state);

    let mut ndjson = String::new();
    for i in 0..5 {
        let _ = write!(
            ndjson,
            r#"{{"timestamp":"2026-04-22T12:00:0{i}Z","host":"h","app":"a","stream_id":"s","body":"n{i}"}}"#
        );
        ndjson.push('\n');
    }
    ingest_batch(&app, &ndjson).await;

    let res = app
        .oneshot(
            Request::get("/v1/search?limit=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(res.into_body(), 65_536).await.unwrap();
    let resp: SearchResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.records.len(), 2);
    assert_eq!(resp.scanned, 5);
    // `search()` returns the MOST RECENT `limit` records, so n3 and n4.
    assert_eq!(resp.records[0].body, "n3");
    assert_eq!(resp.records[1].body, "n4");
}
