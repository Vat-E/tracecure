//! Parquet segment writer + reader. One segment = one parquet file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use arrow::array::Array;
use chrono::{DateTime, Utc};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use std::collections::BTreeMap;
use tracecure_core::{Level, Record};

use crate::arrow::{records_to_batch, schema};
use crate::partition::{partition_id, partition_label};

const ZSTD_LEVEL: i32 = 3;
const MAX_ROW_GROUP_SIZE: usize = 1_000_000;

#[derive(Debug, Clone)]
pub struct SegmentHandle {
    pub path: PathBuf,
    pub row_count: u64,
    pub min_ts: DateTime<Utc>,
    pub max_ts: DateTime<Utc>,
    /// Partition identity (floored timestamp). Matches `min_ts.duration_trunc`
    /// for any valid segment; stored separately so compactor queries don't
    /// re-derive from filenames.
    pub partition: DateTime<Utc>,
}

/// Write `records` to `path` as a parquet file and return a handle describing
/// it. Records are expected in timestamp-ascending order (memtable order).
/// All records must belong to `partition` (as derived from `granularity`).
pub fn write_segment(
    records: &[Record],
    path: &Path,
    partition: DateTime<Utc>,
    granularity: Duration,
) -> anyhow::Result<SegmentHandle> {
    anyhow::ensure!(!records.is_empty(), "refusing to write empty segment");

    for r in records {
        let got = partition_id(r.timestamp, granularity);
        anyhow::ensure!(
            got == partition,
            "record timestamp {} maps to partition {got}, not {partition}",
            r.timestamp
        );
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let min_ts = records.iter().map(|r| r.timestamp).min().unwrap();
    let max_ts = records.iter().map(|r| r.timestamp).max().unwrap();

    let schema = schema();
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(ZSTD_LEVEL)?))
        .set_max_row_group_size(MAX_ROW_GROUP_SIZE)
        .build();
    let file = std::fs::File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(props))?;
    let batch = records_to_batch(records, schema)?;
    writer.write(&batch)?;
    let meta = writer.close()?;

    Ok(SegmentHandle {
        path: path.to_path_buf(),
        row_count: meta.num_rows.cast_unsigned(),
        min_ts,
        max_ts,
        partition,
    })
}

/// Filename inside the `segments/` directory for a given partition + sequence.
#[must_use]
pub fn segment_filename(partition: DateTime<Utc>, seq: u64, granularity: Duration) -> String {
    format!(
        "{label}-{seq:010}.parquet",
        label = partition_label(partition, granularity),
    )
}

/// Tmp filename used mid-compaction. Renamed to the final form on success.
#[must_use]
pub fn compacting_filename(partition: DateTime<Utc>, seq: u64, granularity: Duration) -> String {
    format!(
        "{label}-{seq:010}.compacting.parquet",
        label = partition_label(partition, granularity),
    )
}

/// Open a segment file and return a `SegmentHandle` read from its footer.
pub fn load_handle(path: &Path, granularity: Duration) -> anyhow::Result<SegmentHandle> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let md = reader.metadata();
    let row_count = md.file_metadata().num_rows().cast_unsigned();
    anyhow::ensure!(row_count > 0, "segment has 0 rows: {}", path.display());

    let schema_arrow = reader.schema().clone();
    let ts_idx = schema_arrow.index_of("timestamp")?;

    let mut min_micros: Option<i64> = None;
    let mut max_micros: Option<i64> = None;
    for rg in md.row_groups() {
        let col = rg.column(ts_idx);
        if let Some(stats) = col.statistics() {
            if let Some(min) = stats.min_bytes_opt() {
                if min.len() == 8 {
                    let v = i64::from_le_bytes(min.try_into().unwrap());
                    min_micros = Some(min_micros.map_or(v, |m: i64| m.min(v)));
                }
            }
            if let Some(max) = stats.max_bytes_opt() {
                if max.len() == 8 {
                    let v = i64::from_le_bytes(max.try_into().unwrap());
                    max_micros = Some(max_micros.map_or(v, |m: i64| m.max(v)));
                }
            }
        }
    }
    let min_ts =
        DateTime::<Utc>::from_timestamp_micros(min_micros.unwrap_or(0)).unwrap_or_else(Utc::now);
    let max_ts =
        DateTime::<Utc>::from_timestamp_micros(max_micros.unwrap_or(0)).unwrap_or_else(Utc::now);

    let partition = partition_id(min_ts, granularity);

    Ok(SegmentHandle {
        path: path.to_path_buf(),
        row_count,
        min_ts,
        max_ts,
        partition,
    })
}

