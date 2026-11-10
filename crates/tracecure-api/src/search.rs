use serde::{Deserialize, Serialize};
use tracecure_core::Record;

/// Query string for `GET /v1/search`.
///
/// `q` is a case-sensitive substring match against `Record::body`.
/// `limit` caps the number of returned records; it is applied after every
/// filter, so it keeps the newest `limit` records that match.
///
/// `level`, `app`, `host` and `stream_id` are each a comma-separated list of
/// alternatives; absent or empty means "any". They are strings rather than
/// lists because this type is a URL query string, where a repeated key does
/// not survive every client. A name in `app` / `host` / `stream_id` ending in
/// `*` matches by prefix. An unknown name in `level` is a `400`.
///
/// All four are additive: a client that does not send them gets exactly the
/// behaviour it had before they existed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,
}

fn default_limit() -> usize {
    100
}

// Manual impl: derived Default would give `limit = 0`, but the wire default is 100.
// `default_limit` is shared with serde so the two paths can never drift.
impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            q: String::new(),
            limit: default_limit(),
            from: None,
            to: None,
            level: None,
            app: None,
            host: None,
            stream_id: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub records: Vec<Record>,
    /// Number of records scanned from disk, regardless of whether they matched.
    pub scanned: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_query_parses_from_form_like_input() {
        let q: SearchQuery = serde_json::from_str(r#"{"q":"boom"}"#).unwrap();
        assert_eq!(q.q, "boom");
        assert_eq!(q.limit, 100);
    }

    #[test]
    fn search_query_default_limit_is_100() {
        assert_eq!(SearchQuery::default().limit, 100);
    }

    #[test]
    fn search_response_roundtrips_empty() {
        let r = SearchResponse {
            records: vec![],
            scanned: 0,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"records":[],"scanned":0}"#);
        let back: SearchResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn search_query_filters_default_to_absent() {
        let q: SearchQuery = serde_json::from_str(r#"{"q":"boom"}"#).unwrap();
        assert_eq!(
            (q.level, q.app, q.host, q.stream_id),
            (None, None, None, None)
        );
    }

    #[test]
    fn search_query_parses_the_filter_params() {
        let json = r#"{"level":"error,fatal","app":"pay*","host":"edge-1","stream_id":"api-prod"}"#;
        let q: SearchQuery = serde_json::from_str(json).unwrap();
        assert_eq!(q.level.as_deref(), Some("error,fatal"));
        assert_eq!(q.app.as_deref(), Some("pay*"));
        assert_eq!(q.host.as_deref(), Some("edge-1"));
        assert_eq!(q.stream_id.as_deref(), Some("api-prod"));
    }

    /// An unset filter is left out on the wire rather than sent as `null`,
    /// so a request from a new client to an old server is one the old
    /// server already understands.
    #[test]
    fn unset_filters_are_not_serialized() {
        let json = serde_json::to_string(&SearchQuery::default()).unwrap();
        for key in ["level", "app", "host", "stream_id"] {
            assert!(!json.contains(key), "{key} leaked into {json}");
        }
    }

    #[test]
    fn search_query_parses_time_range_params() {
        let json = r#"{"q":"x","from":"2026-04-23T00:00:00Z","to":"2026-04-23T01:00:00Z"}"#;
        let q: SearchQuery = serde_json::from_str(json).unwrap();
        assert!(q.from.is_some());
        assert!(q.to.is_some());
        assert_eq!(q.q, "x");
    }
}
