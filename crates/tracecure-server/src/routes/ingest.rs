use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use tracecure_api::IngestResponse;
use tracecure_core::Record;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/ingest", post(ingest))
}

async fn ingest(State(state): State<AppState>, body: Bytes) -> impl IntoResponse {
    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let Ok(text) = std::str::from_utf8(&body) else {
        return (StatusCode::BAD_REQUEST, "request body is not valid UTF-8").into_response();
    };
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Record>(line) {
            Ok(record) => match state.store().append(record).await {
                Ok(()) => accepted += 1,
                Err(err) => {
                    // Mid-batch failure: earlier records in this batch are
                    // already committed to disk, but we discard their count
                    // and return a plain 500. Revisit when the store offers
                    // batch transactional semantics.
                    tracing::error!(%err, "append failed");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "append failed").into_response();
                }
            },
            Err(err) => {
                tracing::debug!(%err, "rejecting malformed ingest line");
                rejected += 1;
            }
        }
    }
    (StatusCode::OK, Json(IngestResponse { accepted, rejected })).into_response()
}
