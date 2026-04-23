//! Arrow schema + `records_to_batch`, ported from bench/format-fit.
//!
//! Schema (8 columns):
//!   timestamp    TIMESTAMP(MICROSECOND, UTC)    not null
//!   host         UTF8                            not null
//!   app          UTF8                            not null
//!   `stream_id`    UTF8                            not null
//!   level        UTF8                            not null
//!   body         UTF8                            not null
//!   labels       MAP<UTF8, UTF8>                 not null (may be empty)
//!   `fields_json`  UTF8                            not null (serialized JSON blob)
//!
//! The spike's `env` / `region` flattened columns are removed per ADR-002.

use std::sync::Arc;

use arrow::array::{ArrayRef, MapBuilder, RecordBatch, StringBuilder, TimestampMicrosecondBuilder};
use arrow::datatypes::{DataType, Field, Fields, Schema, TimeUnit};
use sift_core::{Level, Record};

#[must_use]
pub fn schema() -> Arc<Schema> {
    // Arrow 55: MapBuilder emits values as nullable, so declare them nullable
    // in the schema too — mismatch causes RecordBatch::try_new to reject.
    let labels = DataType::Map(
        Arc::new(Field::new(
            "entries",
            DataType::Struct(Fields::from(vec![
                Field::new("keys", DataType::Utf8, false),
                Field::new("values", DataType::Utf8, true),
            ])),
            false,
        )),
        false,
    );
    Arc::new(Schema::new(vec![
        Field::new(
            "timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("host", DataType::Utf8, false),
        Field::new("app", DataType::Utf8, false),
        Field::new("stream_id", DataType::Utf8, false),
        Field::new("level", DataType::Utf8, false),
        Field::new("body", DataType::Utf8, false),
        Field::new("labels", labels, false),
        Field::new("fields_json", DataType::Utf8, false),
    ]))
}

pub fn records_to_batch(records: &[Record], schema: Arc<Schema>) -> anyhow::Result<RecordBatch> {
    let n = records.len();
    let mut ts = TimestampMicrosecondBuilder::with_capacity(n).with_timezone("UTC");
    let mut host = StringBuilder::with_capacity(n, n * 16);
    let mut app = StringBuilder::with_capacity(n, n * 8);
    let mut stream = StringBuilder::with_capacity(n, n * 16);
    let mut level_col = StringBuilder::with_capacity(n, n * 8);
    let mut body = StringBuilder::with_capacity(n, n * 80);
    let mut labels = MapBuilder::new(None, StringBuilder::new(), StringBuilder::new());
    let mut fields_json = StringBuilder::with_capacity(n, n * 40);

    for r in records {
        ts.append_value(r.timestamp.timestamp_micros());
        host.append_value(&r.host);
        app.append_value(&r.app);
        stream.append_value(&r.stream_id);
        level_col.append_value(level_as_str(r.level));
        body.append_value(&r.body);

        for (k, v) in &r.labels {
            labels.keys().append_value(k);
            labels.values().append_value(v);
        }
        labels.append(true)?;

        fields_json.append_value(serde_json::to_string(&r.fields)?);
    }

    let arrays: Vec<ArrayRef> = vec![
        Arc::new(ts.finish()),
        Arc::new(host.finish()),
        Arc::new(app.finish()),
        Arc::new(stream.finish()),
        Arc::new(level_col.finish()),
        Arc::new(body.finish()),
        Arc::new(labels.finish()),
        Arc::new(fields_json.finish()),
    ];
    Ok(RecordBatch::try_new(schema, arrays)?)
}

fn level_as_str(l: Level) -> &'static str {
    match l {
        Level::Trace => "trace",
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
        Level::Fatal => "fatal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use sift_core::{Level, Record};
    use std::collections::BTreeMap;

    fn rec(body: &str) -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 23, 12, 0, 0).unwrap(),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: Level::Info,
            body: body.into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    #[test]
    fn schema_has_eight_columns() {
        assert_eq!(schema().fields().len(), 8);
    }

    #[test]
    fn records_to_batch_preserves_row_count_and_body() {
        let recs = vec![rec("alpha"), rec("beta"), rec("gamma")];
        let batch = records_to_batch(&recs, schema()).unwrap();
        assert_eq!(batch.num_rows(), 3);
        let bodies = batch
            .column_by_name("body")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        assert_eq!(bodies.value(0), "alpha");
        assert_eq!(bodies.value(1), "beta");
        assert_eq!(bodies.value(2), "gamma");
    }

    #[test]
    fn records_to_batch_serializes_fields_as_json_string() {
        let mut r = rec("x");
        r.fields.insert("n".into(), serde_json::json!(7));
        let batch = records_to_batch(&[r], schema()).unwrap();
        let fields = batch
            .column_by_name("fields_json")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        assert_eq!(fields.value(0), r#"{"n":7}"#);
    }
}
