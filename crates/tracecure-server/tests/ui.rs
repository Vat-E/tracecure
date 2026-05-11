use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn serves_index_html_from_dist_when_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let dist = tmp.path().join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), b"<!doctype html>hello").unwrap();

    let state = tracecure_server::AppState::open(tmp.path()).await.unwrap();
    let app = tracecure_server::router_with_ui(state, Some(dist));

    let res = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    assert_eq!(&body[..], b"<!doctype html>hello");
}

#[tokio::test]
async fn root_returns_404_when_ui_not_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tracecure_server::AppState::open(tmp.path()).await.unwrap();
    let app = tracecure_server::router_with_ui(state, None);

    let res = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
