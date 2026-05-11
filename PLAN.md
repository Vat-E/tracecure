# Tracecure — a self-hosted, Rust-native log platform

Name: **Tracecure**. Think "Mezmo / LogDNA, but a single Rust binary your team drops on a box." Logs are the only first-class citizen; metrics, traces, alerting, and everything else live behind a stable plugin API.

## 1. Goals and non-goals

**Goals**

- Single-binary deployment (`tracecure serve`) that runs on a developer laptop, a bare-metal host, or a VM without external dependencies.
- Ingest ~100k events/sec per node on commodity hardware (8 vCPU, NVMe) with sub-second live-tail latency. Note: this assumes text indexing off or async. With inline Tantivy indexing, realistic is 25–30k/sec.
- Full-text + structured search over compressed on-disk segments, with retention measured in weeks-to-months per TB.
- Plugin-first: anything beyond "ingest, store, query, display" is an out-of-process or in-process WASM plugin.
- Rust across the stack: server, storage, query, and a WASM/Rust frontend (Leptos or Dioxus).
- Internal-tool ergonomics: no multi-tenant billing, no SSO-federation-of-SSOs — just enough auth to keep honest people honest.

**Non-goals (initial releases)**

- Horizontal clustering. A single-node design with read replicas is enough for v1; distributed query comes after the storage format is stable.
- Long-term cold storage tiering (S3, etc.) — can be added as a storage plugin later.
- Being a metrics or tracing backend out of the box. Those land as plugins.
- Kubernetes-native operators, CRDs, helm charts. We'll publish a static binary + a docker-compose file. Anyone who wants a chart can write one.

## 2. Naming notes

Name committed: **Tracecure**. Previously **Sift** — renamed because `sift` is taken on crates.io. Picked over Lumina (collides with `lumina` / `lumina-rs` in the graphics space), Flint/Ember/Scout (all taken by other infra tools), Loupe, Pry, Cairn, Fathom, Spindle. Do a last-pass availability check on crates.io, NPM, and USPTO before cutting the v0.1 crate name; trademark is the one that bites later.

## 3. High-level architecture

```
                       +--------------------+
  agents / syslog ---> |  ingest gateway    |
  HTTP bulk       ---> |  (tokio, axum,     |
  OTLP/logs       ---> |   proto decoders)  |
                       +----------+---------+
                                  |
                           batched RecordBatches
                                  v
                       +--------------------+       +--------------------+
                       |  write-ahead log   | ----> |  segment builder   |
                       |  (sled/rocks/own)  |       |  (arrow + parquet- |
                       +--------------------+       |   ish columnar)    |
                                                    +---------+----------+
                                                              |
                                                              v
                                                    on-disk segments
                                                    (time-partitioned,
                                                     zstd-compressed)
                                                              ^
                        +----------------------+              |
  Web UI (WASM) <- REST/|  query engine        | -------------+
  TUI (ratatui) <- WS   |  (datafusion-based)  |
  CLI           <-      |                      |
                        +----------+-----------+
                                   |
                                   v
                        +----------------------+
                        |  plugin runtime      |
                        |  (wasmtime +         |
                        |   component model)   |
                        +----------------------+
                           |        |        |
                       alerting  metrics  transforms
                       plugin    plugin   plugin
```

Four long-lived subsystems live in one binary: ingest, storage, query, and the plugin host. Three clients share the REST + WebSocket API: a static-WASM web UI served from the same binary on `/`, a `tracecure` CLI for one-shot commands, and a `tracecure tui` interactive terminal UI for live-tail-with-filter in an SSH context. All three types-share via the `tracecure-api` crate.

## 4. Subsystem design

### 4.1 Ingestion

