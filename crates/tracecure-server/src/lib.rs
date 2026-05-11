//! sift-server — library surface used by integration tests and the binary.

use std::path::PathBuf;

use axum::Router;

pub mod routes;
pub mod state;
pub mod ui;

pub use sift_storage::Store;
pub use state::AppState;

/// Router with API routes only. Root path returns 404. Used by tests that
/// don't care about UI serving.
pub fn router(state: AppState) -> Router {
    router_with_ui(state, None)
}

/// Router with optional static UI serving mounted at `/`.
pub fn router_with_ui(state: AppState, ui_dist: Option<PathBuf>) -> Router {
    let mut app = Router::new().merge(routes::all());
    if let Some(dist) = ui_dist {
        app = app.merge(ui::router(dist));
    }
    app.with_state(state)
}
