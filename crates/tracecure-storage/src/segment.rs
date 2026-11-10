//! Parquet segment writer + reader. One segment = one parquet file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use arrow::array::{Array, RecordBatch, StringArray};
use chrono::{DateTime, Utc};
use parquet::arrow::arrow_reader::{ParquetRecordBatchReaderBuilder, RowSelection, RowSelector};
use parquet::arrow::{ArrowWriter, ProjectionMask};
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use tracecure_core::{Level, Record};

use crate::arrow::{records_to_batch, schema};
use crate::filter::SearchFilter;
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
    scan_segment_filtered(handle, &SearchFilter::text(needle))
}

/// Scan a segment and return every record `filter` accepts.
///
/// The filter is tested on the row's plain columns first, so a row that
/// fails it is never decoded: its labels map and fields JSON are the
/// expensive part of materializing a record.
///
/// This reads the whole segment. A search that only wants the newest few
/// matches should use [`scan_segment_newest`].
pub fn scan_segment_filtered(
    handle: &SegmentHandle,
    filter: &SearchFilter,
) -> anyhow::Result<Vec<Record>> {
    let file = std::fs::File::open(&handle.path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch?;
        let cols = Columns::of(&batch);
        for i in 0..batch.num_rows() {
            if cols.accepted_by(filter, i) {
                out.push(cols.record(i)?);
            }
        }
    }
    Ok(out)
}

/// Scan a segment for the newest `limit` records that `filter` accepts and
/// whose timestamp is in `[from, to)`. The result is in no particular order.
///
/// Two passes, so that only the rows that are returned are ever decoded:
///
/// 1. Read the timestamp and the columns the filter looks at — nothing
///    else — and keep the row numbers of the newest `limit` matches.
/// 2. Read those rows, and only those, with every column.
///
/// A search for "the newest 500" over a million-row segment therefore
/// decodes 500 label maps instead of a million, and a search that matches
/// nothing never touches the labels and fields columns at all.
pub fn scan_segment_newest(
    handle: &SegmentHandle,
    filter: &SearchFilter,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    limit: usize,
) -> anyhow::Result<Vec<Record>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let rows = newest_matching_rows(handle, filter, from, to, limit)?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let file = std::fs::File::open(&handle.path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?
        .with_row_selection(select_rows(&rows))
        .build()?;
    let mut out = Vec::with_capacity(rows.len());
    for batch in reader {
        let batch = batch?;
        let cols = Columns::of(&batch);
        for i in 0..batch.num_rows() {
            out.push(cols.record(i)?);
        }
    }
    Ok(out)
}

