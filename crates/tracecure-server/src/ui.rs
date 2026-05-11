use std::path::PathBuf;

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

use crate::state::AppState;

/// Serve the pre-built `dist/` directory at `/`, with SPA-style fallback to
/// `index.html` for unmatched paths (no SPA routing in M0, but this keeps the
/// behaviour consistent when we add client-side routes in M2+).
pub fn router(dist: PathBuf) -> Router<AppState> {
    let index = dist.join("index.html");
    let service = ServeDir::new(dist).not_found_service(ServeFile::new(index));
    Router::new().fallback_service(service)
}
