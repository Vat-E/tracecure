//! tracecure-api — REST + WebSocket wire types shared by server, CLI, and UI.

mod ingest;
mod query;
mod search;
mod tail;

pub use ingest::IngestResponse;
pub use query::{QueryError, QueryRequest, QueryResponse};
pub use search::{SearchQuery, SearchResponse};
pub use tail::TailFrame;
