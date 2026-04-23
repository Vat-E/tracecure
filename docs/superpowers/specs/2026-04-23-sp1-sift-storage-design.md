# SP1 — `sift-storage` MVP Design

**Status:** accepted design, awaiting implementation plan
**Date:** 2026-04-23
**Sub-project of:** M1 (real storage; see [`PLAN.md`](../../../PLAN.md) §7 and [[ADR-001]] / [[ADR-002]])
**Successor to:** M0's throwaway NDJSON `sift_server::Store`

## Goal

Build the minimum vertical slice of durable, queryable, crash-recoverable storage: a WAL that takes writes, a memtable that holds them for serving, and an immutable parquet-backed segment set they flush to. Drop-in replacement for the M0 `Store` surface, so the cutover (SP2) is a one-line swap in `sift-server`.

**Explicit non-goals for SP1** (owned by later sub-projects):

- Time-partitioned segment layout (SP3)
- Background compaction (SP3)
- Retention / eviction (SP4)
- SQLite catalog — SP1 uses the filesystem as its catalog (SP4 migrates)
- Bloom filters / row-group tuning — disabled per [[ADR-002]]
- Label index / inverted postings (SP5 or later)
- Multi-shard / multi-tenant isolation
- OTLP / syslog ingest — still goes through the REST API in `sift-server`

## Scope

New crate `crates/sift-storage/` with one public type, `Store`, exposing exactly the three methods `sift-server`'s current NDJSON store exposes:

```rust
pub struct Store { /* … */ }

impl Store {
    pub async fn open(data_dir: impl AsRef<Path>) -> anyhow::Result<Self>;
    pub async fn append(&self, record: Record) -> anyhow::Result<()>;
    pub async fn search(&self, query: &str, limit: usize)
        -> anyhow::Result<(Vec<Record>, usize)>;
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Record>;
}
```

Same signatures as `sift_server::Store`. `search` returns `(matches, scanned_total)` where `scanned_total` counts records-shaped rows across memtable + segments (keeps the `SearchResponse::scanned` field honest).

## Architecture

`Store` owns three cooperating pieces, each with a well-defined interface so they can be understood and tested independently.

### 3.1 WAL writer

- Owns a `tokio::fs::File` opened append-only on the active WAL file plus a `Mutex<Vec<u8>>` buffer.
- `append(record)` serializes the record (see §4.1 on frame format) and appends the bytes to the buffer under the mutex.
- A background ticker task wakes every 10 ms. If the buffer is non-empty **or** hits 1 MB (whichever first), the task drains it into the file and calls `file.sync_data().await` before notifying waiters.
- `append` returns as soon as bytes are in the buffer. It does **not** wait for fsync — durability is deferred to the ticker (up to 10 ms window, matching [[ADR-002]]-adjacent PLAN intent).
- On "flush memtable to segment" the memtable publisher waits for the ticker to have fsynced through the last record that went into the memtable before starting the parquet write. This keeps `recover` safe: every record replayable from the WAL is at least in a segment or the memtable we're about to write.

### 3.2 Memtable

- `Mutex<MemtableInner>` where `MemtableInner` holds `records: Vec<Record>`, `approx_bytes: usize`, and `opened_at: Instant`.
- `append(record)` computes a cheap byte estimate (`host.len() + app.len() + body.len() + labels/fields lens + 32` overhead), bumps `approx_bytes`, pushes the record.
- After pushing, `append` checks flush triggers: `approx_bytes >= 256 * 1024 * 1024` OR `opened_at.elapsed() >= 60s`. If either fires, `append` attempts to acquire the flush semaphore (see §3.3).

### 3.3 Flush path

- A `tokio::sync::Semaphore` with **1 permit** controls "at most one flush in progress". This is option C from the design conversation: double-buffered with backpressure.
- When a flush trigger fires inside `append`:
  1. Try to acquire the flush semaphore. If **available**: atomically swap the full memtable with a new empty one (take a `std::mem::replace` under the mutex), spawn the flush task with the swapped-out memtable, return `Ok`.
  2. If **unavailable** (a flush is still running): `append` blocks on the semaphore. The server's `POST /v1/ingest` handler upstream sees this as slow `append` — acceptable in SP1 since we don't have 429-on-backpressure yet. Tests lower the trigger so this path is exercised.
