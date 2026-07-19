# ADR-002: Revise M1 writer knobs — no bloom filters by default, plan a secondary label index

**Status:** accepted — in force; these are the M1 writer defaults
**Date:** 2026-04-23 (same day as ADR-001)
**Amends:** [ADR-001](ADR-001-m1-storage-format.md) — supersedes its row-group-size and bloom-filter decisions and retracts its follow-up action 1 (the gating 100 M-row spike). The rest of ADR-001 — parquet 55, zstd 3, `Map<String,String>` labels, `fields_json` — stands unchanged.
**Evidence:** [`docs/m1-format-fit-report.md`](../m1-format-fit-report.md) — "Spike v2" section.

## Context

ADR-001 accepted `arrow-rs` + `parquet` 55 as the M1 segment format and recommended three knob adjustments to address the observation that a flattened `env` column was 4× slower than a Map-column scan:

1. Bloom filters on `timestamp`, `stream_id`, `host`.
2. 256 K-row row groups (vs the spike's 8 K default).
3. A "second spike at 100 M rows" before committing to compaction design.

A same-day follow-up spike (spike v2) tested knobs 1 and 2 at 10 M rows. The results invalidated the recommendation.

## Measured (10 M records, same corpus as spike v1)

| Metric | Spike v1 (defaults) | Spike v2 (ADR-001 knobs) |
|---|---|---|
| Row groups | 1,221 | 39 |
| Bytes on disk | 631 MiB | **1,225 MiB** (1.94× larger) |
| Write amp | 0.20× | 0.39× |
| Write throughput | 400 k/s | 329 k/s |
| Map-column filter | 1,413 ms | 1,497 ms |
| Flat-column filter | 5,544 ms | 5,363 ms |

The file almost doubled in size, write throughput dropped 18 %, and **neither filter path got faster**.

## Decision

Update the M1 writer defaults from ADR-001 to:

1. **No bloom filters in the default configuration.** The spike showed that bloom filters at the parquet default `ndv` (distinct-value count) cost ~600 MB on a 631 MB file and gave zero query-time benefit for a 2 %-selectivity label query. Bloom filters help point lookups; they don't help label-value matching.

   Future revisit criterion: a real user query with >99 % selectivity (e.g. `request_id = "…"`). At that point, enable a bloom filter **on that specific column** with an explicit `ndv` matching the column's actual distinct count.

2. **Row group size stays near the `arrow-rs` default.** Spike v2 went from 1,221 row groups to 39 with no measurable query-time change. The cost isn't in row-group metadata; it's in columnar decode, which is linear in row count regardless of grouping. Set `max_row_group_size = 1_000_000` (close to parquet's own default of 1,048,576) and stop worrying about it.

3. **Accept the Map-column scan as the M1 label-filter path.** ~1.4 s for a 10 M-row schema-flexible label filter is fast enough for ad-hoc queries. Extrapolated linearly, 100 M rows = ~15 s, 1 B rows = ~150 s. That's the M1 ceiling for the "give me all records with `env=prod`" query shape.

4. **Plan a secondary per-segment label index for M3 (or earlier if UI queries demand it).** Shape: sparse `(label_key, label_value) → row_ids` inverted list per segment. This is the standard approach; tantivy's posting-list format is a reasonable reference. The text-index sidecar pattern from `PLAN.md` §4.2 applies here too — separate file, opt-out per stream, index-only eviction cheap.

5. **Revise the 100 M spike gate.** ADR-001 gated compaction design on a 100 M-row spike with the above knobs. Since the knobs are retracted, the gate is also retracted. We can start `tracecure-storage` scaffolding immediately with the simpler defaults and the Map-column path.

## Alternatives considered

### A. Keep ADR-001's knobs and set explicit `ndv`
Would cut bloom-filter size to something reasonable, but still wouldn't help the `env=v0` query (medium selectivity). Filed under "revisit when we have a high-selectivity column".

### B. Skip parquet and use a bespoke format optimised for label lookups
Rejected for the same reasons as ADR-001 alternative A — the ecosystem cost outweighs the theoretical speedup, especially since the Map-column scan is already fast enough.

### C. Always pair every segment with an inverted label index
Promising but adds implementation cost before we have evidence it matters. Plan it for M3 (or when a user query shows the need); not M1.

## Consequences

**Easier:**

- M1 writer is simpler: no bloom-filter tuning, no explicit row-group plumbing, no flatten logic.
- File size stays tight: 0.20× write amp = 5× more retention per TB than initially feared.
- Compaction design can start immediately; no gating 100 M spike.

**Harder:**

- No "free" interactive label lookup for M1. Users doing repeated label filters will hit the ~1.4 s/10 M scan time every time. If this hurts in practice before M3 lands, we build the label index early.
- We've consumed two ADR slots on storage format before writing M1 code. Worth it — the findings saved weeks of building on top of wrong defaults — but worth noting the overhead.

## References

- [ADR-001](ADR-001-m1-storage-format.md) — the decision this one amends.
- [`docs/m1-format-fit-report.md`](../m1-format-fit-report.md) — spike v1 + v2 data.
- [`bench/format-fit/`](../../bench/format-fit/) — the spike harness, with `--batch-rows` and `--bloom` flags added for v2.