- **Protocols out of the box**: HTTP NDJSON bulk (`POST /v1/ingest`), syslog RFC5424 over TCP/UDP, and OTLP/logs (gRPC + HTTP). Everything else (Fluent Bit forward protocol, Windows Event Log, journald) lives behind an `InputPlugin` trait.
- **Schema**: every record normalizes to `{ timestamp, host, app, stream_id, level, body, labels: map<string,string>, fields: map<string,json>, _raw }`. `stream_id` is assigned at ingest (by token → stream mapping, or by a small rule set) — it is not derived from a label, so renaming `app` never silently moves retention/policy. `fields` is schema-flexible; `labels` is low-cardinality and indexed.
- **Backpressure**: tokio `mpsc` between decoder tasks and the WAL; if the WAL falls behind, the ingest side returns 429 with `Retry-After`. No unbounded buffering ever.
- **Deduplication**: optional per-source dedup window using a rolling bloom filter keyed on `(host, timestamp_nanos, ingest_seq, hash(body))`. Second-resolution timestamps give false collisions at high rates — always use nanos plus the seq.

### 4.2 Storage

The storage layer is the highest-risk subsystem and the one most worth investing in.

- **WAL**: append-only log file per shard, `fsync` group-commit every N ms or N MB, whichever first. Records are framed protobuf with a CRC32 header. On crash, replay into the in-memory memtable.
- **Memtable**: a bounded Arrow `RecordBatch` buffer per shard. Flushed to disk when it hits size or age thresholds.
- **Segments**: immutable, time-partitioned columnar files (1h partitions by default). Each segment has:
  - Column chunks with zstd page compression.
  - Per-column min/max and bloom filters in a footer.
  - A tokenized inverted index for `body`, stored as a **sidecar** file next to the segment (not inline). Sidecar makes per-stream index opt-out and index-only eviction cheap.
  - A manifest row in the catalog (see below).
- **Catalog**: a small SQLite DB tracks segments, retention, compaction state. This is the one embedded dependency we allow; it is not on the hot path.
- **Compaction**: background task merges small segments into larger ones within a time partition, and prunes segments past retention.
- **Retention**: configured per-stream (explicit `stream_id`, not derived from `app`). Time-based v1; size-based and plugin-driven retention later.
- **Segment size target**: 256 MB default, configurable per stream. 128 MB is too chatty for compaction; 512 MB hurts query selectivity.
- **Format risk**: Parquet for log data is unusual. Before M1, write 10M real-ish records through `arrow-rs` + `parquet` and measure write amplification, footer size, and label-only query time. If ugly, reconsider before compaction is built around the format. **Spike completed 2026-04-23 — see [docs/m1-format-fit-report.md](docs/m1-format-fit-report.md) and [ADR-001](docs/decisions/ADR-001-m1-storage-format.md).** Write side passed with big margins (0.20× amp, 400k rec/s, 0.003% footer); decision is to adopt parquet. Labels stay as a `Map<String,String>` column — the flatten-labels-to-top-level-columns intuition turned out to be a pessimisation at default row-group size / no bloom filters. A second spike at 100M rows with larger row groups + bloom filters on `timestamp`/`stream_id`/`host` is gating compaction design.

**Build-vs-buy**: do not write a brand-new columnar format. Sit on top of Apache Arrow + a parquet-ish layout using `arrow-rs` and `parquet`. For the text index, vendor `tantivy`. This saves roughly a person-year and the formats are well-understood.

### 4.3 Query

- **Surface**: two query languages in v1.
  - **TracecureQL** — LogQL-inspired, pipe-separated. Example: `{app="api", env="prod"} | level="error" | body ~ "timeout" | rate(5m)`. This is what humans type in the UI.
  - **SQL** — via DataFusion, for analysts and exports. Every stream is a virtual table.
- **Execution**: TracecureQL parses to a logical plan that lowers into DataFusion. That means one execution engine, two front-ends. DataFusion has pluggable TableProviders — we implement one that reads Tracecure segments and pushes down time/label predicates.
- **Live-tail**: a separate fast path. `GET /v1/tail?query=…` upgrades to websocket; the query planner registers a tailer on the WAL's broadcast channel and streams matching events as they land, bypassing segments entirely.
- **Pagination**: cursor-based on `(timestamp_desc, shard_id, ingest_seq)`. A globally monotonic seq across shards is either contention or lies; encoding the shard into the cursor avoids both.

### 4.4 Plugin runtime

This is the part worth getting right, because your "metrics, alerts, etc. as plugins" decision leans on it heavily.

