//! Arrow schema for Record and parquet writer/reader.
//!
//! Schema (all nullable flags match the intent):
//!   timestamp    TIMESTAMP(MICROSECOND, UTC)   not null
//!   host         UTF8                           not null
//!   app          UTF8                           not null
//!   stream_id    UTF8                           not null
//!   level        UTF8                           not null
//!   body         UTF8                           not null
//!   labels       MAP<UTF8, UTF8>                not null (may be empty)
//!   fields_json  UTF8                           not null (serialized JSON)
//!   env          UTF8                           nullable (flattened label)
//!   region       UTF8                           nullable (flattened label)
//!
//! `labels` is the full schema-flexible map. `env` and `region` are flattened
//! top-level columns sourced from `labels["env"]` and `labels["region"]` —
//! they exist to measure parquet predicate-pushdown vs map-column scan.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use arrow::array::{ArrayRef, MapBuilder, RecordBatch, StringBuilder, TimestampMicrosecondBuilder};
use arrow::datatypes::{DataType, Field, Fields, Schema, TimeUnit};
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use parquet::schema::types::ColumnPath;
use sift_core::{Level, Record};

/// Writer knobs. `Default` matches spike v1 behaviour (tiny row groups,
/// no bloom filters) so the existing tests keep passing without changes.
#[derive(Debug, Clone)]
pub struct WriteConfig {
    /// Rows per RecordBatch handed to the ArrowWriter. Parquet uses this as
    /// the row-group boundary by default.
    pub batch_rows: usize,
    /// Which top-level columns get a bloom filter. Spike v1 wrote no bloom
    /// filters; ADR-001 recommends `timestamp`, `stream_id`, `host`.
    pub bloom_filter_columns: Vec<String>,
    /// zstd compression level, 1..=22. 3 is a reasonable default.
    pub zstd_level: i32,
}

impl Default for WriteConfig {
    fn default() -> Self {
        Self {
            batch_rows: 8192,
            bloom_filter_columns: Vec::new(),
            zstd_level: 3,
        }
    }
}

pub fn schema() -> Arc<Schema> {
    // Arrow 55: MapBuilder produces values as nullable=true, so the schema
    // must match — declaring values as nullable=false causes a type mismatch
    // at RecordBatch::try_new time.
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
        Field::new("env", DataType::Utf8, true),
        Field::new("region", DataType::Utf8, true),
    ]))
}

pub fn records_to_batch(records: &[Record], schema: Arc<Schema>) -> anyhow::Result<RecordBatch> {
    let mut ts =
        TimestampMicrosecondBuilder::with_capacity(records.len()).with_timezone("UTC");
    let mut host = StringBuilder::with_capacity(records.len(), records.len() * 16);
    let mut app = StringBuilder::with_capacity(records.len(), records.len() * 8);
    let mut stream = StringBuilder::with_capacity(records.len(), records.len() * 16);
    let mut level_col = StringBuilder::with_capacity(records.len(), records.len() * 8);
    let mut body = StringBuilder::with_capacity(records.len(), records.len() * 80);
    let key_builder = StringBuilder::new();
    let value_builder = StringBuilder::new();
    let mut labels = MapBuilder::new(None, key_builder, value_builder);
    let mut fields_json = StringBuilder::with_capacity(records.len(), records.len() * 40);
    let mut env = StringBuilder::with_capacity(records.len(), records.len() * 4);
    let mut region = StringBuilder::with_capacity(records.len(), records.len() * 4);

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

        match r.labels.get("env") {
            Some(v) => env.append_value(v),
            None => env.append_null(),
        }
        match r.labels.get("region") {
            Some(v) => region.append_value(v),
            None => region.append_null(),
        }
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
        Arc::new(env.finish()),
        Arc::new(region.finish()),
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

#[derive(Debug)]
pub struct WriteStats {
    pub bytes_on_disk: u64,
    pub row_groups_written: usize,
    #[allow(dead_code)] // available for the Task 7 ADR report if needed
    pub row_group_meta_rows: i64,
}

// Default-config wrapper. Only the tests use it directly now; main.rs always
// passes an explicit WriteConfig. Kept as a convenience for ad-hoc scripts.
#[allow(dead_code)]
pub fn write_parquet(records: &[Record], path: &Path) -> anyhow::Result<WriteStats> {
    write_parquet_with(records, path, &WriteConfig::default())
}

pub fn write_parquet_with(
    records: &[Record],
    path: &Path,
    cfg: &WriteConfig,
) -> anyhow::Result<WriteStats> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    let schema = schema();

    let mut props_builder = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(cfg.zstd_level)?))
        .set_max_row_group_size(cfg.batch_rows);
    if !cfg.bloom_filter_columns.is_empty() {
        props_builder = props_builder.set_bloom_filter_enabled(true);
        for col in &cfg.bloom_filter_columns {
            let path = ColumnPath::new(vec![col.clone()]);
            props_builder = props_builder
                .set_column_bloom_filter_enabled(path.clone(), true)
                .set_column_bloom_filter_fpp(path, 0.01);
        }
    }
    let props = props_builder.build();
    let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(props))?;

    let mut row_groups = 0usize;
    for chunk in records.chunks(cfg.batch_rows) {
        let batch = records_to_batch(chunk, schema.clone())?;
        writer.write(&batch)?;
        row_groups += 1;
    }
    let file_meta = writer.close()?;
    let size = std::fs::metadata(path)?.len();
    Ok(WriteStats {
        bytes_on_disk: size,
        row_groups_written: row_groups,
        row_group_meta_rows: file_meta.num_rows,
    })
}

/// Footer length + trailer: read the last 8 bytes of the file, parse the
/// little-endian u32 footer-length, add the 8-byte trailer. Version-stable.
pub fn footer_size(path: &Path) -> anyhow::Result<u64> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    anyhow::ensure!(len >= 8, "file too short to be parquet");
    file.seek(SeekFrom::End(-8))?;
    let mut trailer = [0u8; 8];
    file.read_exact(&mut trailer)?;
    anyhow::ensure!(&trailer[4..] == b"PAR1", "missing parquet magic at EOF");
    let footer_len = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    Ok(u64::from(footer_len) + 8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::generate;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    #[test]
    fn round_trip_small_batch_preserves_row_count_and_bodies() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tiny.parquet");
        let records = generate(1, 1024);

        let stats = write_parquet(&records, &path).unwrap();
        assert!(stats.bytes_on_disk > 0);

        let file = File::open(&path).unwrap();
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap();
        let mut rows = 0usize;
        for batch in reader {
            rows += batch.unwrap().num_rows();
        }
        assert_eq!(rows, records.len());
    }

    #[test]
    fn schema_has_ten_columns() {
        assert_eq!(schema().fields().len(), 10);
    }

    #[test]
    fn footer_size_is_smaller_than_total() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("small.parquet");
        let records = generate(2, 2048);
        let stats = write_parquet(&records, &path).unwrap();
        let footer = footer_size(&path).unwrap();
        assert!(
            footer < stats.bytes_on_disk,
            "footer {footer} should be < file {}",
            stats.bytes_on_disk
        );
    }
}