/// Pass 1 of [`scan_segment_newest`]: the row numbers, ascending, of the
/// newest `limit` rows that pass the filter and the time range.
fn newest_matching_rows(
    handle: &SegmentHandle,
    filter: &SearchFilter,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    limit: usize,
) -> anyhow::Result<Vec<usize>> {
    let mut wanted = vec!["timestamp"];
    if !filter.levels.is_empty() {
        wanted.push("level");
    }
    if !filter.apps.is_empty() {
        wanted.push("app");
    }
    if !filter.hosts.is_empty() {
        wanted.push("host");
    }
    if !filter.streams.is_empty() {
        wanted.push("stream_id");
    }
    if !filter.text.is_empty() {
        wanted.push("body");
    }

    let file = std::fs::File::open(&handle.path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let mask = ProjectionMask::columns(builder.parquet_schema(), wanted);
    let reader = builder.with_projection(mask).build()?;

    // A min-heap on (timestamp, row): the root is the oldest row still in
    // the running, so it is the one a newer row displaces.
    let mut newest: BinaryHeap<Reverse<(i64, usize)>> = BinaryHeap::with_capacity(limit + 1);
    let mut first_row = 0usize;
    for batch in reader {
        let batch = batch?;
        let cols = Columns::of(&batch);
        let ts = cols.timestamps()?;
        for i in 0..batch.num_rows() {
            let micros = ts.value(i);
            // Cheapest test first: a row no newer than the oldest one kept
            // cannot change the answer, whatever else is true of it.
            if newest.len() == limit
                && newest
                    .peek()
                    .is_some_and(|Reverse((oldest, _))| micros <= *oldest)
            {
                continue;
            }
            let Some(at) = DateTime::<Utc>::from_timestamp_micros(micros) else {
                continue;
            };
            if from.is_some_and(|f| at < f) || to.is_some_and(|t| at >= t) {
                continue;
            }
            if !cols.accepted_by(filter, i) {
                continue;
            }
            newest.push(Reverse((micros, first_row + i)));
            if newest.len() > limit {
                newest.pop();
            }
        }
        first_row += batch.num_rows();
    }

    let mut rows: Vec<usize> = newest.into_iter().map(|Reverse((_, row))| row).collect();
    rows.sort_unstable();
    Ok(rows)
}

/// A selection of exactly `rows` (ascending, distinct row numbers).
fn select_rows(rows: &[usize]) -> RowSelection {
    let mut selectors = Vec::new();
    let mut next = 0usize;
    let mut run = 0usize;
    for &row in rows {
        if row > next {
            if run > 0 {
                selectors.push(RowSelector::select(run));
                run = 0;
            }
            selectors.push(RowSelector::skip(row - next));
        }
        run += 1;
        next = row + 1;
    }
    if run > 0 {
        selectors.push(RowSelector::select(run));
    }
    RowSelection::from(selectors)
}

/// The columns of one batch, each present only if the reader projected it.
struct Columns<'a> {
    ts: Option<&'a arrow::array::TimestampMicrosecondArray>,
    host: Option<&'a StringArray>,
    app: Option<&'a StringArray>,
    stream: Option<&'a StringArray>,
    level: Option<&'a StringArray>,
    body: Option<&'a StringArray>,
    labels: Option<&'a arrow::array::MapArray>,
    fields_json: Option<&'a StringArray>,
}

impl<'a> Columns<'a> {
    fn of(batch: &'a RecordBatch) -> Self {
        fn col<'b, T: 'static>(batch: &'b RecordBatch, name: &str) -> Option<&'b T> {
            batch.column_by_name(name)?.as_any().downcast_ref::<T>()
        }
        Self {
            ts: col(batch, "timestamp"),
            host: col(batch, "host"),
            app: col(batch, "app"),
            stream: col(batch, "stream_id"),
            level: col(batch, "level"),
            body: col(batch, "body"),
            labels: col(batch, "labels"),
            fields_json: col(batch, "fields_json"),
        }
    }

    fn timestamps(&self) -> anyhow::Result<&'a arrow::array::TimestampMicrosecondArray> {
        self.ts.context("segment has no timestamp column")
    }

    /// Whether row `i` passes `filter`. A column the reader did not project
    /// reads as empty — correct only because [`newest_matching_rows`]
    /// projects every column the filter has a constraint on.
    fn accepted_by(&self, filter: &SearchFilter, i: usize) -> bool {
        let text = |c: Option<&'a StringArray>| c.map_or("", |c| c.value(i));
        filter.matches_columns(
            self.level.map_or(Level::Info, |c| parse_level(c.value(i))),
            text(self.app),
            text(self.host),
            text(self.stream),
            text(self.body),
        )
    }

