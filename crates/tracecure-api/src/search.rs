use serde::{Deserialize, Serialize};
use tracecure_core::Record;

/// Query string for `GET /v1/search`.
///
/// `q` is a case-sensitive substring match against `Record::body` for M0.
/// `limit` caps the number of returned records (server enforces an upper
/// bound).
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
    fn search_query_parses_time_range_params() {
        let json = r#"{"q":"x","from":"2026-04-23T00:00:00Z","to":"2026-04-23T01:00:00Z"}"#;
        let q: SearchQuery = serde_json::from_str(json).unwrap();
        assert!(q.from.is_some());
        assert!(q.to.is_some());
        assert_eq!(q.q, "x");
    }
}
