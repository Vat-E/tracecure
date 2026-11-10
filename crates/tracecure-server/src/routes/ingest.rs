use axum::body::Bytes;
use axum::extract::State;
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use tracecure_api::IngestResponse;
use tracecure_core::Record;

use crate::auth::bearer;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/ingest", post(ingest))
}

/// A 401 that says what was wrong with the request — never anything about
/// the tokens that exist.
fn unauthorized(reason: &'static str) -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, "Bearer")],
        reason,
    )
        .into_response()
}

async fn ingest(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Checked before the body is looked at: an unauthenticated caller gets
    // no information about what a valid batch looks like, and costs nothing
    // to turn away.
    if let Some(tokens) = state.ingest_tokens() {
        let Some(presented) = headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(bearer)
        else {
            return unauthorized("ingest requires a token: Authorization: Bearer tc_...");
        };
        let Some(token) = tokens.verify(presented) else {
            tracing::warn!("ingest rejected: invalid token");
            return unauthorized("invalid or revoked token");
        };
        tracing::debug!(token_id = %token.id, token_name = %token.name, "ingest authorized");
    }

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
