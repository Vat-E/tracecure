//! `GET /v1/query` — search with `TracecureQL`, a page at a time.

use axum::extract::{Query as UrlQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use tracecure_api::{QueryError, QueryRequest, QueryResponse};
use tracecure_query::Query;
use tracecure_storage::SearchFilter;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/query", get(query))
}

fn bad_request(error: impl Into<String>, at: Option<usize>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(QueryError {
            error: error.into(),
            at,
        }),
    )
        .into_response()
}

/// A cursor is "everything older than this instant": `t` and the instant in
/// microseconds since the epoch. Clients are told it is opaque, so the
/// encoding can grow (a shard, a sequence number) without breaking them.
fn encode_cursor(before: DateTime<Utc>) -> String {
    format!("t{}", before.timestamp_micros())
}

fn decode_cursor(cursor: &str) -> Option<DateTime<Utc>> {
    let micros = cursor.strip_prefix('t')?.parse::<i64>().ok()?;
    DateTime::<Utc>::from_timestamp_micros(micros)
}

async fn query(State(state): State<AppState>, UrlQuery(req): UrlQuery<QueryRequest>) -> Response {
    let parsed = match Query::parse(&req.query) {
        Ok(q) => q,
        Err(e) => return bad_request(e.message, Some(e.at)),
    };
    let before = match req.cursor.as_deref() {
        None => None,
        Some(c) => match decode_cursor(c) {
            Some(at) => Some(at),
            // Not "start from the top": that would silently repeat page one.
            None => return bad_request("unrecognised cursor", None),
        },
    };

    match state
        .store()
        .search_page(
            &SearchFilter::query(parsed),
            req.limit,
            req.from,
            req.to,
            before,
        )
        .await
    {
        Ok(page) => Json(QueryResponse {
            records: page.records,
            scanned: page.scanned,
            next_cursor: page.next_before.map(encode_cursor),
        })
        .into_response(),
        Err(err) => {
            tracing::error!(%err, "query failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "query failed").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip_and_reject_garbage() {
        let at = DateTime::<Utc>::from_timestamp_micros(1_745_409_600_123_456).unwrap();
        assert_eq!(decode_cursor(&encode_cursor(at)), Some(at));
        for bad in ["", "t", "tx", "1745409600", "t12.5"] {
            assert_eq!(decode_cursor(bad), None, "{bad}");
        }
    }
}
