use axum::Router;

use crate::state::AppState;

pub mod health;
pub mod ingest;

pub fn all() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(ingest::router())
}
