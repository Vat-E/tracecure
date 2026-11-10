# ADR-006: TracecureQL runs on the storage scan, not on DataFusion; cursors are timestamps

**Status:** accepted — in force
**Date:** 2026-11-10
**Relates to:** PLAN.md §4.3 ("Query") and §7 (M2). Changes §4.3's "TracecureQL parses to a logical plan that lowers into DataFusion" and its cursor shape. The SQL endpoint, and DataFusion with it, is not decided here.

## Context

PLAN.md §4.3 gives TracecureQL and SQL one execution engine: both lower into DataFusion, through a TableProvider over segments. §8 already hedged that — "if it underperforms, we keep DataFusion for SQL only and hand-roll the TracecureQL executor."

Two things have happened since. The M1 benches (see `crates/tracecure-storage/README.md`) led to a segment scan that answers "the newest N records matching these tests" in tens of milliseconds over ten million records, by reading segments newest-first, stopping early, and projecting only the columns a filter looks at. And what TracecureQL needs to express today — tests on a record's columns, body and labels, all of which must hold — is exactly what that scan evaluates.

So the question is no longer whether a hand-rolled executor can keep up. It is whether to put a query planner, and a dependency the size of DataFusion, between a filter language and a scan that already runs it.

## Decision

**1. A TracecureQL query is a conjunction of clauses, and the storage scan runs it directly.**
`tracecure-query` parses the text into `Clause { field, test }` values and nothing else. `tracecure-storage` carries them in `SearchFilter::clauses` and evaluates them in the same pass, on the same projected columns, as the `/v1/search` filters. There is no plan, no optimiser and no second engine.

**2. The language, for now, is filtering only.**

```
{app="api", env="prod"} | level="error,fatal" | body ~ "timeout" | host!="edge-9"
```

A selector in braces, then `|`-separated stages; both are lists of `key op "value"`. `app`, `host`, `stream`, `level` and `body` name the record's columns, any other key a label. `=` and `!=` take comma-separated alternatives, with a trailing `*` for a prefix; `~` and `!~` are substring tests. A missing label fails `=` and `~` and passes `!=` and `!~`.

Not in it: `or` between clauses, regular expressions, tests on `fields`, and anything that aggregates — the plan's `rate(5m)`. Those are where a real engine earns its place, and they are deferred to the SQL work rather than half-built here.

**3. `tracecure-query` depends on `tracecure-core` only.**
It is compiled into the browser UI as well as the server, so the query box filters the loaded buffer and the live tail with the very code the server runs. That rules out storage, tokio or arrow as dependencies, and is the practical reason the parser and the executor live in different crates.

**4. Labels become filterable, by scanning.**
A label clause adds the `labels` map column to the scan's projection and looks the key up row by row. No index. ADR-002 deferred a label index until the scan was shown to be too slow; this is the first code that can show it, and the benches are there to measure with.

**5. A cursor is a timestamp, and a page never splits one.**
PLAN.md specified `(timestamp_desc, shard_id, ingest_seq)`. There is one shard and records carry no sequence number; inventing one would mean a format change. Instead `/v1/query` returns `next_cursor` meaning "everything older than this instant", and a page that would end among records sharing a timestamp is extended to include all of them (up to 10,000). Nothing is skipped or repeated at a boundary, and the cursor does not depend on how records are laid out in segments — which compaction is free to change between two requests.

The cursor is documented as opaque, so a shard and a sequence number can be added to it later without breaking a client.

**6. `/v1/query` is new; `/v1/search` is unchanged.**
The query-string filters of `/v1/search` stay as they are for existing clients. `/v1/query` takes the language and returns pages. A query that does not parse is a `400` whose body says where.

## What this costs

- **A page boundary is two scans.** A full page triggers a second, one-microsecond-wide search for the records sharing its last timestamp. It reads the same columns as the first; on the 10M corpus that is tens of milliseconds, not seconds.
- **More than 10,000 records in one microsecond** lose the excess at a page boundary. Real logs do not do this; a bulk import with a constant timestamp could.
- **Two filter syntaxes in one box.** The UI's `level:error app:billing text` form stays, because it is what one types quickly; input starting with `{` or `|` is TracecureQL. They are told apart by the first character.
- **SQL will be a second engine after all**, if it is built on DataFusion. That was the plan's stated fallback; this ADR takes it deliberately rather than by underperformance.

## Consequences

- PLAN.md's M2 line "TracecureQL parser → logical plan" is done without the logical plan. Still open in M2: the SQL endpoint, and with it the DataFusion TableProvider.
- A new filter field needs three things kept in step: the parser's key table, `Clause::matches` for records, and `columns_read_by` / `Columns::accepted_by` in the segment scan. `newest_agrees_with_a_full_scan_for_every_kind_of_filter` checks the last two against each other.
