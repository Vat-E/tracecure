use serde::{Deserialize, Serialize};
use tracecure_core::Record;

/// A single frame sent over the live-tail WebSocket.
///
/// `Record` carries a newly ingested record. `Heartbeat` is sent periodically
/// so idle proxies keep the connection open. `Error` surfaces a server-side
/// problem the client should display and then reconnect. `Unknown` is a
/// forward-compat catch-all so an older client does not hard-fail when a
/// newer server emits a variant it has not learned about yet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TailFrame {
    Record(Record),
    Heartbeat,
    Error {
        message: String,
    },
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;
    use tracecure_core::{Level, Record};

    #[test]
    fn heartbeat_serializes_as_tagged_object() {
        let f = TailFrame::Heartbeat;
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(json, r#"{"type":"heartbeat"}"#);
    }

    #[test]
    fn record_frame_roundtrips() {
        let rec = Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 22, 0, 0, 0).unwrap(),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: Level::Info,
            body: "hi".into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        };
        let frame = TailFrame::Record(rec.clone());
        let json = serde_json::to_string(&frame).unwrap();
        let back: TailFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TailFrame::Record(rec));
    }

    #[test]
    fn error_frame_roundtrips() {
        let f = TailFrame::Error {
            message: "boom".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(json, r#"{"type":"error","message":"boom"}"#);
    }

    #[test]
    fn unknown_tag_deserializes_to_unknown_variant() {
        let json = r#"{"type":"stats","count":42}"#;
        let frame: TailFrame = serde_json::from_str(json).unwrap();
        assert_eq!(frame, TailFrame::Unknown);
    }
}
