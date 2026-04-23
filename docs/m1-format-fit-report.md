# M1 Format-Fit Report

**Spec:** [`docs/superpowers/plans/2026-04-23-m1-format-fit-spike.md`](superpowers/plans/2026-04-23-m1-format-fit-spike.md)
**Date:** 2026-04-23
**Hardware:** AMD Ryzen 7 5800X, 8 cores / 16 threads, 32 GiB RAM, Windows 11 (NVMe SSD assumed from build times)
**Rust:** 1.94.1 (e408947bf 2026-03-25)
**Crates:** `arrow = 55.2.0`, `parquet = 55.2.0` (plan specified `53`; crates.io current was `55`), zstd level 3, row group chunks of 8,192 rows

## Corpus

- **10,000,000** synthetic records, seed `42`.
- Generator: [`bench/format-fit/src/gen.rs`](../bench/format-fit/src/gen.rs). 500 hosts, 30 apps, 80 stream_ids, 20-key label pool (values `v0..v11` — so `env=v0` matches roughly 1/12 of records), 16-key field pool.
- Logical (pre-serialization) size: **3.06 GiB** (sum of host+app+stream_id+body+labels+fields_json strings + ~30 B fixed overhead per row).

## Results

| Metric | Value |
|---|---|
| Generate time | 25.68 s |
| Write time | **25.00 s** |
| Write throughput (records/s) | **399,941** |
| Write throughput (MiB/s in / out) | 125.5 / 25.3 |
| Bytes on disk | **631.42 MiB** |
| Write amplification (disk / logical) | **0.20×** |
| Row groups written | 1,221 |
| Footer size | **19.36 KiB** (0.00% of file) |
| Label filter `env=v0` via Map column | **1,412.77 ms** (188,401 matches, 5-iter avg) |
| Label filter `env=v0` via flattened column | **5,544.32 ms** (188,401 matches, 5-iter avg) |
| Flattened "speedup" over map | **0.3×** (flat is 4× **slower**) |

Both filter paths agree on `matched_rows = 188,401` → the correctness contract from the query tests holds at scale.

## Assessment against acceptance thresholds

| Criterion | Threshold | Actual | Pass? |
|---|---|---|---|
| Write amplification ≤ 0.6× | ≤ 0.6 | **0.20** | ✅ (67% under budget) |
| Footer size ≤ 2% of file | ≤ 2% | **0.00%** (19 KiB / 631 MiB) | ✅ |
| Write throughput ≥ 50k rec/s | ≥ 50,000 | **399,941** | ✅ (8× over target) |
| Flattened label filter ≤ 500 ms on 10M rows | ≤ 500 ms | **5,544 ms** | ❌ (11× over) |
| Map-column label filter ≤ 5 s on 10M rows | ≤ 5 s | **1,413 ms** | ✅ |

## The surprise

The spec expected flattened top-level columns to beat Map-column scans by some multiple — ideally enough to warrant promoting high-value labels to first-class columns. **They don't.** The flat path was 4× slower at 10M records and held the same 0.3× ratio at 100K records, so the result is not a small-scale anomaly.

Three likely causes worth investigating before committing more engineering to this direction:

1. **Row group fanout.** The write produced 1,221 row groups (8,192-row batches → ~1,221 for 10M). Each row group has its own column chunk for `env`. Opening 1,221 tiny column chunks adds per-chunk metadata overhead that overwhelms the "only read one small column" advantage. Larger row groups (256K or 1M rows) would probably invert this.

2. **No bloom filter / dictionary pushdown configured.** The writer used default column properties. Parquet's predicate pushdown without a bloom filter falls back to min/max statistics, and for a uniformly-distributed 12-value string column min/max says "yes, `v0` is in `[v0,v9]`" for every row group — zero pruning. Enabling a bloom filter on `env` would likely give dramatic improvement here, at ~1% file-size cost.

3. **`ArrowPredicateFn` per-batch overhead.** The predicate runs per-batch and the arrow-rs implementation re-projects and allocates a `BooleanArray` each time. For 1,221 row groups × batches, that overhead is real.

The Map path wins today because it has a flat loop over a single column — no predicate machinery, no per-row-group setup.

## Decision

**Go — adopt `arrow-rs` + `parquet` for M1 segments, but revise the label-strategy.**

Write-side numbers pass with huge margins. Map-column query time (1.4 s on 10M rows) is comfortably inside the "ad-hoc schema-flexible query" budget. The `env=v0` case (`matched_rows = 188,401`, ~2% selectivity) is exactly the scan-heavy shape we care about.

**Design adjustments versus the plan's original intent:**

1. **Do NOT flatten `env`/`region`/`tenant` into top-level columns by default.** The spike data shows this is a pessimisation, not an optimisation, with current parquet writer defaults. Keep labels schema-flexible in the `labels: Map<String,String>` column.

2. **Larger row groups for the M1 writer.** Pick a target of 256K rows / row group (default Parquet is 1M; arrow-rs default is much smaller). This should dramatically shrink row group count and amortise per-chunk overhead.

3. **Bloom filters on high-cardinality hot columns.** For M1, enable bloom filters on `timestamp` (range queries), `stream_id` (stream isolation), and `host` (ops drill-down). Skip on `labels` for now — bloom on Map values isn't well-supported. Revisit when we know which label keys the UI actually filters on.

4. **Re-measure before trusting this decision.** Before compaction is built on top of this format, run a follow-up spike with the above three knobs (larger row groups + bloom on `stream_id`/`host` + the current schema) and confirm the Map filter stays under ~1 s at 100M rows. If it does, proceed. If not, consider a secondary per-segment label index (inverted posting list keyed on `(label_key, label_value)` → row IDs).

## Follow-up actions

- [ ] Task 7: write ADR-001 "Adopt arrow-rs + parquet for M1 segments with Map-column labels" reflecting the decision + the three design adjustments above.
- [ ] File an M1 issue: "spike #2 — verify bloom filter + large row group assumptions at 100M rows" to block compaction design.
- [ ] Delete `./target/format-fit/` after the numbers are captured (630+ MB).

## Reproduce

```
cargo run --release --manifest-path bench/format-fit/Cargo.toml -- \
  --records 10000000 \
  --output ./target/format-fit/corpus.parquet \
  --seed 42
```

Runtime on this hardware: ~60 s total (25 s generate + 25 s write + ~10 s × 5 query iters × 2 paths averaged).
