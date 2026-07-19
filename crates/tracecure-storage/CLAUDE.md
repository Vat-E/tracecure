# tracecure-storage — WAL, memtable, parquet segments

Loaded when working under `crates/tracecure-storage/`. See
`README.md` here for the on-disk layout and the tunable constants; this file
covers the things that bite.

## The format decisions are ADR-governed — read before changing writer knobs

`segment.rs` writes zstd level 3, `max_row_group_size = 1_000_000`, labels as
`Map<String,String>`, fields as a `fields_json` blob, and **no bloom filters**.
Every one of those is a recorded decision:

- [ADR-001](../../docs/decisions/ADR-001-m1-storage-format.md) — adopt parquet;
  keep labels as a Map column (flattening measured 4× *slower*).
- [ADR-002](../../docs/decisions/ADR-002-m1-writer-knobs-revision.md) —
  **retracts** ADR-001's 256K-row-group and bloom-filter items. Turning bloom
  filters on with the parquet default `ndv` nearly **doubled** the file
  (631 MiB → 1,225 MiB) for no query win.

If you are about to enable bloom filters, ADR-002 requires doing it per-column
with an explicit `ndv`. Measure with `bench/format-fit/` first — that harness
exists for exactly this and holds the comparable baseline.

## Config is not reachable by operators

`StoreConfig` exists and lets tests lower thresholds, but the server always
calls `Store::open(data_dir)` → `StoreConfig::default()`. There is no config
file, no env var, and no CLI flag beyond `--data`/`--bind`/`--ui-dist`. Do not
document these constants as tunable without adding a real plumbing path.

## Search ordering is part of the wire contract

`search()` sorts **newest-first** (`b.timestamp.cmp(&a.timestamp)`) and then
truncates to `limit` — so `limit` keeps the newest N, not the first N.
`from` is inclusive, `to` is exclusive. The web UI reverses for display and
depends on this ordering; `docs/book/src/integration.md` documents it. Changing it
breaks both.

## Crash-safety invariants

- WAL frames are length-prefix + `serde_json` body + CRC32 **over the body
  only**. A partial trailing frame is tolerated on replay (torn final write).
- A memtable flush must `fsync_through_current()` before publishing a segment,
  so no segment is ever visible ahead of its WAL.
- Two orphan sweeps run at open: `*.compacting.parquet` leftovers and stray
  WAL logs. Keep both if you touch startup.

There is no catalog database — `PLAN.md` §4.2 sketches SQLite, but the
implementation scans the segment directory instead.
