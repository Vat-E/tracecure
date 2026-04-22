use serde::{Deserialize, Serialize};
use sift_core::Record;

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
}

fn default_limit() -> usize {
    100
}

#[allow(clippy::derivable_impls)] // keep manual impl; default_limit is also used by serde
impl Default for SearchQuery {
    fn default() -> Self {
        Self { q: String::new(), limit: default_limit() }
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
        let r = SearchResponse { records: vec![], scanned: 0 };
        let json = serde_json::to_string(&r).unwrap();
        let back: SearchResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