- The flush task:
  1. Waits for the WAL ticker to have fsynced through the high-watermark offset captured when the swap happened.
  2. Opens a new parquet writer at `{data_dir}/segments/{next_seq}.parquet`.
  3. Converts the memtable's `Vec<Record>` to `RecordBatch`es. The Arrow schema + `records_to_batch` function is ported from `bench/format-fit/src/arrow.rs` into a new `sift-storage::arrow` module — SP1 copies the code rather than adding a workspace dependency from the bench crate onto `sift-storage`. De-duplication is a small follow-up; it's not on the critical path.
  4. Writes, closes, flushes to disk (`File::sync_all`).
  5. Appends a new `SegmentHandle` to `segment_set`.
  6. Rolls the WAL: writes a new `wal/{next_seq}.log`, updates `wal/current`, deletes the previous WAL.
  7. Releases the flush semaphore.

Rotation sequencer: a `u64` counter persisted as the filename prefix (`0000000001.parquet`, `0000000002.parquet`). On `open`, the max existing segment's seq determines the next number.

### 3.4 Segment set

- `Arc<RwLock<Vec<SegmentHandle>>>`. The lock is short-lived: read-lock to snapshot during `search`, write-lock only when appending a new handle after a flush.
- `SegmentHandle { path: PathBuf, row_count: u64, min_ts: DateTime<Utc>, max_ts: DateTime<Utc> }`. `min_ts` / `max_ts` come from the parquet statistics footer; no separate catalog needed in SP1.

### 3.5 Broadcast

