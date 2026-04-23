//! Label-filter benchmarks. Compares two ways to find "env = v0" rows:
//!
//! A) **Map-column scan** — iterate the `labels` MAP column and check each
//!    record's entries. This is the schema-flexible path.
//! B) **Flattened-column predicate** — predicate pushdown on the `env` top-
//!    level column. This is what a real deployment would index if it knew
//!    `env` was high-value up front.

use std::fs::File;
use std::path::Path;
use std::time::{Duration, Instant};

use arrow::array::{Array, BooleanArray, MapArray, StringArray};
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::{ArrowPredicateFn, ParquetRecordBatchReaderBuilder, RowFilter};

pub struct QueryStats {
    #[allow(dead_code)] // available for the Task 7 ADR report if needed
    pub scanned_rows: u64,
    pub matched_rows: u64,
    pub duration: Duration,
    #[allow(dead_code)] // available for the Task 7 ADR report if needed
    pub bytes_read: u64,
}

/// Path A: scan every `labels` map entry, match on (key, needle).
pub fn label_filter_via_map(
    path: &Path,
    key: &str,
    needle: &str,
) -> anyhow::Result<QueryStats> {
    let file = File::open(path)?;
    let start = Instant::now();
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;

    // Use ProjectionMask::roots so all leaves beneath the `labels` map root
    // are included. The Arrow schema root index for `labels` is 6.
    let labels_root_idx = builder.schema().index_of("labels")?;
    let mask = ProjectionMask::roots(builder.parquet_schema(), [labels_root_idx]);
    let reader = builder.with_projection(mask).build()?;

    let mut scanned = 0u64;
    let mut matched = 0u64;
    for batch in reader {
        let batch = batch?;
        scanned += batch.num_rows() as u64;
        let col = batch
            .column(0)
            .as_any()
            .downcast_ref::<MapArray>()
            .expect("labels is a MapArray");
        let offsets = col.value_offsets(); // &[i32]
        let keys = col
            .keys()
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("keys utf8");
        let values = col
            .values()
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("values utf8");
        for row in 0..col.len() {
            let s = offsets[row] as usize;
            let e = offsets[row + 1] as usize;
            for i in s..e {
                if keys.value(i) == key && values.value(i) == needle {
                    matched += 1;
                    break;
                }
            }
        }
    }
    let duration = start.elapsed();
    let bytes_read = std::fs::metadata(path)?.len();
    Ok(QueryStats {
        scanned_rows: scanned,
        matched_rows: matched,
        duration,
        bytes_read,
    })
}

/// Path B: predicate pushdown on the flattened `env`/`region`/etc. column.
///
/// Uses `ArrowPredicateFn` (parquet 55's concrete predicate impl) to filter
/// rows during decoding. Null values in the target column are treated as
/// non-matching (false).
pub fn label_filter_via_flat(
    path: &Path,
    column: &str,
    needle: &str,
) -> anyhow::Result<QueryStats> {
    let file = File::open(path)?;
    let start = Instant::now();

    // Clone needle into an owned String so it can be moved into the closure,
    // which must be 'static (required by ArrowPredicateFn).
    let needle_owned = needle.to_owned();

    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;

    // The `env` (and similar flat) columns are top-level, so their root index
    // in the parquet schema matches their Arrow schema index.
    let col_root_idx = builder.schema().index_of(column)?;
    let predicate_mask = ProjectionMask::roots(builder.parquet_schema(), [col_root_idx]);

    // Projection mask for the final output: we want all columns.
    // (No with_projection call → default is all columns.)

    // Build the predicate. The closure receives a RecordBatch containing only
    // the projected column (the flat label column). It returns a BooleanArray:
    // true  → keep the row
    // false → skip the row
    // null  → skip the row (parquet treats null as non-matching)
    let predicate = ArrowPredicateFn::new(predicate_mask, move |batch| {
        // column(0) because the projection mask contains exactly one column.
        let arr = batch.column(0);
        let strings = arr
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("flat label column is UTF8");
        let bools: BooleanArray = (0..strings.len())
            .map(|i| {
                if strings.is_null(i) {
                    Some(false)
                } else {
                    Some(strings.value(i) == needle_owned)
                }
            })
            .collect();
        Ok(bools)
    });

    let row_filter = RowFilter::new(vec![Box::new(predicate)]);
    let reader = builder.with_row_filter(row_filter).build()?;

    // Because the filter runs inside the reader, only matching rows are
    // returned in each batch. scanned == matched from our perspective
    // (we count what comes out).
    let mut scanned = 0u64;
    let mut matched = 0u64;
    for batch in reader {
        let batch = batch?;
        let n = batch.num_rows() as u64;
        scanned += n;
        matched += n;
    }
    let duration = start.elapsed();
    let bytes_read = std::fs::metadata(path)?.len();
    Ok(QueryStats {
        scanned_rows: scanned,
        matched_rows: matched,
        duration,
        bytes_read,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow::write_parquet;
    use crate::gen::generate;

    #[test]
    fn map_and_flat_filters_agree_on_count() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agree.parquet");
        let records = generate(1, 5_000);
        write_parquet(&records, &path).unwrap();

        let a = label_filter_via_map(&path, "env", "v0").unwrap();
        let b = label_filter_via_flat(&path, "env", "v0").unwrap();
        assert_eq!(a.matched_rows, b.matched_rows);
    }

    #[test]
    fn filters_find_expected_values() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hits.parquet");
        let records = generate(7, 10_000);
        write_parquet(&records, &path).unwrap();

        let stats = label_filter_via_map(&path, "env", "v0").unwrap();
        assert!(stats.matched_rows > 0);
        assert!(stats.scanned_rows == 10_000);
    }
}
