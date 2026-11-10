use axum::Router;

use crate::state::AppState;

pub mod auth;
pub mod health;
pub mod ingest;
pub mod query;
pub mod search;
pub mod tail;

/// Routes that never require a session: the health probe, ingest (guarded
/// by its own tokens when those are on) and sign-in itself.
pub fn open() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(ingest::router())
        .merge(auth::router())
}

/// Routes that read logs. Behind a session when sign-in is configured.
pub fn reading() -> Router<AppState> {
    Router::new()
        .merge(search::router())
        .merge(query::router())
        .merge(tail::router())
}
