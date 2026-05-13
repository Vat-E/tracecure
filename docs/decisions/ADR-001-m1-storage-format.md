# ADR-001: Adopt `arrow-rs` + `parquet` for M1 segments (with Map-column labels)

**Status:** accepted
**Date:** 2026-04-23
**Evidence:** [`docs/m1-format-fit-report.md`](../m1-format-fit-report.md)
**Mirrored in vault:** `Vault/Sift/20-Decisions/ADR-001 M1 Storage Format.md`

## Context

M1 needs a column-oriented on-disk format for immutable segments — the storage layer `PLAN.md` §4.2 sketches as WAL + memtable + segmented Arrow/Parquet files with zstd compression. Before committing to that design, we ran the format-fit spike described in `docs/superpowers/plans/2026-04-23-m1-format-fit-spike.md` on 10 million synthetic records to measure write amplification, footer overhead, write throughput, and label-filter query time.

The spike also tested a secondary hypothesis from the plan: that flattening high-value label keys (`env`, `region`, `tenant`) into top-level columns would give a large speedup versus a schema-flexible `Map<String,String>` labels column.

## Decision

Adopt `arrow-rs` + `parquet` 55 as the M1 segment format. Concrete configuration:

- **Compression:** zstd level 3.
- **Labels column:** keep as `Map<String,String>`. Do **not** flatten by default.
- **Row group size:** target 256 K rows (vs the spike's default ~8 K). This is the single largest identified source of per-row-group overhead at query time.
- **Bloom filters:** enable for `timestamp`, `stream_id`, and `host`. Skip for `labels` in M1; revisit when the UI gives us evidence of which keys the humans actually filter on.
- **Fields column:** serialize as a JSON-string blob (`fields_json`) in M1. Nested Arrow types are a post-M2 optimisation.
- **Segment format versioning:** write a 4-byte magic + 1-byte version byte into custom parquet key-value metadata so the `tracecure migrate` tool introduced by PLAN §8 can tell v1 from v2.

## Alternatives considered

### A. Custom columnar format from scratch
Rejected. Gives us nothing parquet doesn't already provide, costs us ecosystem interop (Polars, DataFusion, `arrow-rs` analytics tooling).

### B. Flatten `env` / `region` / `tenant` into top-level columns by default
Rejected based on the spike. With current parquet writer defaults, predicate pushdown on a flat string column was **4× slower** than a loop scan over the Map column at 10 M rows (5.5 s vs 1.4 s) — the opposite of what PLAN §4.2 implied. The effect persists at 100 K scale (0.3× speedup in both directions), so it is not a small-scale noise artefact. Once the row-group and bloom-filter adjustments in this ADR land we should re-measure, but flattening is no longer a default.

### C. Raw Arrow IPC files (no Parquet)
Rejected. Smaller on-disk only if we add our own dictionary encoding, and we lose parquet's built-in predicate pushdown / column pruning that DataFusion will lean on at M2.

### D. Lance / ORC / other formats
Rejected without benchmarking. The ecosystem gravity around `arrow-rs + parquet` is strong enough that pivoting away needs a much stronger signal than any we've seen.

## Consequences

**Easier:**

- M1's segment builder, compaction, and retention pruning can proceed with the design from PLAN §4.2 — no format pivot needed.
- DataFusion integration (M2) is free-ish: parquet already has a `TableProvider`.
- Write-side performance has a lot of headroom: 400 K records/s measured, target was 50 K.
- Write amplification (0.20×) gives retention-per-TB numbers 5× better than the plan assumed.

**Harder:**

- The "promote hot labels to columns" intuition from PLAN §4.2 doesn't pay off with the straightforward approach. If M2's TracecureQL shows real user queries dominated by label filters, we will need either:
  - A secondary per-segment inverted index on `(label_key, label_value) → row_ids` (revisits M3's text-index sidecar pattern applied to labels), or
  - Bloom filters on specific label values once we know which ones matter, or
  - A format-v2 that flattens labels **after** we've configured row group size and bloom filters correctly and confirmed the pessimisation has flipped.

- The spike used default row group size and no bloom filters; the above adjustments are planned but **not yet measured at 100 M row scale**. We are one spike away from a surprise.

## Follow-up actions (before M1 design freeze)

1. **Second spike at 100 M rows** with the row-group / bloom-filter knobs from this ADR, same corpus shape, same query. Acceptance criterion: Map-column label filter ≤ 1 s. If it fails, revisit this ADR before compaction design.
2. **Update PLAN.md §4.2** to reflect that labels stay as a Map column and the flatten-by-default strategy is retracted. (Next commit on this branch.)
3. **M1 Issue tracker:** open an item for the second spike so it's not silently skipped.

## References

- [`docs/m1-format-fit-report.md`](../m1-format-fit-report.md) — measured numbers.
- [`bench/format-fit/`](../../bench/format-fit/) — the spike itself.
- `PLAN.md` §4.2 — original design intent.
- `docs/superpowers/plans/2026-04-23-m1-format-fit-spike.md` — the spike plan + acceptance thresholds.
