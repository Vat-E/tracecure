use axum::Router;

use crate::state::AppState;

pub mod health;

pub fn all() -> Router<AppState> {
    Router::new().merge(health::router())
}
