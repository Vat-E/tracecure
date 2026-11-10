# Tracecure

Self-hosted, Rust-native log platform. One binary you drop on a box: it
ingests, stores, queries, and serves its own web UI.

Logs are the only first-class citizen. Metrics, traces and alerting are
deliberately out of scope for the core and live behind a plugin API.

## What exists today

Tracecure is at **M1 — real storage**. Concretely:

- **Ingest** — `POST /v1/ingest`, NDJSON, one record per line.
- **Storage** — write-ahead log with 10 ms / 1 MB group-commit fsync, an
  in-memory memtable, and time-partitioned Parquet segments with zstd
  compression. A background compactor merges closed partitions.
- **Query** — `GET /v1/search`, a substring match over `body` with an optional
  time range.
- **Live tail** — `GET /v1/tail`, a WebSocket streaming records as they land.
- **Clients** — a web UI (Rust/WASM), a `tracecure` CLI, and a `tracecure tui`
  terminal UI. All three share the same wire types.

## What does not exist yet

Being explicit, because the roadmap in `PLAN.md` describes much more than is
built:

- **Retention is off by default.** Unless `--retention` is given, nothing is
  deleted and disk use grows without bound. There is no size-based limit.
- **No auth.** Anyone who can reach the port can read and write.
- **No query language.** TracecureQL and the SQL endpoint are M2.
- **No full-text index.** Search is a substring scan; Tantivy is M3.
- **No plugins.** The WASM plugin host is M4.

Treat it as an internal tool on a trusted network.

## Where to go next

- [Install and run](./install.md) — get it running.
- [Operating the service](./operations.md) — the Windows service + IIS
  deployment, logs, and day-2 commands.
- [Integration API](./integration.md) — the HTTP/WebSocket contract, if you are
  shipping logs into it or reading them out.
- [CLI and TUI](./cli.md) — the terminal clients.

For architecture and the milestone ladder, see `PLAN.md` in the repo root.
Design decisions are recorded as ADRs under `docs/decisions/`.
