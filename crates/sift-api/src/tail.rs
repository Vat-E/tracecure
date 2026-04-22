use serde::{Deserialize, Serialize};
use sift_core::Record;

/// A single frame sent over the live-tail WebSocket.
///
/// `Record` carries a newly ingested record. `Heartbeat` is sent periodically
/// so idle proxies keep the connection open. `Error` surfaces a server-side
/// problem the client should display and then reconnect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TailFrame {
    Record(Record),
    Heartbeat,
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use sift_core::{Level, Record};
    use std::collections::BTreeMap;

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
        let f = TailFrame::Error { message: "boom".into() };
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(json, r#"{"type":"error","message":"boom"}"#);
    }
}
