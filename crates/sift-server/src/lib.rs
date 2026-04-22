//! sift-server — library surface used by integration tests and the binary.

pub mod routes;
pub mod state;
pub mod store;

pub use state::AppState;
pub use store::Store;

use axum::Router;

pub fn router(state: AppState) -> Router {
    Router::new().merge(routes::all()).with_state(state)
}
