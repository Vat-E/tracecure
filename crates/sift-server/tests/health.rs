use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_200_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("data.ndjson"))
        .await
        .unwrap();
    let app = sift_server::router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], b"ok");
}
