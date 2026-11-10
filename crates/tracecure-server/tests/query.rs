//! `/v1/query`: `TracecureQL` over HTTP, with labels and paging.

use std::fmt::Write as _;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use tracecure_api::{QueryError, QueryResponse};

/// 30 records one second apart: apps alternate, every third is an error,
/// and `env` is `prod` on even records, `dev` on the rest.
async fn app() -> (tempfile::TempDir, axum::Router) {
    let tmp = tempfile::tempdir().unwrap();
    let state = tracecure_server::AppState::open(tmp.path()).await.unwrap();
    let app = tracecure_server::router(state);

    let mut ndjson = String::new();
    for i in 0..30 {
        let even = i % 2 == 0;
        let app_name = if even { "api" } else { "worker" };
        let level = if i % 3 == 0 { "error" } else { "info" };
        let env = if even { "prod" } else { "dev" };
        writeln!(
            ndjson,
            r#"{{"timestamp":"2026-04-22T12:00:{i:02}Z","host":"edge-1","app":"{app_name}","stream_id":"s","level":"{level}","body":"line {i:02}","labels":{{"env":"{env}"}}}}"#,
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

/// Percent-encode a query-string value.
fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
            out.push(char::from(b));
        } else {
            write!(out, "%{b:02X}").unwrap();
        }
    }
    out
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

async fn page(app: &axum::Router, query: &str, extra: &str) -> QueryResponse {
    let uri = format!("/v1/query?query={}{extra}", enc(query));
    let (status, body) = get(app, &uri).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    serde_json::from_str(&body).unwrap()
}

fn bodies(p: &QueryResponse) -> Vec<&str> {
    p.records.iter().map(|r| r.body.as_str()).collect()
}

#[tokio::test]
async fn a_query_filters_on_columns_labels_and_text_together() {
    let (_tmp, app) = app().await;

    let p = page(&app, r#"{app="api", env="prod"} | level="error""#, "").await;
    assert_eq!(
        bodies(&p),
        ["line 24", "line 18", "line 12", "line 06", "line 00"]
    );
    assert!(p.next_cursor.is_none());

    let p = page(&app, r#"{env!="prod"} | body ~ "1" | body !~ "line 1""#, "").await;
    assert_eq!(bodies(&p), ["line 21", "line 01"]);

    let p = page(&app, "{}", "&limit=3").await;
    assert_eq!(bodies(&p), ["line 29", "line 28", "line 27"]);
    assert_eq!(p.scanned, 30);
}

#[tokio::test]
async fn the_cursor_walks_through_every_match_once() {
    let (_tmp, app) = app().await;
    let query = r#"{env="dev"}"#;

    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let extra = cursor.as_ref().map_or_else(
            || "&limit=4".to_string(),
            |c| format!("&limit=4&cursor={}", enc(c)),
        );
        let p = page(&app, query, &extra).await;
        seen.extend(p.records.iter().map(|r| r.body.clone()));
        pages += 1;
        match p.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
        assert!(pages < 20, "paging does not end");
    }

    let want: Vec<String> = (0..30)
        .rev()
        .filter(|i| i % 2 == 1)
        .map(|i| format!("line {i:02}"))
        .collect();
    assert_eq!(seen, want);
}

#[tokio::test]
async fn the_time_range_bounds_a_query() {
    let (_tmp, app) = app().await;
    let p = page(
        &app,
        "{}",
        "&from=2026-04-22T12:00:10Z&to=2026-04-22T12:00:13Z",
    )
    .await;
    assert_eq!(bodies(&p), ["line 12", "line 11", "line 10"]);
}

#[tokio::test]
async fn a_bad_query_is_a_400_that_says_where() {
    let (_tmp, app) = app().await;

    let (status, body) = get(&app, &format!("/v1/query?query={}", enc("{app=api}"))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let e: QueryError = serde_json::from_str(&body).unwrap();
    assert_eq!(e.at, Some(5));
    assert!(e.error.contains("double quotes"), "{}", e.error);

    let uri = format!("/v1/query?query={}", enc(r#"| level="eror""#));
    let (status, body) = get(&app, &uri).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("unknown level"), "{body}");

    // A cursor the server does not recognise must not quietly restart.
    let (status, body) = get(&app, "/v1/query?query=%7B%7D&cursor=nonsense").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let e: QueryError = serde_json::from_str(&body).unwrap();
    assert!(e.error.contains("cursor") && e.at.is_none(), "{body}");

    // No query at all is the client's mistake too.
    let (status, _) = get(&app, "/v1/query").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
