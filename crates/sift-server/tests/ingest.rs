use axum::body::Body;
use axum::http::{Request, StatusCode};
use sift_api::IngestResponse;
use tower::ServiceExt;

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
    let data = tmp.path().join("data.ndjson");
    let state = sift_server::AppState::open(&data).await.unwrap();
    let app = sift_server::router(state);

    let res = app
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

    let written = std::fs::read_to_string(&data).unwrap();
    let lines: Vec<&str> = written.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"body\":\"hello\""));
    assert!(lines[1].contains("\"body\":\"world\""));
}

#[tokio::test]
async fn ingest_rejects_non_utf8_body_with_400() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("d.ndjson"))
        .await
        .unwrap();
    let app = sift_server::router(state);

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
    let state = sift_server::AppState::open(tmp.path().join("d.ndjson"))
        .await
        .unwrap();
    let app = sift_server::router(state);

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
    assert_eq!(resp, IngestResponse { accepted: 0, rejected: 0 });
}
