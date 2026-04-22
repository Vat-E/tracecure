use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Severity level. Stored as lowercase strings on the wire.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
    Fatal,
}

/// A single log record. Normalized shape used everywhere inside Sift.
///
/// `labels` are low-cardinality indexed key/value pairs.
/// `fields` are schema-flexible structured data.
/// `raw` preserves the original untouched payload when the ingest stage had one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub timestamp: DateTime<Utc>,
    pub host: String,
    pub app: String,
    pub stream_id: String,
    #[serde(default)]
    pub level: Level,
    pub body: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "_raw", skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn level_roundtrips_as_lowercase() {
        let json = serde_json::to_string(&Level::Warn).unwrap();
        assert_eq!(json, "\"warn\"");
        let back: Level = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Level::Warn);
    }

    #[test]
    fn level_defaults_to_info() {
        assert_eq!(Level::default(), Level::Info);
    }

    #[test]
    fn record_roundtrips_through_json() {
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        let mut fields = BTreeMap::new();
        fields.insert("user_id".to_string(), serde_json::json!(42));

        let rec = Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap(),
            host: "box-1".into(),
            app: "api".into(),
            stream_id: "api".into(),
            level: Level::Error,
            body: "boom".into(),
            labels,
            fields,
            raw: Some("boom\n".into()),
        };

        let json = serde_json::to_string(&rec).unwrap();
        let back: Record = serde_json::from_str(&json).unwrap();
        assert_eq!(back, rec);
    }

    #[test]
    fn record_parses_minimal_input_using_defaults() {
        let json = r#"{
            "timestamp":"2026-04-22T12:00:00Z",
            "host":"h",
            "app":"a",
            "stream_id":"s",
            "body":"hi"
        }"#;
        let rec: Record = serde_json::from_str(json).unwrap();
        assert_eq!(rec.level, Level::Info);
        assert!(rec.labels.is_empty());
        assert!(rec.fields.is_empty());
        assert!(rec.raw.is_none());
    }

    #[test]
    fn record_skips_empty_optional_fields_when_serializing() {
        let rec = Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 22, 0, 0, 0).unwrap(),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: Level::Info,
            body: "x".into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        };
        let json = serde_json::to_string(&rec).unwrap();
        assert!(!json.contains("labels"));
        assert!(!json.contains("fields"));
        assert!(!json.contains("_raw"));
    }
}
