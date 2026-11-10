//! `/v1/ingest` with tokens required. What is being protected is a public
//! write endpoint, so the tests are mostly about what is *refused*, and that
//! a refused request leaves nothing behind.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use tracecure_api::{IngestResponse, SearchResponse};
use tracecure_server::auth::TokenStore;

const LINE: &str =
    r#"{"timestamp":"2026-04-22T12:00:00Z","host":"h","app":"a","stream_id":"s","body":"hello"}"#;

async fn guarded() -> (tempfile::TempDir, axum::Router) {
    let tmp = tempfile::tempdir().unwrap();
    let state = tracecure_server::AppState::open(tmp.path())
        .await
        .unwrap()
        .require_ingest_token(TokenStore::open(tmp.path()));
    (tmp, tracecure_server::router(state))
}

async fn post(app: &axum::Router, auth: Option<&str>, body: &str) -> (StatusCode, String, bool) {
    let mut req = Request::post("/v1/ingest").header("content-type", "application/x-ndjson");
    if let Some(value) = auth {
        req = req.header("authorization", value);
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let challenged = res.headers().contains_key("www-authenticate");
    let bytes = axum::body::to_bytes(res.into_body(), 65_536).await.unwrap();
    (
        status,
        String::from_utf8(bytes.to_vec()).unwrap(),
        challenged,
    )
}

async fn stored(app: &axum::Router) -> usize {
    let res = app
        .clone()
        .oneshot(
            Request::get("/v1/search?limit=100")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(res.into_body(), 65_536).await.unwrap();
    serde_json::from_slice::<SearchResponse>(&bytes)
        .unwrap()
        .records
        .len()
}

#[tokio::test]
async fn without_a_token_ingest_is_refused_and_nothing_is_stored() {
    let (_tmp, app) = guarded().await;
    let (status, body, challenged) = post(&app, None, &format!("{LINE}\n")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(challenged, "a 401 must say how to authenticate");
    assert!(body.contains("Bearer"), "{body}");
    assert_eq!(stored(&app).await, 0);
}

#[tokio::test]
async fn a_valid_token_is_accepted() {
    let (tmp, app) = guarded().await;
    let (_, token) = TokenStore::open(tmp.path()).create("billing").unwrap();

    let (status, body, _) =
        post(&app, Some(&format!("Bearer {token}")), &format!("{LINE}\n")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let resp: IngestResponse = serde_json::from_str(&body).unwrap();
    assert_eq!((resp.accepted, resp.rejected), (1, 0));
    assert_eq!(stored(&app).await, 1);
}

#[tokio::test]
async fn wrong_malformed_and_wrong_scheme_tokens_are_refused() {
    let (tmp, app) = guarded().await;
    let (_, token) = TokenStore::open(tmp.path()).create("billing").unwrap();
    let mut tampered = token.clone();
    tampered.pop();
    tampered.push(if token.ends_with('0') { '1' } else { '0' });

    let attempts = [
        format!("Bearer {tampered}"),
        "Bearer tc_00000000_0000".to_string(),
        "Bearer not-a-token".to_string(),
        format!("Basic {token}"),
        token.clone(), // no scheme
        "Bearer".to_string(),
    ];
    for auth in attempts {
        let (status, _, challenged) = post(&app, Some(&auth), &format!("{LINE}\n")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{auth:?}");
        assert!(challenged, "{auth:?}");
    }
    assert_eq!(stored(&app).await, 0);
}

/// Revocation is the whole reason to have one token per project. It has to
/// work on a server that is already running.
#[tokio::test]
async fn a_token_revoked_while_the_server_runs_stops_working() {
    let (tmp, app) = guarded().await;
    let cli = TokenStore::open(tmp.path());
    let (record, token) = cli.create("billing").unwrap();
    let auth = format!("Bearer {token}");

    assert_eq!(
        post(&app, Some(&auth), &format!("{LINE}\n")).await.0,
        StatusCode::OK
    );
    assert!(cli.revoke(&record.id).unwrap());
    assert_eq!(
        post(&app, Some(&auth), &format!("{LINE}\n")).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(stored(&app).await, 1, "only the first batch landed");
}

#[tokio::test]
async fn a_token_created_while_the_server_runs_starts_working() {
    let (tmp, app) = guarded().await;
    // The server has already looked once and found no tokens.
    assert_eq!(post(&app, None, "").await.0, StatusCode::UNAUTHORIZED);

    let (_, token) = TokenStore::open(tmp.path()).create("late").unwrap();
    let (status, _, _) = post(&app, Some(&format!("Bearer {token}")), &format!("{LINE}\n")).await;
    assert_eq!(status, StatusCode::OK);
}

/// The check comes before the body is read: an unauthenticated caller learns
/// nothing about what a valid batch looks like.
#[tokio::test]
async fn the_refusal_does_not_depend_on_the_body() {
    let (_tmp, app) = guarded().await;
    let valid = post(&app, None, &format!("{LINE}\n")).await;
    let garbage = post(&app, None, "\u{0}\u{1} not json at all").await;
    assert_eq!(valid.0, StatusCode::UNAUTHORIZED);
    assert_eq!(valid.0, garbage.0);
    assert_eq!(valid.1, garbage.1);
}

/// Tokens guard writing only. Reading stays as it was.
#[tokio::test]
async fn search_and_health_are_not_guarded() {
    let (_tmp, app) = guarded().await;
    for uri in ["/health", "/v1/search?limit=1"] {
        let res = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
    }
}

/// And when auth is off — the default — a token is neither needed nor a
/// problem: an existing shipper that starts sending one must keep working.
#[tokio::test]
async fn with_auth_off_ingest_is_open_and_ignores_any_token() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tracecure_server::AppState::open(tmp.path()).await.unwrap();
    let app = tracecure_server::router(state);

    assert_eq!(
        post(&app, None, &format!("{LINE}\n")).await.0,
        StatusCode::OK
    );
    assert_eq!(
        post(&app, Some("Bearer whatever"), &format!("{LINE}\n"))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(stored(&app).await, 2);
}
