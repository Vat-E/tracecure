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

## Config is not reachable by operators — except retention

`StoreConfig` exists and lets tests lower thresholds. The server builds a
default one and sets exactly one field from its command line: `retention`
(`--retention`, `--retention-stream`, or the `TRACECURE_RETENTION*`
variables). Everything else — fsync interval, flush thresholds, partition
granularity, compaction tick — has no config file, env var or flag. Do not
document those constants as tunable without adding a real plumbing path.

## Retention deletes data; keep it boring

[ADR-003](../../docs/decisions/ADR-003-m1-retention.md) records the design.
The parts that are easy to break:

- **Off means off.** `RetentionConfig::default()` has no rules and must
  delete nothing. Never give it a non-empty default.
- **`fully_expired` needs a default rule.** Without one, a stream with no
  rule is kept forever and could be in any segment, so a segment may only be
  deleted unread when *every* rule — the longest included — has passed.
- **Partition-aligned, on purpose.** Expiry is judged against the partition's
  end, not each record's timestamp. Making it exact brings back a rewrite of
  every straddling partition on every tick.
- **A clean single segment is not rewritten.** `rewrite_partition` returns
  early when there is one input and nothing to drop. That early return is
  what keeps a restart from rewriting the whole store; the `pruned` map it
  repopulates is in memory only.
- **Catalog first, files second.** `replace_segments` swaps the handle list
  under the write lock before deleting files. `Store::search` retries when a
  file it snapshotted has since gone; do not turn that retry into an error.

## Search ordering is part of the wire contract

`search()` sorts **newest-first** (`b.timestamp.cmp(&a.timestamp)`) and then
truncates to `limit` — so `limit` keeps the newest N, not the first N.
`from` is inclusive, `to` is exclusive. The web UI reverses for display and
depends on this ordering; `docs/book/src/integration.md` documents it. Changing it
breaks both.

## A search reads as little as it can — keep the two halves in step

`Store::search_once` visits segments newest-first and stops when the next
one cannot hold anything newer than what it has; `segment::scan_segment_newest`
projects only the columns the filter constrains, then decodes just the rows
it will return. Two things follow:

- **A new filter field needs a new projected column.** `Columns::accepted_by`
  reads an unprojected column as empty. Add a constraint to `SearchFilter`
  (or a `Field` to `tracecure-query`) without adding its column to
  `columns_read_by` and the filter silently
  rejects (or accepts) everything. `newest_agrees_with_a_full_scan_for_every_kind_of_filter`
  is the test that catches it — extend its filter list.
- **`scanned` comes from segment footers**, not from rows read, so the
  shortcut does not change it. Keep it that way; the UI shows it.

`Store::search_page` is `search_filtered` plus a second, one-microsecond-wide
search for the records sharing the page's last timestamp. That is what lets
the cursor be a bare timestamp; do not "optimise" it into an offset or a
row position — compaction rewrites segments between two requests
([ADR-006](../../docs/decisions/ADR-006-tracecureql-without-datafusion.md)).

The early stop is only valid because `limit` means "newest N". Any other
ordering needs a different stopping rule.

Measure changes here with `cargo bench -p tracecure-storage` (see README).

## Crash-safety invariants

- WAL frames are length-prefix + `serde_json` body + CRC32 **over the body
  only**. A partial trailing frame is tolerated on replay (torn final write).
- A memtable flush must `fsync_through_current()` before publishing a segment,
  so no segment is ever visible ahead of its WAL.
- Two orphan sweeps run at open: `*.compacting.parquet` leftovers and stray
  WAL logs. Keep both if you touch startup.

There is no catalog database — `PLAN.md` §4.2 sketches SQLite, but the
implementation scans the segment directory instead.
