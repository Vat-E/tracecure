//! sift-server — library surface used by integration tests and the binary.

pub mod routes;
pub mod state;

pub use state::AppState;

use axum::Router;

/// Build the top-level router. The same function is used by the binary entry
/// point in `main.rs` and by integration tests.
pub fn router(state: AppState) -> Router {
    Router::new().merge(routes::all()).with_state(state)
}
