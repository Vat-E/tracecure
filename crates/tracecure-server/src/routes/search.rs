use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use tracecure_api::{SearchQuery, SearchResponse};
use tracecure_storage::SearchFilter;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/search", get(search))
}

async fn search(State(state): State<AppState>, Query(q): Query<SearchQuery>) -> impl IntoResponse {
    let filter = match SearchFilter::from_params(
        &q.q,
        q.level.as_deref(),
        q.app.as_deref(),
        q.host.as_deref(),
        q.stream_id.as_deref(),
    ) {
        Ok(f) => f,
        // A filter the server cannot read is the client's error, and saying
        // which one is the difference between a fix and a guess.
        Err(msg) => return (StatusCode::BAD_REQUEST, msg).into_response(),
    };
    match state
        .store()
        .search_filtered(&filter, q.limit, q.from, q.to)
        .await
    {
        Ok((records, scanned)) => {
            (StatusCode::OK, Json(SearchResponse { records, scanned })).into_response()
        }
        Err(err) => {
            tracing::error!(%err, "search failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "search failed").into_response()
        }
    }
}