    /// Row `i` as a record. Needs every column.
    fn record(&self, i: usize) -> anyhow::Result<Record> {
        let text = |c: Option<&'a StringArray>, name: &str| {
            c.map(|c| c.value(i).to_string())
                .with_context(|| format!("segment has no {name} column"))
        };
        Ok(Record {
            timestamp: DateTime::<Utc>::from_timestamp_micros(self.timestamps()?.value(i))
                .context("timestamp out of range")?,
            host: text(self.host, "host")?,
            app: text(self.app, "app")?,
            stream_id: text(self.stream, "stream_id")?,
            level: parse_level(&text(self.level, "level")?),
            body: text(self.body, "body")?,
            labels: decode_labels(self.labels.context("segment has no labels column")?, i),
            fields: serde_json::from_str(&text(self.fields_json, "fields_json")?)
                .unwrap_or_default(),
            raw: None,
        })
    }
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

    /// `n` records one second apart, written as one segment.
    fn segment_of(tmp: &tempfile::TempDir, n: i64) -> SegmentHandle {
        let records: Vec<Record> = (0..n)
            .map(|i| {
                let mut r = rec(&format!("line {i}"), i);
                r.level = if i % 10 == 0 {
                    Level::Error
                } else {
                    Level::Info
                };
                r.app = format!("app-{}", i % 3);
                r
            })
            .collect();
        let granularity = Duration::from_secs(3600);
        let partition = partition_id(records[0].timestamp, granularity);
        write_segment(
            &records,
            &tmp.path().join("seg.parquet"),
            partition,
            granularity,
        )
        .unwrap()
    }

    fn bodies_newest_first(mut records: Vec<Record>) -> Vec<String> {
        records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        records.into_iter().map(|r| r.body).collect()
    }

    #[test]
    fn newest_returns_the_last_rows_fully_decoded() {
        let tmp = tempfile::tempdir().unwrap();
        let h = segment_of(&tmp, 3_000);
        let got = scan_segment_newest(&h, &SearchFilter::default(), None, None, 3).unwrap();
        assert_eq!(got.len(), 3);
        assert!(got
            .iter()
            .all(|r| r.labels["env"] == "prod" && r.host == "h"));
        assert_eq!(
            bodies_newest_first(got),
            ["line 2999", "line 2998", "line 2997"]
        );
    }

    #[test]
    fn newest_agrees_with_a_full_scan_for_every_kind_of_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let h = segment_of(&tmp, 3_000);
        let filters = [
            SearchFilter::default(),
            SearchFilter::text("line 12"),
            SearchFilter::from_params("", Some("error"), None, None, None).unwrap(),
            SearchFilter::from_params("7", None, Some("app-1"), None, None).unwrap(),
            SearchFilter::from_params("", None, None, Some("h*"), Some("s")).unwrap(),
            SearchFilter::text("no such thing"),
        ];
        for filter in &filters {
            for limit in [1, 7, 500, 10_000] {
                let mut full = bodies_newest_first(scan_segment_filtered(&h, filter).unwrap());
                full.truncate(limit);
                let got = scan_segment_newest(&h, filter, None, None, limit).unwrap();
                assert_eq!(bodies_newest_first(got), full, "{filter:?} limit {limit}");
            }
        }
    }

    #[test]
    fn newest_respects_the_range_with_an_exclusive_end() {
        let tmp = tempfile::tempdir().unwrap();
        let h = segment_of(&tmp, 3_000);
        let t0 = Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap();
        let at = |s: i64| Some(t0 + chrono::Duration::seconds(s));

        let got = scan_segment_newest(&h, &SearchFilter::default(), at(100), at(104), 500).unwrap();
        assert_eq!(
            bodies_newest_first(got),
            ["line 103", "line 102", "line 101", "line 100"]
        );

        // The limit is applied inside the range, not before it.
        let got = scan_segment_newest(&h, &SearchFilter::default(), None, at(104), 2).unwrap();
        assert_eq!(bodies_newest_first(got), ["line 103", "line 102"]);

        let got = scan_segment_newest(&h, &SearchFilter::default(), at(5_000), None, 5).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn newest_with_a_zero_limit_reads_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let h = segment_of(&tmp, 10);
        let got = scan_segment_newest(&h, &SearchFilter::default(), None, None, 0).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn select_rows_covers_exactly_the_rows_asked_for() {
        let picked = |rows: &[usize]| -> Vec<(bool, usize)> {
            select_rows(rows)
                .iter()
                .map(|s| (s.skip, s.row_count))
                .collect()
        };
        assert_eq!(picked(&[0, 1, 2]), [(false, 3)]);
        assert_eq!(picked(&[5]), [(true, 5), (false, 1)]);
        assert_eq!(
            picked(&[1, 2, 7, 9]),
            [
                (true, 1),
                (false, 2),
                (true, 4),
                (false, 1),
                (true, 1),
                (false, 1)
            ]
        );
    }
}
