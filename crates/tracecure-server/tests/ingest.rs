use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use tracecure_api::{IngestResponse, SearchResponse};

fn sample_records() -> String {
    // Two valid records + one malformed line.
    let a = r#"{"timestamp":"2026-04-22T12:00:00Z","host":"h","app":"a","stream_id":"s","body":"hello"}"#;
    let b = r#"{"timestamp":"2026-04-22T12:00:01Z","host":"h","app":"a","stream_id":"s","body":"world"}"#;
    let bad = r#"{"not":"a record"}"#;
    format!("{a}\n{b}\n{bad}\n")
}

#[tokio::test]
async fn ingest_appends_valid_records_and_reports_rejects() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tracecure_server::AppState::open(tmp.path()).await.unwrap();
    let app = tracecure_server::router(state);

    let res = app
        .clone()
        .oneshot(
            Request::post("/v1/ingest")
                .header("content-type", "application/x-ndjson")
                .body(Body::from(sample_records()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    let resp: IngestResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.accepted, 2);
    assert_eq!(resp.rejected, 1);

    // Confirm the two records are queryable via the search API.
    let res = app
        .oneshot(
            Request::get("/v1/search?limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 65_536).await.unwrap();
    let search: SearchResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(search.records.len(), 2);
    // Bodies present (order may vary by timestamp; collect and sort).
    let mut bodies: Vec<&str> = search.records.iter().map(|r| r.body.as_str()).collect();
    bodies.sort_unstable();
    assert_eq!(bodies, ["hello", "world"]);
}

#[tokio::test]
async fn ingest_rejects_non_utf8_body_with_400() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tracecure_server::AppState::open(tmp.path()).await.unwrap();
    let app = tracecure_server::router(state);

    let res = app
        .oneshot(
            Request::post("/v1/ingest")
                .header("content-type", "application/x-ndjson")
                .body(Body::from(vec![0xFF, 0xFE, 0x00, 0xFF]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn ingest_rejects_empty_body_with_200_and_zero_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tracecure_server::AppState::open(tmp.path()).await.unwrap();
    let app = tracecure_server::router(state);

    let res = app
        .oneshot(
            Request::post("/v1/ingest")
                .header("content-type", "application/x-ndjson")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
    let resp: IngestResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        resp,
        IngestResponse {
            accepted: 0,
            rejected: 0
        }
    );
}