- **Runtime**: `wasmtime` with the **WASM Component Model** and `wit-bindgen`. Components are language-agnostic; a plugin can be written in Rust, Go (tinygo), JS (componentize-js), or C.
- **Interfaces** (sketched as WIT, stored under `wit/v0/` from day one):
  - `input`: custom ingest sources. Host exports `emit(record)`; plugin owns its own I/O via WASI.
  - `transform`: per-record or per-batch mutators (redaction, field extraction, enrichment). Run in the ingest pipeline.
  - `output`: sinks (forward to another Sift, Kafka, S3, a webhook).
  - `alert`: given a windowed query result, decide whether to fire; host handles delivery via `notifier` plugins.
  - `notifier`: Slack, email, Telegram, PagerDuty, Teams. Each is its own plugin, each ~100 lines.
  - `view`: register a custom UI panel (returns an HTML/JS fragment loaded by the frontend in a sandboxed iframe).
  - `catalog`: custom retention/lifecycle policies.
- **Isolation**: plugins run with a resource budget (fuel limits, memory caps, epoch-based interrupts) and WASI capabilities granted by config. No ambient filesystem or network access; everything is an explicit host import. Supply-chain trust is the operator's responsibility in v1 — document this explicitly; fuel/memory only protect against accidents, not malice.
- **Distribution**: `.wasm` component files. A `plugins/` directory + a `plugins.toml` manifest. `tracecure plugin install <path-or-url>` is a later nicety.
- **Versioning**: interfaces are semver'd in the WIT at the interface level; plugin bundles declare the range they support; the host refuses to load a plugin whose interface-version ceiling is older than the running server's floor.

Getting the boundary right here is the single biggest lever on project longevity. Prototype the `transform` and `notifier` interfaces first — they're small and flush out the plumbing — before the `view` or `alert` ones. Budget M4 at ~8 weeks half-time, not 4: Component Model + wit-bindgen + host-side capability plumbing has more sharp edges than the WIT implies.

### 4.5 Web UI (Rust → WASM)

- **Framework**: **Leptos** (fine-grained reactivity, mature SSR, best-in-class DX right now) or **Dioxus** (React-like, better native story). Pick Leptos for v1 unless you want a native app later.
- **Render mode**: CSR only. Do not couple a Rust web renderer to the server binary for an internal tool.
- **Build**: `trunk` for dev. Tailwind for styling.
- **Pages**:
  - Live tail (default landing).
  - Search / explorer with a TracecureQL editor, time-range picker, and a results table with JSON-field drill-down.
  - Streams view (per-stream retention, volume, ingest rate).
  - Plugins view (installed plugins, their registered panels).
  - Settings / tokens / users.
- **Bundle size caveat**: Rust/WASM frontends are heavier than equivalent React apps. Budget 300–500 KB gzipped and use `wasm-opt -Oz`. Fail CI if gzipped `.wasm` > 600 KB. If the landing page needs to be <100 KB, reconsider the framework choice.

### 4.6 CLI and TUI

Both live inside the `tracecure-cli` crate; the TUI is a subcommand (`tracecure tui`), not a separate binary.

- **CLI one-shot subcommands** (`tracecure send`, `tracecure search`, `tracecure tail`): thin wrappers over the REST / WS API, designed for scripting and one-liners. `tracecure tail` prints records line-by-line and exits cleanly on `Close`.
- **TUI (`tracecure tui`)**: Ratatui + crossterm interactive view. Live-tail with a status pill, client-side substring filter (`/`), and scroll controls (`Up`/`Down`/`PageUp`/`PageDown`/`Home`/`End`). Covers the SSH-into-a-server case where a browser isn't available. Pure state lives in `tui::state::AppState` and is fully unit-testable; terminal lifecycle goes through a `TerminalGuard` that restores raw mode + alt screen on panic.
- **Why a subcommand, not a new binary**: one less artifact to ship, reuses `--server` and other global flags, reuses the same `tracecure-api` types. The three UIs (web / TUI / CLI) end up rendering the same `TailFrame` stream, so wire-shape changes are compile-time breakages rather than runtime drift.
- **Planned expansion** (post-M0, gated by server capabilities): record detail popup (Enter to expand labels / fields / `_raw`), `?` help overlay, level-quick-toggle (`E` / `W` / `I`), streams sidebar (depends on M2), search pane distinct from the live-tail filter (depends on M2+M3). Every TUI feature should also land in the web UI; we accept explicit drift rather than silent divergence.

