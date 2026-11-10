//! tracecure-server — library surface used by integration tests and the binary.

use std::path::PathBuf;

use axum::Router;

pub mod auth;
pub mod oidc;
pub mod routes;
pub mod session;
pub mod state;
pub mod ui;

pub use state::AppState;
pub use tracecure_storage::Store;

/// Router with API routes only. Root path returns 404. Used by tests that
/// don't care about UI serving.
pub fn router(state: AppState) -> Router {
    router_with_ui(state, None)
}

/// Router with optional static UI serving mounted at `/`.
///
/// Routes fall into two groups. Reading logs and the UI sit behind
/// `require_session`, which does nothing unless sign-in is configured.
/// Health, ingest and the sign-in routes themselves do not: health is for
/// probes, ingest has its own tokens, and the sign-in routes are how a
/// session is obtained in the first place.
pub fn router_with_ui(state: AppState, ui_dist: Option<PathBuf>) -> Router {
    let mut protected = routes::reading();
    if let Some(dist) = ui_dist {
        protected = protected.merge(ui::router(dist));
    }
    let protected = protected.layer(axum::middleware::from_fn_with_state(
        state.clone(),
        routes::auth::require_session,
    ));
    Router::new()
        .merge(routes::open())
        .merge(protected)
        .with_state(state)
}
