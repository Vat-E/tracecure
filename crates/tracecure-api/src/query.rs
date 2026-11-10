use serde::{Deserialize, Serialize};
use tracecure_core::Record;

/// Query string for `GET /v1/query`.
///
/// `query` is `TracecureQL` — see the `tracecure-query` crate. `from` is
/// inclusive and `to` exclusive, as in `/v1/search`.
///
/// `cursor` is the `next_cursor` of the previous page, verbatim. It is
/// opaque: its contents are the server's business and may change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

fn default_limit() -> usize {
    100
}

/// A page of results, newest first.
///
/// A page can hold slightly more than `limit` records: it is never cut
/// between records that share a timestamp.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryResponse {
    pub records: Vec<Record>,
    /// Records the time range covers — an upper bound on the work, as in
    /// `/v1/search`.
    pub scanned: usize,
    /// Present when there may be older matches: send it back as `cursor`.
    /// Absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// The body of a `400` from `/v1/query`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryError {
    pub error: String,
    /// Byte offset into `query` where parsing stopped, when the query itself
    /// is what is wrong.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_defaults_and_optional_fields() {
        let q: QueryRequest = serde_json::from_str(r#"{"query":"{}"}"#).unwrap();
        assert_eq!((q.limit, q.cursor.as_deref()), (100, None));
    }

    #[test]
    fn the_last_page_has_no_cursor_key() {
        let r = QueryResponse {
            records: vec![],
            scanned: 3,
            next_cursor: None,
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"records":[],"scanned":3}"#
        );
    }
}
