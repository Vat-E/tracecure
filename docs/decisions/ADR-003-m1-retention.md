# ADR-003: M1 retention — per-stream maximum age, partition-aligned, no catalog

**Status:** accepted — in force
**Date:** 2026-11-10
**Relates to:** PLAN.md §4.2 ("Retention", "Compaction", "Catalog"). Departs from it in two places, both recorded below.

## Context

M1 called for "retention by time", and PLAN.md §4.2 says how: configured per `stream_id`, enforced by the compactor, with a SQLite catalog tracking "segments, retention, compaction state".

By the time this was built the store had a real instance behind it, fed continuously by a log shipper, with no way to delete anything. That made two things concrete that the plan left open:

- A segment holds every stream that logged during its partition. Per-stream retention therefore cannot be "delete old files"; it means rewriting a segment without some of its records.
- There is no catalog. M1 scans the segments directory at open and keeps the list in memory (`store.rs`). Nothing persists a fact *about* a segment other than the segment itself.

## Decision

**1. A rule is a maximum age. One optional default, plus per-stream overrides.**
`RetentionConfig { default: Option<Duration>, per_stream: BTreeMap<String, Duration> }`. A stream with its own rule ignores the default, in either direction — shorter to shed a noisy stream, longer to keep an important one. With no rules, nothing is deleted. That is the default: a store must not drop data it was not told to drop.

**2. Retention is partition-aligned.**
A stream's records are deleted from a partition when the *whole partition* is older than the stream's rule — not record by record as each one ages out.

The alternative, exact per-record expiry, means every partition straddling a cutoff is rewritten on every compactor tick until it has fully aged out: an hour-wide partition, a five-minute tick, twelve rewrites of the same data to delete it in twelfths. Aligned, each partition is rewritten at most once per rule. The cost is that data outlives its rule by up to one partition (one hour by default) plus the compactor's grace (30 min) and tick (5 min). For rules measured in days, that error is noise; it is documented wherever the rule is.

**3. Enforcement is part of the compaction pass, not a second pass.**
Merging small segments and pruning expired records both end in "replace a partition's segments with one rewritten segment, or none". Done together, a partition that needs both is read and written once. A partition past *every* rule (which requires a default rule — otherwise an unruled stream could be inside) is deleted without being read.

**4. What has been pruned is remembered in memory, not persisted.**
Whether a segment still holds records of an expired stream cannot be known without reading it. The compactor keeps a map from segment path to the set of rules it was last rewritten for; a segment whose entry matches the rules currently expired for its partition is skipped without I/O.

The map is lost on restart. Each old segment is then read once, found to need nothing, and recorded again — and, specifically, **not rewritten**: a single segment with nothing to drop is left as the file it is. So a restart costs one read pass over old partitions, not a rewrite of the store.

**5. Configured on the server's command line.**
`--retention <duration>` and repeatable `--retention-stream <stream_id>=<duration>`, with `TRACECURE_RETENTION` / `TRACECURE_RETENTION_STREAMS` as environment fallbacks for containers. Durations are a whole number and a unit (`s m h d w`). An unparseable value, or the same stream given twice, stops startup. The effective rules are logged at startup.

This is the first storage setting an operator can reach. `StoreConfig`'s other fields remain compile-time defaults (see the storage README); retention is different in kind — it is policy about the operator's data, not tuning.

## Departures from PLAN.md

- **No SQLite catalog.** The plan has the catalog tracking retention state. Decision 4 replaces that with an in-memory map plus a cheap re-derivation. A catalog remains the right answer when segment counts make the open-time directory scan or the post-restart read pass slow; neither is true yet, and adding the one embedded dependency the plan allows *for this alone* would be building it ahead of its need.
- **Rules live in process configuration, not in stored per-stream metadata.** The plan implies retention is a property of a stream that the system stores. There is no stream registry to store it in — `stream_id` is a free-form string on each record. Until one exists, the rules are startup configuration, and changing them means a restart.

## Consequences

- Deleted is deleted. There is no tombstone, trash or undo; a too-short rule is unrecoverable for the partitions it has already reached. Hence: off by default, stated at startup, and validated before the store opens.
- Shortening a rule takes effect at the next tick. Lengthening one cannot bring back what was already deleted.
- Records still in the memtable or the WAL are untouched until they are flushed into a segment. A late-arriving record with an old timestamp is therefore searchable until its segment is next compacted.
- Search does not filter by retention. Between a record passing its rule and the compactor reaching its partition, it is still returned.
- The compactor now deletes files in normal operation, not only as merge inputs. `Store::search` snapshots the segment list and scans without a lock, so it can find a file gone; it now retries against the fresh list rather than failing the request. (The same race already existed for merges and was simply rarer.)
- Per-stream rules make the common case — one default, a few noisy streams with shorter rules — cost one extra rewrite of each partition, when the shortest rule passes it.

## Not decided here

- Size-based retention ("keep at most N GB") and plugin-driven lifecycle policies — PLAN.md lists both as later.
- A way to change rules without a restart, or to see them from the API or the UI.
- Deleting specific records on request.
