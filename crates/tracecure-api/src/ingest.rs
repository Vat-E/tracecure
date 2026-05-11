use serde::{Deserialize, Serialize};

/// Response from `POST /v1/ingest`. Summarizes how many NDJSON lines we
/// accepted versus rejected in the request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestResponse {
    pub accepted: usize,
    pub rejected: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_response_roundtrips() {
        let r = IngestResponse {
            accepted: 7,
            rejected: 1,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"accepted":7,"rejected":1}"#);
        let back: IngestResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