### 4.7 Auth and RBAC

- Internal-tool scope ⇒ local users + optional OIDC. Skip SAML.
- Passwords hashed with argon2id. JWTs (15 min) + refresh tokens for sessions; HS256 with a rotated server secret is fine for single-node v1. If read replicas land (hinted at in §1), revisit — shared secret across nodes or move to RS256.
- RBAC at the stream level: `{read, tail, write, admin}` per `stream_id`, expressed as a small policy file (`policy.toml`) loaded at boot. No policy UI in v1 — it's an internal tool, editing a file is OK.
- Ingest tokens are separate from user credentials, scoped to one or more streams, rate-limited per token.

## 5. Tech choices (concrete crate list)

| Concern | Choice | Why |
| --- | --- | --- |
| Async runtime | `tokio` | Default; ecosystem weight. |
| HTTP | `axum` + `tower` | Composable, plays well with tonic for gRPC. |
| gRPC (OTLP) | `tonic` | Standard. |
| Columnar format | `arrow-rs` + `parquet` | Don't reinvent; interop with Polars/DataFusion for free. Verify format fit at M1. |
| SQL engine | `datafusion` | Query optimizer + executor we'd otherwise build. |
| Text index | `tantivy` | Mature Rust inverted index. |
| Catalog DB | `rusqlite` (or `sqlx` + SQLite) | Simple, embedded, boring. |
| Compression | `zstd` | Best ratio/speed tradeoff today. |
| Serialization | `serde_json`, `rmp-serde`, `prost` | JSON for APIs, msgpack for internal, prost for proto. |
| WASM host | `wasmtime` + `wit-bindgen` | Component Model is production-ready. |
| Web UI | `leptos` + `trunk` + tailwind | Fine-grained reactivity in Rust. |
| TUI | `ratatui` + `crossterm` | Ratatui owns rendering; crossterm owns terminal + events. `event-stream` feature gives a tokio-compatible async event source. |
| Logging (dog-food) | `tracing` + `tracing-subscriber` | We emit structured logs into ourselves. Always keep a stderr fallback sink for bootstrap / storage-down scenarios. |
| Observability | `opentelemetry` crate | Self-host OTLP and eat it. |

## 6. Repo layout

```
tracecure/
├─ Cargo.toml                       # workspace
├─ crates/
│  ├─ tracecure-server/             # axum binary, wires everything
│  ├─ tracecure-ingest/             # decoders, input traits
│  ├─ tracecure-storage/            # WAL, memtable, segments, compaction
│  ├─ tracecure-query/              # TracecureQL parser, DataFusion TableProvider
│  ├─ tracecure-plugin-host/        # wasmtime host, WIT bindings
│  ├─ tracecure-api/                # REST + WS types (shared with frontend)
│  ├─ tracecure-cli/                # `tracecure` command
│  └─ tracecure-core/               # shared types: Record, Label, Stream
├─ ui/
│  └─ tracecure-ui/                 # leptos app, depends on tracecure-api
├─ plugins/
│  ├─ notifier-slack/         # reference plugin
│  ├─ transform-redact/       # reference plugin
│  └─ alert-threshold/        # reference plugin
├─ wit/
│  └─ v0/                     # canonical WIT interfaces for plugins
├─ docs/
└─ bench/                     # criterion benches + load-test harness
```

## 7. Milestones

Calendar estimates assume one engineer working roughly half-time. Double them for a realistic ship date.

**M0 — Walking skeleton (2 weeks)**
One binary. `POST /ingest` writes to a flat NDJSON file. `GET /search?q=…` greps it. A Leptos page that live-tails via WebSocket. No storage engine yet. Goal: end-to-end loop, CI, release pipeline, workspace layout. **Shipped 2026-04-22.** A `sift tui` subcommand (Ratatui-based interactive terminal UI) was added on a follow-up pass before M1 started — not in the original M0 plan, see §4.6.

