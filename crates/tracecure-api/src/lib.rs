//! tracecure-api — REST + WebSocket wire types shared by server, CLI, and UI.

mod ingest;
mod search;
mod tail;

pub use ingest::IngestResponse;
pub use search::{SearchQuery, SearchResponse};
pub use tail::TailFrame;
