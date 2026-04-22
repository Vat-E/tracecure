# Integrating with Sift

This guide covers how to send logs to and read logs from a running Sift server.
It reflects the M0 walking-skeleton API — the wire shape is stable for the
milestone; the storage backend behind it will change in M1.

## TL;DR

- Ingest: `POST /v1/ingest` — newline-delimited JSON records.
- Search: `GET /v1/search?q=<substr>&limit=<n>` — case-sensitive substring.
- Live tail: `GET /v1/tail` — upgrades to WebSocket, streams `TailFrame` JSON.
- No auth in M0. Bind to `127.0.0.1` (the default) or a trusted network.

## Running a server

```bash
cargo run -p sift-server -- serve
# or with the UI bundled:
(cd ui/sift-ui && trunk build --release)
cargo run -p sift-server -- serve --ui-dist ./ui/sift-ui/dist
```

Defaults:

- `--bind 127.0.0.1:7789`
- `--data ./data/sift.ndjson` (created if missing, parent dir auto-created)

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

Optional with defaults: `level` (default `"info"`, one of `trace`/`debug`/`info`/`warn`/`error`/`fatal`), `labels` (empty map), `fields` (empty map), `_raw` (null).

Design notes:

- `labels` is for low-cardinality indexed key/value pairs (env, region, version).
- `fields` is for schema-flexible structured data (request ids, durations, user ids).
- `stream_id` is the stable per-stream identifier. For M0 it's a free-form string; pick something stable per service/environment so retention and policy stay attached to it later.

## Ingest: `POST /v1/ingest`

Body is NDJSON — one JSON record per line. Content-Type: `application/x-ndjson`.

Blank lines are skipped. Malformed lines increment `rejected` but do NOT fail the whole batch; the response is `200` with counts.

**Response** (`200 OK`):

```json
{ "accepted": 2, "rejected": 1 }
```

**Error conditions:**

- `400 Bad Request` — body is not valid UTF-8.
- `500 Internal Server Error` — an I/O failure occurred mid-batch. Earlier records in the batch are already durable; the response does not report their count. This is a known M0 limitation (batch transactional semantics land in M1).

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

pub struct SiftLayer {
    tx: mpsc::Sender<serde_json::Value>,
}

impl SiftLayer {
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

impl<S> Layer<S> for SiftLayer
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
    .with(SiftLayer::new("http://127.0.0.1:7789", "my-app"))
    .with(tracing_subscriber::fmt::layer()) // still print to stderr
    .init();
```

This is a sketch, not a published crate. A proper integration crate lands on its own cadence post-v0.1.

## Search: `GET /v1/search`

Query params:

- `q` — case-sensitive substring to match against `body`. Empty = match everything.
- `limit` — max records to return (default 100, hard cap 10,000). Returns the most recent `limit` matches in oldest-first order.

**Response** (`200 OK`):

```json
{
  "records": [ { ...Record }, { ...Record } ],
  "scanned": 42
}
```

`scanned` is the total number of records-shaped lines read from disk (including malformed ones, excluding blanks). It's a cost indicator — if you see `scanned` growing much faster than `records.len()`, your substring is too specific for the corpus.

```bash
curl 'http://127.0.0.1:7789/v1/search?q=timeout&limit=20'
```

## Live tail: `GET /v1/tail` (WebSocket)

Upgrade to WebSocket. The server pushes newline-free JSON text frames.

**Frame shape** (`TailFrame`):

```json
{ "type": "record", "timestamp": "...", "host": "...", ...Record fields }
{ "type": "heartbeat" }
{ "type": "error", "message": "tail lagged by 128 records — reconnect" }
{ "type": "unknown" }   // forward-compat catch-all; clients should ignore
```

Heartbeats are sent every 15 seconds so idle load balancers / proxies don't
close the connection. Clients should ignore them.

If the server's broadcast buffer overflows (slow subscriber), the server
sends a single `"type":"error"` frame indicating how many records were
skipped. The socket stays open; you can keep consuming newer records or
reconnect and replay via `/v1/search` if you need the gap.

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

## What Sift does NOT do today (M0)

Before wiring Sift into anything you'd hate to lose:

- **No auth.** Bind to `127.0.0.1` or a trusted network. Ingest tokens and
  RBAC land in M6.
- **No retention.** The NDJSON file grows forever. Size-based or time-based
  eviction lands in M1.
- **No durability guarantee.** `append` flushes to the kernel buffer but does
  not `fsync`. A host crash can lose the last few seconds of records. M1 WAL
  fixes this with group commit.
- **No backpressure beyond tokio's socket buffers.** If you ingest faster than
  the single writer can serialize, your client will eventually block or time out.
- **No clustering.** Single-node only. Read replicas and distributed query
  are post-v0.1.
- **No structured search.** Only case-sensitive substring match on `body`.
  SiftQL + DataFusion backend land in M2.
- **No TLS.** Put Sift behind a reverse proxy if you need encryption on the wire.
- **Body size limit.** Ingest accepts up to axum's default ~2 MB per POST. For
  larger batches, split client-side.

## Rough capacity ceiling

On M0's single-writer NDJSON backend, expect roughly:

- **1–5 k records/sec** ingest on a laptop, limited by JSON serialization and
  the single `Mutex<File>` writer.
- **Linear-time search**, so query latency grows with corpus size. A 100 MB
  file scans in ~a few hundred ms on NVMe.
- **Live tail** is O(1) per record — it reads off a broadcast channel, not
  disk — so it's the cheap path.

If you need more than that, wait for M1's Arrow segment storage before
putting production workloads through it.

## Reporting issues

Sift is pre-v0.1. If something behaves surprisingly:

1. Reproduce with `RUST_LOG=debug cargo run -p sift-server -- serve` and capture the server's logs.
2. Check `./data/sift.ndjson` directly to see what was actually persisted.
3. Open an issue with the request body, the server log excerpt, and the
   observed vs expected response.
