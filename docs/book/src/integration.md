# Integrating with Tracecure

This guide covers how to send logs to and read logs from a running Tracecure server.
It reflects the M1 API — the wire shape is stable; the storage backend is the
WAL + Parquet segment store from the M1 tracecure-storage crate.

## TL;DR

- Ingest: `POST /v1/ingest` — newline-delimited JSON records.
- Search: `GET /v1/search?q=<substr>&limit=<n>&from=<ts>&to=<ts>` — case-sensitive
  substring, newest-first.
- Query: `GET /v1/query?query=<TracecureQL>&limit=<n>` — filter on columns, labels and
  text; results come a page at a time.
- Live tail: `GET /v1/tail` — upgrades to WebSocket, streams `TailFrame` JSON.
- Health: `GET /health` — returns `200 OK` with the literal body `ok`.
- Open by default: bind to `127.0.0.1` (the default) unless you turn on
  sign-in for reading ([Install and run](./install.md#sign-in)) and tokens for
  writing ([Ingest tokens](#ingest-tokens)).

## Running a server

```bash
cargo run -p tracecure-server -- serve
# or with the UI bundled:
(cd ui/tracecure-ui && trunk build --release)
cargo run -p tracecure-server -- serve --ui-dist ./ui/tracecure-ui/dist
```

Defaults:

- `--bind 127.0.0.1:7789`
- `--data ./data` (directory; WAL + segment files are created inside it)

## The `Record` shape

Every ingested line is one `Record`:

```json
{
  "timestamp": "2026-04-22T12:00:00.123456Z",
  "host": "app-server-1",
  "app": "api",
  "stream_id": "api",
  "level": "info",
  "body": "request processed",
  "labels": { "env": "prod", "region": "us-east-1" },
  "fields": { "request_id": "abc123", "duration_ms": 42 },
  "_raw": "optional untouched original payload"
}
```

Required fields: `timestamp` (RFC3339), `host`, `app`, `stream_id`, `body`.

Optional with defaults: `level` (default `"info"`, one of `trace`/`debug`/`info`/`warn`/`error`/`fatal`), `labels` (empty map), `fields` (empty map), `_raw` (absent).

**On responses these three are omitted, not empty.** The defaults above apply when
you *send* a record. When the server *returns* one — from `/v1/search` or
`/v1/tail` — `labels` and `fields` are dropped entirely when empty, and `_raw` is
dropped when absent. They are never sent as `{}` or `null`. Clients must treat all
three as possibly-missing keys: `record["labels"]["env"]` raises rather than
yielding an empty map. `level` is always present.

Design notes:

- `labels` is for low-cardinality indexed key/value pairs (env, region, version).
- `fields` is for schema-flexible structured data (request ids, durations, user ids).
- `stream_id` is the stable per-stream identifier. It is currently a free-form string with no server-side validation; pick something stable per service/environment so retention and policy stay attached to it later.

## Ingest: `POST /v1/ingest`

Body is NDJSON — one JSON record per line. Content-Type: `application/x-ndjson`.

If the server runs with `--ingest-auth required`, the request must also carry
`Authorization: Bearer <token>`; see [Ingest tokens](#ingest-tokens). Without
that mode the header is ignored, so a client can send it unconditionally.

Blank lines are skipped. Malformed lines increment `rejected` but do NOT fail the whole batch; the response is `200` with counts.

**Response** (`200 OK`):

```json
{ "accepted": 2, "rejected": 1 }
```

**Error conditions:**

- `400 Bad Request` — body is not valid UTF-8.
- `401 Unauthorized` — the server requires a token and the request had none, or
  one that is malformed, unknown or revoked. The body is plain text saying
  which. Nothing from the batch is stored.
- `500 Internal Server Error` — a store append failed mid-batch. Earlier records in the batch have already been written to the WAL; the response does not report their count. This is a known limitation and is still outstanding — the store does not yet offer batch transactional semantics, so treat a `500` as "an unknown prefix of this batch landed" and prefer idempotent or re-sendable batches.

### curl

```bash
curl -X POST http://127.0.0.1:7789/v1/ingest \
  -H 'content-type: application/x-ndjson' \
  --data-binary $'{"timestamp":"2026-04-22T12:00:00Z","host":"laptop","app":"demo","stream_id":"demo","body":"hello"}\n'
```

### Python (requests)

```python
import json
import requests
from datetime import datetime, timezone

def send(records, server="http://127.0.0.1:7789"):
    body = "\n".join(json.dumps(r) for r in records) + "\n"
    r = requests.post(
        f"{server}/v1/ingest",
        data=body,
        headers={"content-type": "application/x-ndjson"},
        timeout=5,
    )
    r.raise_for_status()
    return r.json()

send([{
    "timestamp": datetime.now(timezone.utc).isoformat(),
    "host": "my-host",
    "app": "my-app",
    "stream_id": "my-app",
    "level": "info",
    "body": "hello from python",
    "labels": {"env": "dev"},
    "fields": {"pid": 4242},
}])
```

### Node (fetch)

```javascript
async function send(records, server = "http://127.0.0.1:7789") {
  const body = records.map((r) => JSON.stringify(r)).join("\n") + "\n";
  const resp = await fetch(`${server}/v1/ingest`, {
    method: "POST",
    headers: { "content-type": "application/x-ndjson" },
    body,
  });
  if (!resp.ok) throw new Error(`ingest failed: ${resp.status}`);
  return resp.json();
}

await send([{
  timestamp: new Date().toISOString(),
  host: "my-host",
  app: "my-app",
  stream_id: "my-app",
  body: "hello from node",
}]);
```

### Rust — drop-in `tracing` layer

For a Rust app already using `tracing`, the smallest useful integration is a
custom `Layer` that batches events and POSTs them. A minimal sketch:

```rust
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::json;
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

pub struct TracecureLayer {
    tx: mpsc::Sender<serde_json::Value>,
}

impl TracecureLayer {
    pub fn new(server: impl Into<String>, app: impl Into<String>) -> Self {
        let server = server.into();
        let app = app.into();
        let (tx, rx) = mpsc::channel::<serde_json::Value>();
        thread::spawn(move || {
            let client = reqwest::blocking::Client::new();
            let mut buf: Vec<serde_json::Value> = Vec::new();
            loop {
                // Drain up to 256 events or 500 ms, whichever comes first.
                let first = match rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(v) => v,
                    Err(mpsc::RecvTimeoutError::Timeout) if buf.is_empty() => continue,
                    Err(mpsc::RecvTimeoutError::Timeout) => None.unwrap_or_else(|| json!(null)),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                if !first.is_null() { buf.push(first); }
                while let Ok(v) = rx.try_recv() {
                    buf.push(v);
                    if buf.len() >= 256 { break; }
                }
                if buf.is_empty() { continue; }
                let mut body = String::new();
                for r in &buf {
                    body.push_str(&r.to_string());
                    body.push('\n');
                }
                let _ = client.post(format!("{server}/v1/ingest"))
                    .header("content-type", "application/x-ndjson")
                    .body(body).send();
                buf.clear();
            }
            let _ = app; // silence unused in this sketch
        });
        Self { tx }
    }
}

impl<S> Layer<S> for TracecureLayer
where
    S: Subscriber + for<'l> LookupSpan<'l>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        // Extract fields into a Record shape. This sketch elides the field
        // visitor for brevity — see tracing-subscriber docs for the full form.
        let record = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "host": std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".into()),
            "app": env!("CARGO_PKG_NAME"),
            "stream_id": env!("CARGO_PKG_NAME"),
            "level": format!("{}", event.metadata().level()).to_lowercase(),
            "body": format!("{}", event.metadata().name()),
        });
        let _ = self.tx.send(record);
    }
}
```

Install with:

```rust
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

tracing_subscriber::registry()
    .with(TracecureLayer::new("http://127.0.0.1:7789", "my-app"))
    .with(tracing_subscriber::fmt::layer()) // still print to stderr
    .init();
```

This is a sketch, not a published crate. A proper integration crate lands on its own cadence post-v0.1.

## Ingest tokens

By default anyone who can reach the server can write to it. To restrict
writing, start the server with `--ingest-auth required` (or
`TRACECURE_INGEST_AUTH=required`) and give each writer its own token:

```bash
tracecure-server token create --name billing-service --data ./data
# Created ingest token "billing-service" (id 3fa9c1d2). It is shown once — store it now.
# tc_3fa9c1d2_9b1e…  <- on stdout, nothing else, so it can be captured

tracecure-server token list --data ./data
tracecure-server token revoke 3fa9c1d2 --data ./data
```

Then send it as a bearer token:

```bash
curl -X POST https://logs.example.com/v1/ingest \
  -H "Authorization: Bearer $TRACECURE_TOKEN" \
  -H 'content-type: application/x-ndjson' \
  --data-binary $'{"timestamp":"2026-04-22T12:00:00Z","host":"web-1","app":"billing","stream_id":"billing-prod","body":"hello"}\n'
```

```python
requests.post(f"{server}/v1/ingest", data=body,
              headers={"content-type": "application/x-ndjson",
                       "authorization": f"Bearer {token}"})
```

The CLI reads `TRACECURE_TOKEN`, or takes `--token`:
`TRACECURE_TOKEN=tc_… tracecure send --server https://logs.example.com "hello"`.

What to know:

- **The token is shown once.** Only its hash is stored. If it is lost, revoke it
  and create another.
- **`create` and `revoke` take effect on a running server at once** — no restart.
  In Docker: `docker compose exec tracecure tracecure-server token create --name x --data /data`.
- **A token only allows writing.** It cannot read or search, and nothing but
  `/v1/ingest` checks it.
- **One token per writer.** That is what makes revoking one possible without
  touching the others, and `token list` readable.
- **Send it over TLS only.** A bearer token on plain HTTP is visible to anyone on
  the path.
- **Limits.** A token can write any `app` and `stream_id`, does not expire, and
  is not rate-limited. If one leaks, revoke it; retention bounds what it left
  behind.

A `tc_` prefix makes a leaked token easy to spot in a log or a repository.

## Search: `GET /v1/search`

Query params:

- `q` — case-sensitive substring to match against `body`. Empty = match everything.
- `limit` — max records to return (default 100). Returns the most recent `limit`
  matches in **newest-first** (descending timestamp) order. There is no server-side
  ceiling on `limit` today — a large value will attempt to materialize that many
  records, so pick one your client can actually hold.
- `from` — RFC3339 timestamp, **inclusive** lower bound. Optional; omit for unbounded.
- `to` — RFC3339 timestamp, **exclusive** upper bound. Optional; omit for unbounded.
- `level` — only these levels: a comma-separated list of `trace`, `debug`, `info`,
  `warn`, `error`, `fatal` (case-insensitive; `warning` and `err` are accepted).
- `app`, `host`, `stream_id` — only records whose field equals one of a
  comma-separated list of names. A name ending in `*` matches by prefix:
  `app=billing*`.

The four filters are optional, and an absent or empty one means "any". Set
together they are a conjunction; the alternatives inside one are a disjunction —
`level=error,fatal&app=billing` is *(error or fatal) and billing*.

**`limit` applies after every filter.** `level=error&limit=100` returns the newest
100 errors, however many other records are newer. Filtering a `limit`ed result on
the client instead gives you only the errors that happen to be among the newest
100 records — usually far fewer, and silently so.

Supplying `from`/`to` is not just a filter: the server skips whole Parquet segments
whose time range doesn't intersect the window, so a bounded query is substantially
cheaper than an unbounded one.

**Encoding `from` / `to` — read this before you debug a 400.** Emit them as RFC3339
with a `Z` suffix and percent-encode the value. A raw `+HH:MM` offset (which is what
most RFC3339 formatters produce for UTC) decodes to a space under
`application/x-www-form-urlencoded`, and the request is rejected with `400` before
the handler ever runs. Use `2026-04-22T12:00:00.000Z`, not `2026-04-22T12:00:00+00:00`.
Most HTTP clients percent-encode query values for you if you pass params as a map
rather than interpolating them into the URL string.

**Response** (`200 OK`):

```json
{
  "records": [ { ...Record }, { ...Record } ],
  "scanned": 42
}
```

Records are newest-first. As noted under [The `Record` shape](#the-record-shape),
empty `labels` / `fields` and an absent `_raw` are omitted from each record rather
than sent as `{}` or `null`.

`scanned` is the number of records the time range covers — in-range memtable
records plus the full row count of every segment whose time range intersects the
query. Segments pruned by `from`/`to` are not counted. It is an upper bound on the
work, not a measure of it: a search stops reading once it has the newest `limit`
matches, so a common filter touches far fewer rows than `scanned` says. A filter
that matches rarely does read them all, and there a tighter `from`/`to` window is
the cheapest way to bring the cost down.

**Error conditions:**

- `400 Bad Request` — a query parameter failed to parse: a non-numeric `limit`, or
  an unparseable `from`/`to` (see the encoding note above), rejected before the
  handler runs; or `level` names something that is not a level, in which case the
  body is plain text saying which. An unknown level is an error rather than "no
  level filter" so that a typo cannot quietly return everything.
- `401 Unauthorized` — the server requires sign-in and the request carried no
  live session cookie. The body is JSON: `{"error": …, "login": "/auth/login"}`.
- `500 Internal Server Error` — a segment scan failed (e.g. an unreadable segment
  file). Body is the plain text `search failed`.

```bash
curl 'http://127.0.0.1:7789/v1/search?q=timeout&limit=20'

# The newest 50 errors and fatals from any app whose name starts with "billing"
curl -G 'http://127.0.0.1:7789/v1/search' \
  --data-urlencode 'level=error,fatal' \
  --data-urlencode 'app=billing*' \
  --data-urlencode 'limit=50'

# Bounded window — [from, to), Z-form timestamps, percent-encoded by curl's --data-urlencode
curl -G 'http://127.0.0.1:7789/v1/search' \
  --data-urlencode 'q=timeout' \
  --data-urlencode 'from=2026-04-22T12:00:00.000Z' \
  --data-urlencode 'to=2026-04-22T13:00:00.000Z' \
  --data-urlencode 'limit=500'
```

## Query: `GET /v1/query`

`/v1/search` takes a fixed set of filters. `/v1/query` takes a query in
**TracecureQL** and returns results a page at a time.

```
{app="api", env="prod"} | level="error,fatal" | body ~ "timeout" | host!="edge-9"
```

### The language

A query is an optional **selector** in braces, then any number of `|`-separated
**stages**. Both are lists of clauses, and a record matches when *every* clause
holds. `{}` on its own matches everything.

A clause is `key op "value"`. The value is always in double quotes; `\"` and
`\\` escape a quote and a backslash inside it.

| `key` | Refers to |
|---|---|
| `app`, `host`, `stream` (or `stream_id`), `level` (or `lvl`) | that field of the record |
| `body` | the message text |
| anything else | the label of that name |

| `op` | Holds when the value… |
|---|---|
| `=` | equals one of the comma-separated alternatives. An alternative ending in `*` matches by prefix. |
| `!=` | equals none of them |
| `~` | contains the text, as a case-sensitive substring |
| `!~` | does not contain it |

- A record without the label fails `=` and `~`, and passes `!=` and `!~`:
  `env!="prod"` is true of a record that has no `env` at all.
- `level` takes `=` and `!=` only, and an unknown level name is an error — a
  typo must not look like "no errors today".
- There is no `or` between clauses (use alternatives: `app="api,worker"`), no
  regular expressions, no tests on `fields`, and no aggregation.

### Request

| Parameter | Meaning |
|---|---|
| `query` | the query, URL-encoded. Required. |
| `limit` | page size, default `100` |
| `from`, `to` | time range, `from` inclusive and `to` exclusive, as in `/v1/search` |
| `cursor` | the `next_cursor` of the previous page |

```
curl -G http://127.0.0.1:7789/v1/query \
  --data-urlencode 'query={app="api"} | level="error"' \
  --data-urlencode 'limit=200'
```

### Response

```json
{
  "records": [ ... ],
  "scanned": 1200,
  "next_cursor": "t1745409600123456"
}
```

`records` are newest-first. `scanned` means what it means for `/v1/search`.

To page, send `next_cursor` back as `cursor` with the same `query`, `from` and
`to`. When `next_cursor` is absent, that was the last page.

- **Treat the cursor as opaque.** Do not build or edit one.
- **A page can be slightly longer than `limit`.** Records that share a timestamp
  are never split across pages, so that nothing is skipped or repeated at a
  boundary.
- **Paging is not a snapshot.** Records that arrive while you page are newer
  than your cursor and are not returned; records deleted by retention between
  two requests simply stop appearing.

Errors:

- `400 Bad Request` — the query does not parse, or the cursor is not one this
  server issued. The body is JSON: `{"error": "...", "at": 5}`, where `at` is
  the byte offset into `query` at which parsing stopped (absent for a cursor
  error).
- `401 Unauthorized` — as for `/v1/search`.
- `500 Internal Server Error` — a segment scan failed. Body is the plain text
  `query failed`.

## Live tail: `GET /v1/tail` (WebSocket)

Upgrade to WebSocket. The server pushes newline-free JSON text frames.

**Frame shape** (`TailFrame`):

```json
{ "type": "record", "timestamp": "...", "host": "...", ...Record fields }
{ "type": "heartbeat" }
{ "type": "error", "message": "tail lagged by 128 records — reconnect" }
{ "type": "unknown" }   // forward-compat catch-all; clients should ignore
```

The envelope is internally tagged: a `record` frame is the `Record` fields
inlined alongside `"type": "record"`, not nested under a key. The same omission
rule applies as everywhere else — empty `labels` / `fields` and an absent `_raw`
are not present on the frame, so guard before reading them.

Heartbeats are sent every 15 seconds so idle load balancers / proxies don't
close the connection. Clients should ignore them.

If the server's broadcast buffer overflows (slow subscriber), the server
sends a single `"type":"error"` frame indicating how many records were
skipped. The socket stays open; you can keep consuming newer records or
reconnect and replay via `/v1/search` if you need the gap. To replay, note the
timestamp of the last record you saw before the error and query
`?from=<that timestamp>&to=<timestamp of the first record after the gap>` —
`from` is inclusive and `to` exclusive, so passing the last-seen timestamp as
`from` re-delivers that record and everything after it.

### wscat

```bash
wscat -c ws://127.0.0.1:7789/v1/tail
```

### Python (websockets)

```python
import asyncio, json, websockets

async def tail(server="ws://127.0.0.1:7789"):
    async with websockets.connect(f"{server}/v1/tail") as ws:
        async for msg in ws:
            frame = json.loads(msg)
            if frame["type"] == "record":
                print(f"{frame['timestamp']} {frame['app']} {frame['body']}")

asyncio.run(tail())
```

### Node (`ws`)

```javascript
import WebSocket from "ws";
const ws = new WebSocket("ws://127.0.0.1:7789/v1/tail");
ws.on("message", (buf) => {
  const f = JSON.parse(buf.toString());
  if (f.type === "record") console.log(f.timestamp, f.app, f.body);
});
```

### Interactive terminal UI

For an interactive alternative to `tracecure tail`, use `tracecure tui`. It's a
Ratatui-based TUI with a status indicator, a live-updating record list,
a `/`-to-filter input, and scroll controls (Up/Down, PageUp/PageDown,
Home/End). Press `q` (or Ctrl+C) to quit. Same `--server` flag as the
other subcommands.

```
cargo run -p tracecure-cli -- tui --server http://127.0.0.1:7789
```

## Health: `GET /health`

Returns `200 OK` with the literal body `ok` — not JSON. No auth, and it checks no
dependencies: it answers as soon as the HTTP server is accepting connections, so
treat it as a liveness probe rather than a readiness signal about storage.

```bash
curl http://127.0.0.1:7789/health   # -> ok
```

The repo's own `deploy.ps1` polls this endpoint and compares the body against the
exact string `ok`, so the body is a deploy contract — it can't become JSON without
updating that script.

## Durability

`append` returns once the record has been framed into the WAL buffer and added to
the in-memory memtable. **It is not a synchronous durability barrier** — the fsync
has not happened yet when `POST /v1/ingest` responds.

A background group-commit ticker fsyncs every 10 ms, or immediately once 1 MiB of
frames is pending, whichever comes first. So a host crash loses at most the records
appended since the last tick — on the order of the last 10 ms, not the last few
seconds. Memtable flushes fsync the WAL through the current position before
publishing a segment, so no segment is ever visible ahead of its WAL. On restart
the WAL is replayed, so records that were fsynced but not yet flushed to a segment
survive.

If you need a hard per-record durability guarantee before acknowledging work
elsewhere, Tracecure does not currently offer one.

## What Tracecure does NOT do today (M1)

Before wiring Tracecure into anything you'd hate to lose:

- **Reading is open unless sign-in is configured.** With `--oidc-issuer`,
  `/v1/search`, `/v1/tail` and the UI require a browser session and answer
  `401` without one — so a script cannot read through the API at all yet; there
  are no personal API tokens. Without it they answer anyone who can reach them.
  There are no roles: whoever may sign in may read everything.
- **Ingest tokens are write-only and coarse.** They are off by default; when on,
  a token can write any `app` and `stream_id`, never expires, and has no rate
  limit. See [Ingest tokens](#ingest-tokens).
- **No retention unless you turn it on.** By default segments accumulate
  forever. `--retention 30d` deletes records older than that, and
  `--retention-stream <stream_id>=<duration>` sets a different age for one
  stream — which is one reason to choose `stream_id` deliberately. See
  [Install and run](./install.md#retention). There is no size-based limit:
  watch the size of your `--data` directory yourself.
- **No synchronous durability barrier.** `append` returns before its fsync. See
  [Durability](#durability) above for what is and isn't guaranteed.
- **No backpressure beyond tokio's socket buffers.** If you ingest faster than
  the single writer can serialize, your client will eventually block or time out.
- **No clustering.** Single-node only. Read replicas and distributed query
  are post-v0.1.
- **Search is filtering, not analysis.** `/v1/search` and `/v1/query` select
  records by level, app, host, stream, label and body text; text matching is a
  case-sensitive substring. There is no aggregation, no regular expressions, no
  SQL, and no text index — a search for rare text reads every record in the
  time range.
- **No TLS.** Put Tracecure behind a reverse proxy if you need encryption on the wire.
- **Body size limit.** Ingest accepts up to axum's default ~2 MB per POST. For
  larger batches, split client-side.

## Rough capacity ceiling

On M1's WAL + Parquet segment backend, expect roughly:

- **1–10 k records/sec** ingest on a laptop, limited by WAL fsync cadence and
  JSON serialization.
- **Search** scans the in-memory memtable and Parquet segments. Segment scans
  run in a blocking thread pool and benefit from Parquet predicate pushdown in
  future milestones.
- **Live tail** is O(1) per record — it reads off a broadcast channel, not
  disk — so it's the cheap path.

## Reporting issues

Tracecure is pre-v0.1. If something behaves surprisingly:

1. Reproduce with `RUST_LOG=debug cargo run -p tracecure-server -- serve` and capture the server's logs.
2. Check `./data/wal/` and `./data/segments/` directly to see what was actually persisted.
3. Open an issue with the request body, the server log excerpt, and the
   observed vs expected response.
