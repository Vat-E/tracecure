//! Backend query helper — wraps /v1/search for windowed queries and
//! backward pagination.
//!
//! Two things here are load-bearing and easy to get wrong, so both are
//! split into pure functions with host-side tests:
//!
//! 1. **Timestamp encoding.** `DateTime::to_rfc3339` emits a `+00:00`
//!    offset. Interpolated raw into a query string, the `+` decodes to a
//!    space under `application/x-www-form-urlencoded`, and axum's
//!    `Query<SearchQuery>` rejects the request with 400 before the handler
//!    ever runs. We emit the `Z` form and percent-encode it anyway.
//! 2. **Response envelope.** `/v1/search` returns
//!    `SearchResponse { records, scanned }`, not a bare array.

use chrono::{DateTime, SecondsFormat, Utc};
use gloo_net::http::Request;
use tracecure_core::Record;

use crate::filter::ServerFilter;

#[derive(Debug)]
pub enum QueryError {
    Http(String),
    Parse(String),
    /// The server wants a sign-in first (`401`). Not a failure to report:
    /// the caller should go and sign in.
    Unauthorized,
    /// The query box holds a query that does not parse. Nothing was sent.
    Query(String),
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "http error: {e}"),
            Self::Parse(e) => write!(f, "parse error: {e}"),
            Self::Unauthorized => write!(f, "not signed in"),
            Self::Query(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for QueryError {}

/// Build the `/v1/search` URL for a windowed query.
///
/// Timestamps are emitted as RFC3339 with a `Z` suffix and percent-encoded,
/// so no `+` ever reaches the wire.
///
/// A constraint the filter does not set is left out of the URL, so a request
/// with no filters is byte-for-byte what it was before the server had any.
#[must_use]
pub fn search_url(
    origin: &str,
    filter: &ServerFilter,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    limit: usize,
) -> String {
    let mut url = format!(
        "{origin}/v1/search?q={}&from={}&to={}",
        urlencoded(&filter.text),
        urlencoded(&from.to_rfc3339_opts(SecondsFormat::Millis, true)),
        urlencoded(&to.to_rfc3339_opts(SecondsFormat::Millis, true)),
    );
    let lists = [
        ("level", filter.level_param()),
        ("app", joined(&filter.apps)),
        ("host", joined(&filter.hosts)),
        ("stream_id", joined(&filter.streams)),
    ];
    for (key, value) in lists {
        if let Some(value) = value {
            url.push_str(&format!("&{key}={}", urlencoded(&value)));
        }
    }
    url.push_str(&format!("&limit={limit}"));
    url
}

/// Build the `/v1/query` URL for a `TracecureQL` query over a window.
#[must_use]
pub fn query_url(
    origin: &str,
    query: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    limit: usize,
) -> String {
    format!(
        "{origin}/v1/query?query={}&from={}&to={}&limit={limit}",
        urlencoded(query),
        urlencoded(&from.to_rfc3339_opts(SecondsFormat::Millis, true)),
        urlencoded(&to.to_rfc3339_opts(SecondsFormat::Millis, true)),
    )
}

/// Parse a `/v1/query` response body into its records.
///
/// # Errors
///
/// Returns `QueryError::Parse` if the body is not a `QueryResponse`.
pub fn parse_query_body(body: &str) -> Result<Vec<Record>, QueryError> {
    serde_json::from_str::<tracecure_api::QueryResponse>(body)
        .map(|r| r.records)
        .map_err(|e| QueryError::Parse(e.to_string()))
}

fn joined(values: &[String]) -> Option<String> {
    (!values.is_empty()).then(|| values.join(","))
}

/// Parse a `/v1/search` response body into its records.
///
/// # Errors
///
/// Returns `QueryError::Parse` if the body is not a `SearchResponse`.
pub fn parse_search_body(body: &str) -> Result<Vec<Record>, QueryError> {
    serde_json::from_str::<tracecure_api::SearchResponse>(body)
        .map(|r| r.records)
        .map_err(|e| QueryError::Parse(e.to_string()))
}

/// Fetch records from `/v1/search` within a time window.
///
/// Returns records **newest-first**, as the server orders them.
///
/// # Errors
///
/// Returns `QueryError::Http` if the request fails or the server returns a non-2xx status.
/// Returns `QueryError::Parse` if the response body cannot be deserialized.
pub async fn fetch_window(
    filter: &ServerFilter,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<Record>, QueryError> {
    // Nothing can match (every level off): the answer is known, and an
    // empty `level=` would read to the server as "any level".
    if filter.matches_nothing() {
        return Ok(Vec::new());
    }

    let origin = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_else(|| "http://localhost:7789".to_string());

    // A query goes to `/v1/query`; the plain filter syntax to `/v1/search`.
    let ql = match filter.query_text() {
        Some(Err(e)) => return Err(QueryError::Query(e)),
        Some(Ok(text)) => Some(text),
        None => None,
    };
    let url = match &ql {
        Some(text) => query_url(&origin, text, from, to, limit),
        None => search_url(&origin, filter, from, to, limit),
    };

    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| QueryError::Http(e.to_string()))?;

    if resp.status() == 401 {
        return Err(QueryError::Unauthorized);
    }
    if !resp.ok() {
        return Err(QueryError::Http(format!("HTTP {}", resp.status())));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| QueryError::Http(e.to_string()))?;

    if ql.is_some() {
        parse_query_body(&body)
    } else {
        parse_search_body(&body)
    }
}

#[cfg(target_arch = "wasm32")]
fn urlencoded(s: &str) -> String {
    js_sys::encode_uri_component(s)
        .as_string()
        .unwrap_or_default()
}

/// Host-side equivalent of `encodeURIComponent` so `search_url` is testable
/// off-wasm. Encodes everything outside the unreserved set plus the small
/// set `encodeURIComponent` deliberately leaves alone.
#[cfg(not(target_arch = "wasm32"))]
fn urlencoded(s: &str) -> String {
    const KEEP: &[u8] = b"-_.!~*'()";
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || KEEP.contains(b) {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::BTreeMap;
    use tracecure_core::{Level, Record};

    fn rec() -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 7, 19, 9, 0, 0).unwrap(),
            host: "build-host".into(),
            app: "example_app".into(),
            stream_id: "example_app".into(),
            level: Level::Warn,
            body: "client error response".into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    /// Regression: `to_rfc3339()` emits `+00:00`; the raw `+` decodes to a
    /// space in a form-encoded query string and axum answers 400 before the
    /// handler runs. Verified against the live server.
    #[test]
    fn search_url_never_emits_a_raw_plus() {
        let from = Utc.with_ymd_and_hms(2026, 7, 19, 9, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 7, 19, 12, 0, 0).unwrap();
        let url = search_url("http://x", &ServerFilter::default(), from, to, 500);
        assert!(
            !url.contains('+'),
            "a raw '+' in the query string makes axum reject with 400: {url}"
        );
    }

    #[test]
    fn search_url_encodes_timestamps_as_utc_z_form() {
        let from = Utc.with_ymd_and_hms(2026, 7, 19, 9, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 7, 19, 12, 0, 0).unwrap();
        let url = search_url("http://x", &ServerFilter::default(), from, to, 500);
        // 'Z' is unreserved-ish for encodeURIComponent (alphanumeric), so it
        // survives verbatim; the colons are percent-encoded.
        assert!(url.contains("2026-07-19T09%3A00%3A00.000Z"), "got: {url}");
        assert!(url.contains("2026-07-19T12%3A00%3A00.000Z"), "got: {url}");
    }

    #[test]
    fn search_url_encodes_the_query_term() {
        let t = Utc.with_ymd_and_hms(2026, 7, 19, 9, 0, 0).unwrap();
        let url = search_url("http://x", &"a b&c=d".into(), t, t, 10);
        assert!(url.contains("q=a%20b%26c%3Dd"), "got: {url}");
        assert!(url.ends_with("&limit=10"), "got: {url}");
    }

    #[test]
    fn search_url_without_filters_has_no_filter_params() {
        let t = Utc.with_ymd_and_hms(2026, 7, 19, 9, 0, 0).unwrap();
        let url = search_url("http://x", &ServerFilter::default(), t, t, 10);
        for key in ["level=", "app=", "host=", "stream_id="] {
            assert!(!url.contains(key), "{key} in {url}");
        }
    }

    #[test]
    fn search_url_carries_every_set_filter_encoded() {
        use crate::filter::Filter;
        let t = Utc.with_ymd_and_hms(2026, 7, 19, 9, 0, 0).unwrap();
        let filter = ServerFilter::new(
            &Filter::parse("app:pay*,web host:edge-1 stream:docker/api boom"),
            &[Level::Error, Level::Fatal],
        );
        let url = search_url("http://x", &filter, t, t, 10);
        assert!(url.contains("q=boom"), "got: {url}");
        assert!(url.contains("&level=error%2Cfatal"), "got: {url}");
        assert!(url.contains("&app=pay*%2Cweb"), "got: {url}");
        assert!(url.contains("&host=edge-1"), "got: {url}");
        assert!(url.contains("&stream_id=docker%2Fapi"), "got: {url}");
        assert!(url.ends_with("&limit=10"), "got: {url}");
    }

    /// Cross-crate contract test: fails the moment either side changes the
    /// wire shape of `/v1/search`.
    #[test]
    fn parse_search_body_reads_the_search_response_envelope() {
        let payload = tracecure_api::SearchResponse {
            records: vec![rec()],
            scanned: 7,
        };
        let body = serde_json::to_string(&payload).unwrap();
        let out = parse_search_body(&body).expect("envelope must parse");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].body, "client error response");
    }

    /// Regression: the old code targeted `Vec<Record>`, so every call
    /// returned `Parse` and the UI silently showed nothing.
    #[test]
    fn parse_search_body_rejects_a_bare_array() {
        let body = serde_json::to_string(&vec![rec()]).unwrap();
        assert!(
            parse_search_body(&body).is_err(),
            "a bare array is not the server's response shape"
        );
    }
}