- `tokio::sync::broadcast::Sender<Record>` with capacity 1024 (same as M0).
- `append` calls `tx.send(record.clone())` after bytes are in the WAL buffer (not after fsync — broadcast is best-effort, receivers can miss records anyway on lag; correctness of receivers is the WAL's job).

## Data flow

### 4.1 WAL frame format

Each record is written as one frame:

```
┌────────────┬──────────────────────────────┬─────────────┐
│ len: u32LE │    bincode(record): [u8; N]  │ crc32: u32LE│
└────────────┴──────────────────────────────┴─────────────┘
       4                 N                         4
```

- `len` is the length of the bincode body in bytes (so replay can skip to the next frame).
- `bincode` uses the default `bincode = "1"` config (fixed-size integers, little-endian, no var-int). `Record` already derives `Serialize` + `Deserialize` via serde; bincode rides on serde.
- `crc32` is computed with `crc32fast` over the bincode bytes (NOT over the length prefix — that's ergonomic for partial-read detection).

No file-level header or magic. The first frame's length prefix serves as an implicit format marker; if we bump the frame format in the future, we introduce a header at that point.

### 4.2 Append path

```
append(rec)
  │
  ├─ serialize_frame(rec) → Vec<u8>
  ├─ WAL_BUFFER.lock().extend(frame_bytes)
  ├─ MEMTABLE.lock() → push(rec); update approx_bytes, check triggers
  │     │
  │     └─ trigger? → spawn_flush_if_permit_available()
  │
  ├─ broadcast.send(rec.clone())   // best-effort
  └─ return Ok
```

### 4.3 Search path

```
search(q, limit)
  │
  ├─ mem_snapshot = MEMTABLE.lock().records.clone()            // O(n) copy; n bounded by 256MB memtable
  ├─ seg_snapshot = SEGMENT_SET.read().clone()                 // Vec<SegmentHandle> clone
  │
  ├─ scan mem_snapshot: filter rec.body.contains(q), collect
  ├─ for each segment (parallel via rayon or tokio::spawn_blocking):
  │     ├─ open parquet, read batches in row-group order
  │     ├─ filter by body.contains(q), collect matching records
  │     └─ count scanned rows
  │
  ├─ merge results, sort by timestamp desc, truncate to limit
  └─ return (matches, total_scanned)
```

Note: the flushing memtable (the one that was swapped out but hasn't finished writing yet) is **not** searched in SP1. Records in it are transiently invisible for ~seconds until the segment lands. Fix in SP3.

### 4.4 Restart recovery (`Store::open`)

```
open(data_dir)
  │
  ├─ ensure data_dir/{wal,segments} exist
  ├─ list data_dir/segments/*.parquet
  │     └─ for each: read footer → SegmentHandle → push into segment_set
  │     └─ next_seq = max(handles.seq) + 1
  │
  ├─ read data_dir/wal/current → current_wal_filename
  ├─ open data_dir/wal/<current_wal_filename> for read
  │     └─ while can read frame:
  │           ├─ read 4 bytes → len
  │           ├─ read len bytes → body
  │           ├─ read 4 bytes → crc
  │           ├─ verify crc32(body) == crc; on mismatch: log warn, stop.
  │           ├─ deserialize body → Record
  │           └─ memtable.push_replay(record)        // bypasses size/age triggers
  │     └─ (EOF or partial frame → stop gracefully)
  │
  ├─ if memtable.approx_bytes > 256MB after replay: trigger one flush now
  │                                                  (synchronous, before accepting writes)
  ├─ reopen <current_wal_filename> for append (at end)
  ├─ start ticker task
  └─ return Store
```

A partial trailing frame or bad CRC is treated as EOF, not as an error. A process killed mid-append leaves a possibly-partial final frame; SP1 discards it. A power-loss that corrupts the middle of the WAL is reported as a fatal error on open — we don't try to skip and resume. (Rationale: since the WAL is append-only and we only truncate via full-file replacement on rotation, any corruption that isn't at the tail means filesystem-level damage the user should triage, not a normal operational event.)

Replay uses a separate `push_replay` that skips memtable flush triggers — otherwise replaying a large WAL would trip a flush mid-replay and the WAL rotation logic (§4.5) would fight itself. Instead, after replay finishes, `open` checks the memtable size once: if it exceeds the threshold, it runs exactly one synchronous flush before enabling writes.

### 4.5 WAL rotation (post-flush)

After a successful segment flush, the segment contains every record the WAL holds (by construction: we waited for WAL fsync through the memtable high-watermark). The WAL file is now redundant and can be rotated:

```
rotate_wal()
  │
  ├─ new_path = data_dir/wal/<next_seq>.log
  ├─ create new_path (empty, append-only)
  ├─ atomic_write(data_dir/wal/current, "<next_seq>.log")   // write-to-tmp + rename
  ├─ old_path = previous current
  └─ remove old_path   // safe: new current points away from it
```

Crash during rotation: if `current` points to the new file but the old file wasn't deleted, `open` sees the old WAL as orphaned and deletes it (no records to replay since the handoff was atomic). If `current` still points to the old file, nothing changed — we continue using the old file. No data loss in either case.

## File layout

```
{data_dir}/
├─ wal/
│  ├─ current                # text file, single word: "0000000042.log"
│  └─ 0000000042.log         # the live WAL (append-only)
└─ segments/
   ├─ 0000000001.parquet
   ├─ 0000000002.parquet
   └─ 0000000003.parquet
```

No lock files, no manifest, no catalog.db — filesystem enumerate + parquet footer reads are authoritative. SP4 replaces this with a SQLite catalog.

## Error handling

- **WAL write errors** (disk full, permissions): propagate from `append` as `anyhow::Error`. The server's ingest handler maps to 500.
- **Fsync errors in the ticker:** log at error level, set a `poisoned: AtomicBool` that subsequent `append`s observe and refuse (return a "WAL writer poisoned" error). Recovery requires restarting the process.
- **Parquet write errors in the flush task:** log at error level, do NOT register the handle, retry the flush on next trigger with the same memtable (the semaphore is released). If a second attempt fails, panic — something's broken about the filesystem and the user should restart + inspect.
- **Corrupt segment on open:** log at error level and **skip the segment** (don't register its handle). Users see the gap in search results; they can quarantine the file manually. Fatal rejection felt wrong for a "best-effort durable" M0-plus-one.
- **Corrupt WAL mid-file on open:** fatal. Report the offset, refuse to open.

## Concurrency model

- The `Mutex`es around WAL buffer and memtable are short-held (a serialize + push, not an fsync). Contention is low.
- The flush semaphore (1 permit) bounds parallelism at 1 flush at a time, but N concurrent `append`s can queue up while it's running.
- The broadcast send after WAL buffer append can race the ticker's fsync — that's fine. A subscriber that sees a record before it's fsynced will still have seen the record after the ticker fires; the only divergence is if a crash happens between the broadcast and the fsync, and that's acceptable loss.

## Crate structure

```
crates/sift-storage/
├─ Cargo.toml
└─ src/
   ├─ lib.rs           # pub Store, pub use types
   ├─ store.rs         # Store impl, open/append/search/subscribe
   ├─ wal.rs           # WalWriter + frame encoding + ticker task
   ├─ memtable.rs      # MemtableInner + trigger check
   ├─ segment.rs       # SegmentHandle + parquet write/read
   └─ arrow.rs         # Arrow schema + records_to_batch (hoisted from bench/format-fit)
```

Each file has one responsibility. `store.rs` is the thin orchestrator; real logic lives in `wal.rs`, `memtable.rs`, `segment.rs`. `arrow.rs` is pure data-conversion — no I/O.

## Dependencies

In `crates/sift-storage/Cargo.toml`:

- `sift-core = { workspace = true }` — for `Record`, `Level`
- `tokio = { workspace = true }` — with full features (already in workspace)
- `arrow = "55"`, `parquet = "55"` with `zstd` + `arrow` features — same versions the spike used; add these to `[workspace.dependencies]` so any future crate that needs Arrow shares versions
- `bincode = "1"` — WAL serialization (new workspace dep)
- `crc32fast = "1"` — WAL frame checksum (new workspace dep)
- `anyhow`, `thiserror`, `tracing`, `serde`, `serde_json` — already in workspace

## Testing

### Unit tests

- `wal::tests::frame_roundtrip` — write+read a single frame.
- `wal::tests::crc_mismatch_is_rejected_as_eof` — corrupt the crc byte, replay stops at that frame.
- `wal::tests::partial_frame_at_eof_is_tolerated` — truncate the file mid-frame, replay stops gracefully.
- `memtable::tests::trigger_fires_on_byte_threshold` — small threshold, verify trigger math.
- `memtable::tests::trigger_fires_on_age_threshold` — mock time, verify age-based trigger.
- `segment::tests::filename_sequencer_increments` — given a directory with N segments, next_seq is N+1.

### Integration tests (`tests/` directory, `#[tokio::test]`)

- `append_then_search_returns_record` — happy path, one record.
- `records_survive_reopen` — write 100 records, drop `Store`, reopen, find all 100 via `search`.
- `wal_replay_tolerates_partial_final_frame` — write good records, corrupt final frame, reopen, earlier records survive.
- `flush_happens_under_memtable_pressure` — tunable `StoreConfig { memtable_size_bytes }` lowered to 4 KB, write 10 KB of records, observe a segment file appears and memtable has 0-few records.
- `search_merges_memtable_and_segments` — force one flush, then append more records without flushing, search should find records from both.
- `backpressure_engages_on_second_flush` — tunable config to slow the flush task, push beyond the first flush trigger, observe second `append` awaiting the semaphore.
- `broadcast_delivers_to_live_subscribers` — subscribe, append, receive.
- `concurrent_appends_are_ordered_in_wal` — 4 tokio tasks each appending 100 records; verify WAL replay produces exactly 400 records (order within a task preserved, interleaving between tasks allowed).

Tests bypass the 10 ms ticker by exposing a `StoreConfig::fsync_interval` and `StoreConfig::memtable_size_bytes` in `#[cfg(test)]` or via a crate-private constructor. Production code uses hardcoded defaults.

### Out of scope for SP1 tests

- 256 MB and 10 s scale runs — too slow; owned by SP5's criterion benches.
- Kill-the-process-and-restart tests — hard to do cleanly in a single `cargo test` run; covered by manual runbook + SP4 integration tests.
- Multi-process / fs-lock tests — SP1 assumes one `Store` per data_dir, enforced by convention, not by file locks.

## Migration / cutover note for SP2

SP2 replaces `sift_server::Store` with `sift_storage::Store`. Because the public API is identical, the diff should be:

- `crates/sift-server/Cargo.toml`: replace the ad-hoc file-writer code paths' deps with `sift-storage = { workspace = true }`.
- `crates/sift-server/src/state.rs`: `use sift_storage::Store`.
- `crates/sift-server/src/store.rs`: delete the file (or leave a stub with `#[deprecated]` until a tagged release).
- `crates/sift-server/src/routes/*.rs`: unchanged — they talk to `state.store()` which now returns a `sift_storage::Store`.

All 10 existing `sift-server` integration tests must pass unchanged after the swap. That's the SP2 acceptance test.

## Open questions to resolve before implementation

None. Everything in the design conversation converged.

## References

- [[PLAN.md]] §4.2 — original storage design.
- [[ADR-001]] — adopted arrow-rs + parquet.
- [[ADR-002]] — retracted bloom filters + large row groups from ADR-001; Map-column scan is the M1 label-filter path.
- `bench/format-fit/` — the spike that produced the numbers.
- `crates/sift-server/src/store.rs` — the M0 NDJSON `Store` being replaced.