/// Scan a segment and return records whose `body` contains `needle` as a
/// case-sensitive substring. Empty `needle` matches all rows.
pub fn scan_segment(handle: &SegmentHandle, needle: &str) -> anyhow::Result<Vec<Record>> {
    let file = std::fs::File::open(&handle.path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch?;
        let n = batch.num_rows();

        let ts = batch
            .column_by_name("timestamp")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::TimestampMicrosecondArray>()
            .unwrap();
        let host = batch
            .column_by_name("host")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let app = batch
            .column_by_name("app")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let stream = batch
            .column_by_name("stream_id")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let level_col = batch
            .column_by_name("level")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let body = batch
            .column_by_name("body")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let labels = batch
            .column_by_name("labels")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::MapArray>()
            .unwrap();
        let fields_json = batch
            .column_by_name("fields_json")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();

        for i in 0..n {
            let b = body.value(i);
            if !needle.is_empty() && !b.contains(needle) {
                continue;
            }
            out.push(Record {
                timestamp: DateTime::<Utc>::from_timestamp_micros(ts.value(i)).unwrap(),
                host: host.value(i).to_string(),
                app: app.value(i).to_string(),
                stream_id: stream.value(i).to_string(),
                level: parse_level(level_col.value(i)),
                body: b.to_string(),
                labels: decode_labels(labels, i),
                fields: serde_json::from_str(fields_json.value(i)).unwrap_or_default(),
                raw: None,
            });
        }
    }
    Ok(out)
}

fn parse_level(s: &str) -> Level {
    match s {
        "trace" => Level::Trace,
        "debug" => Level::Debug,
        "warn" => Level::Warn,
        "error" => Level::Error,
        "fatal" => Level::Fatal,
        _ => Level::Info,
    }
}

fn decode_labels(arr: &arrow::array::MapArray, row: usize) -> BTreeMap<String, String> {
    let offsets = arr.value_offsets();
    let start = offsets[row].cast_unsigned() as usize;
    let end = offsets[row + 1].cast_unsigned() as usize;
    let keys = arr
        .keys()
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .unwrap();
    let values = arr
        .values()
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .unwrap();
    let mut out = BTreeMap::new();
    for i in start..end {
        if values.is_valid(i) {
            out.insert(keys.value(i).to_string(), values.value(i).to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn rec(body: &str, offset_s: i64) -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap()
                + chrono::Duration::seconds(offset_s),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: Level::Info,
            body: body.into(),
            labels: BTreeMap::from([("env".to_string(), "prod".to_string())]),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    #[test]
    fn write_then_scan_roundtrips_records() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("seg.parquet");
        let records = vec![rec("alpha", 0), rec("beta", 1), rec("gamma", 2)];
        let granularity = Duration::from_secs(3600);
        let partition = crate::partition::partition_id(records[0].timestamp, granularity);
        let handle = write_segment(&records, &path, partition, granularity).unwrap();
        assert_eq!(handle.row_count, 3);
        assert_eq!(handle.min_ts, records[0].timestamp);
        assert_eq!(handle.max_ts, records[2].timestamp);
        assert_eq!(handle.partition, partition);

        let got = scan_segment(&handle, "").unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].body, "alpha");
        assert_eq!(got[0].labels.get("env").map(String::as_str), Some("prod"));
    }

    #[test]
    fn scan_with_substring_filters() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("seg.parquet");
        let records = vec![rec("hello world", 0), rec("goodbye", 1)];
        let granularity = Duration::from_secs(3600);
        let partition = crate::partition::partition_id(records[0].timestamp, granularity);
        let handle = write_segment(&records, &path, partition, granularity).unwrap();
        let got = scan_segment(&handle, "hello").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].body, "hello world");
    }

    #[test]
    fn load_handle_reads_row_count_from_footer() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("seg.parquet");
        let records = vec![rec("x", 0), rec("y", 1)];
        let granularity = Duration::from_secs(3600);
        let partition = crate::partition::partition_id(records[0].timestamp, granularity);
        let written = write_segment(&records, &path, partition, granularity).unwrap();
        let loaded = load_handle(&path, Duration::from_secs(3600)).unwrap();
        assert_eq!(loaded.row_count, written.row_count);
    }

    #[test]
    fn write_empty_slice_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("seg.parquet");
        assert!(write_segment(&[], &path, chrono::Utc::now(), Duration::from_secs(3600)).is_err());
    }

    #[test]
    fn write_segment_rejects_cross_partition_records() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.parquet");
        // Two records separated by more than 1h land in different partitions.
        let records = vec![rec("a", 0), rec("b", 7200)];
        let granularity = Duration::from_secs(3600);
        let partition = crate::partition::partition_id(records[0].timestamp, granularity);
        let err = write_segment(&records, &path, partition, granularity)
            .expect_err("should reject cross-partition records");
        assert!(err.to_string().contains("partition"));
    }
}
