use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use sift_api::{SearchQuery, SearchResponse};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/search", get(search))
}

async fn search(State(state): State<AppState>, Query(q): Query<SearchQuery>) -> impl IntoResponse {
    match state.store().search(&q.q, q.limit, q.from, q.to).await {
        Ok((records, scanned)) => {
            (StatusCode::OK, Json(SearchResponse { records, scanned })).into_response()
        }
        Err(err) => {
            tracing::error!(%err, "search failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "search failed").into_response()
        }
    }
}
