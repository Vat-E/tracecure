use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable identifier for a logical log stream. Assigned at ingest.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StreamId(pub String);

impl StreamId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for StreamId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_id_is_transparent_in_json() {
        let id = StreamId::new("api.prod");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"api.prod\"");
        let back: StreamId = serde_json::from_str("\"api.prod\"").unwrap();
        assert_eq!(back, id);
    }
}