**M1 — Real storage (4–6 weeks)**
WAL, memtable, Arrow-based segments with zstd + min/max footers. Time-partitioned directory layout. Restart safety. Retention by time. Basic label index (no full-text yet). Criterion benches on a 10M-record corpus. Format-fit spike (see §4.2) lands in the first week.

**M2 — Query engine (3–4 weeks)**
DataFusion TableProvider over segments. TracecureQL parser → logical plan. SQL endpoint. Pushdown for time range and label equality. Cursor pagination.

**M3 — Text index (2–3 weeks)**
Tantivy per-segment (sidecar). Integrate with TracecureQL `~` and `|~` operators. Hybrid queries (label predicate + text match) with segment pruning.

**M4 — Plugin host (~8 weeks)**
Wasmtime component runtime. WIT interfaces for `transform` and `notifier` under `wit/v0/`. Two reference plugins: `transform-redact`, `notifier-slack`. Plugin loading from `plugins/`.

**M5 — Alerting as a plugin (2 weeks)**
`alert` interface. Scheduler that runs windowed TracecureQL queries on a cron. Reference plugin `alert-threshold`. UI page to inspect alert runs.

**M6 — Hardening (ongoing)**
OTLP ingest. Auth + RBAC. Metrics dog-fooding. Docs. Docker image. First tagged release `v0.1.0`.

Anything past M6 — clustering, S3 tiering, tracing plugin, metrics plugin, managed distribution — is post-v0.1.

## 8. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Storage format churn locks out existing data. | Version every on-disk structure with a magic header + version byte; write a `tracecure migrate` tool from day one, even if it only migrates v1→v1. |
| Parquet-ish format doesn't fit wide-schema log data well. | M1 format-fit spike on 10M records. If write amp or footer size blows up, decide before compaction is built. |
| 100k events/sec headline target is unreachable with text index on. | Either soften the goal (25–30k/sec with index) or make index building async off the segment-writer thread. Decide at M1. |
| WASM component model changes break plugins. | Pin `wasmtime` and the WIT spec; bump in deliberate, documented releases. Ship a conformance test kit plugins can run against. |
| Rust/WASM frontend bundle is too heavy. | Budget it in CI (fail the build if gzipped `.wasm` > 600 KB). Lazy-load the SQL editor and chart libs. Fall back to a TS frontend if we're still over budget by M4. |
| DataFusion doesn't push down our custom predicates well. | Benchmark early on a realistic dataset at M2. If it underperforms, we keep DataFusion for SQL only and hand-roll the TracecureQL executor. |
| Tantivy index sizes balloon past expectations. | Make the text index optional per stream; streams that only need label search skip it. Sidecar layout makes this a per-segment choice. |
| "Alerting as a plugin" means every user reinvents it. | Ship a canonical `alert-threshold` + `notifier-slack` combo; document them as the blessed default. |
| Malicious plugin exploits WASI grants. | Document explicitly: v1 trust model is "operator vets plugins". Fuel/memory limits are accident protection, not a sandbox for hostile code. |
| Scope creep into full observability. | Hold the line: metrics/traces are plugins that live in separate repos and ship on their own cadence. |

## 9. Open questions to resolve before M1

Most of the v1 open questions are now answered (see §4.2, §4.5, §4.4). Remaining:

- Do we want a "query language v2" escape hatch early, or commit to TracecureQL-only for v1? (SQL via DataFusion is already the escape hatch — probably skip v2.)
- Exact WAL shard count default: one per vCPU, or fixed at 4? Affects `shard_id` cardinality in cursors.
- Format-fit spike result: does `arrow-rs` + `parquet` survive a 10M-record, wide-schema corpus, or do we pivot before M1 design freezes?

## 10. Immediate next step

Start M0 this week. Scaffold the Cargo workspace with `tracecure-core`, `tracecure-server`, `tracecure-cli`, and `tracecure-ui`. Wire `POST /ingest` → flat file → WS tail. Run the crates.io / NPM / USPTO availability check on "Tracecure" before publishing the first crate. Everything else downstream of that loop.
