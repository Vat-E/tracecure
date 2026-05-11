//! Backend query helper — wraps /v1/search for windowed queries and
//! backward pagination.

use chrono::{DateTime, Utc};
use gloo_net::http::Request;
use sift_core::Record;

#[derive(Debug)]
pub enum QueryError {
    Http(String),
    Parse(String),
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "http error: {e}"),
            Self::Parse(e) => write!(f, "parse error: {e}"),
        }
    }
}

impl std::error::Error for QueryError {}

/// Fetch records from `/v1/search` within a time window.
///
/// # Errors
///
/// Returns `QueryError::Http` if the request fails or the server returns a non-2xx status.
/// Returns `QueryError::Parse` if the response body cannot be deserialized as JSON.
#[allow(dead_code)]
pub async fn fetch_window(
    q: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<Record>, QueryError> {
    let origin = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_else(|| "http://localhost:7789".to_string());

    let url = format!(
        "{origin}/v1/search?q={}&from={}&to={}&limit={}",
        urlencoded(q),
        from.to_rfc3339(),
        to.to_rfc3339(),
        limit,
    );

    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| QueryError::Http(e.to_string()))?;

    if !resp.ok() {
        return Err(QueryError::Http(format!("HTTP {}", resp.status())));
    }

    resp.json::<Vec<Record>>()
        .await
        .map_err(|e| QueryError::Parse(e.to_string()))
}

fn urlencoded(s: &str) -> String {
    js_sys::encode_uri_component(s)
        .as_string()
        .unwrap_or_default()
}
