# M0 Walking Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Scaffold the Sift Cargo workspace and wire the end-to-end log loop — ingest over HTTP, NDJSON file storage, substring search, WebSocket live-tail, plus a Leptos UI, `sift` CLI, and CI.

**Architecture:** Single `sift serve` binary written in Rust with `axum`. Ingested records are appended to an NDJSON file on disk and fanned out to a `tokio::sync::broadcast` channel that feeds live-tail WebSocket subscribers. Search is line-wise `grep` over the NDJSON file. The frontend is a Leptos CSR app built with `trunk` and served as static files by the same binary. A `sift` CLI wraps the REST API. This is a throwaway storage model — the M1 WAL + Arrow segments replace it — so no abstractions that assume they'll last.

**Tech Stack:** Rust 1.94, `tokio`, `axum` 0.7, `serde`/`serde_json`, `chrono`, `clap` 4, `leptos` 0.6, `trunk`, `tower-http`, `tracing`. GitHub Actions for CI.

---

## File Structure

```
sift/
├─ Cargo.toml                    # workspace root, shared deps
├─ rust-toolchain.toml           # pin 1.94, add wasm32 target
├─ .gitignore
├─ .gitattributes
├─ README.md
├─ PLAN.md                       # (already exists)
├─ crates/
│  ├─ sift-core/
│  │  ├─ Cargo.toml
│  │  └─ src/
│  │     ├─ lib.rs               # re-exports
│  │     ├─ record.rs            # Record, Level
│  │     └─ stream.rs            # StreamId newtype
│  ├─ sift-api/
│  │  ├─ Cargo.toml
│  │  └─ src/
│  │     ├─ lib.rs               # re-exports
│  │     ├─ ingest.rs            # IngestResponse
│  │     ├─ search.rs            # SearchQuery, SearchResponse
│  │     └─ tail.rs              # TailFrame
│  ├─ sift-server/
│  │  ├─ Cargo.toml
│  │  └─ src/
│  │     ├─ main.rs              # entry, arg parsing, server bootstrap
│  │     ├─ state.rs             # AppState, file-backed store handle
│  │     ├─ store.rs             # append/read/broadcast plumbing
│  │     ├─ routes/
│  │     │  ├─ mod.rs            # Router composition
│  │     │  ├─ health.rs
│  │     │  ├─ ingest.rs
│  │     │  ├─ search.rs
│  │     │  └─ tail.rs
│  │     └─ ui.rs                # static UI serving (dist fallback)
│  ├─ sift-cli/
│  │  ├─ Cargo.toml
│  │  └─ src/main.rs             # send/search/tail commands
│  └─ (sift-ingest, sift-storage, sift-query, sift-plugin-host — scaffolded as stubs for later milestones)
├─ ui/
│  └─ sift-ui/
│     ├─ Cargo.toml
│     ├─ Trunk.toml
│     ├─ index.html
│     ├─ style.css
│     └─ src/
│        ├─ main.rs              # mount Leptos App
│        └─ app.rs               # live-tail component
├─ .github/
│  └─ workflows/
│     └─ ci.yml
└─ docs/
   └─ superpowers/plans/         # (this file lives here)
```

File responsibilities:

- `sift-core` — stable, minimal record types shared by everything. No I/O, no tokio. Compiles to wasm32.
- `sift-api` — REST/WS request/response types. Depends on `sift-core`. Also wasm32-clean.
- `sift-server` — axum binary. `store.rs` owns the NDJSON file handle and the broadcast sender; `routes/*.rs` each own one endpoint. Keep handlers thin.
- `sift-cli` — thin `reqwest`/`tokio-tungstenite` wrapper around the REST API.
- `sift-ui` — Leptos app, CSR only, talks to the server over HTTP + WebSocket.

---

## Task 1: Workspace bootstrap (git, workspace Cargo.toml, toolchain, gitignore)

**Files:**
- Create: `.gitignore`
- Create: `.gitattributes`
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `README.md`
- Create: `crates/sift-core/Cargo.toml`
- Create: `crates/sift-core/src/lib.rs` (placeholder)
- Create: `crates/sift-api/Cargo.toml`
- Create: `crates/sift-api/src/lib.rs` (placeholder)
- Create: `crates/sift-server/Cargo.toml`
- Create: `crates/sift-server/src/main.rs` (placeholder)
- Create: `crates/sift-cli/Cargo.toml`
- Create: `crates/sift-cli/src/main.rs` (placeholder)

- [ ] **Step 1: Initialize git repository**

Run: `git init -b main`
Expected: `Initialized empty Git repository in C:/Users/vz/Programming/sift/.git/`

- [ ] **Step 2: Write `.gitignore`**

```
/target
**/*.rs.bk
Cargo.lock.bak

# trunk output
/ui/sift-ui/dist
/dist

# local runtime state
/data
*.ndjson

# editors
.vscode/
.idea/
*.swp
.DS_Store

# OS
Thumbs.db
```

- [ ] **Step 3: Write `.gitattributes`**

```
* text=auto eol=lf
*.rs text eol=lf
*.toml text eol=lf
*.md text eol=lf
```

This keeps line endings LF on Windows so CI doesn't trip on CRLF differences.

- [ ] **Step 4: Write workspace `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = [
    "crates/sift-core",
    "crates/sift-api",
    "crates/sift-server",
    "crates/sift-cli",
]
# ui/sift-ui is excluded from the workspace because it builds for wasm32
# and has incompatible target features with the server binaries.
exclude = ["ui/sift-ui"]

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "MIT OR Apache-2.0"
rust-version = "1.80"
repository = "https://github.com/sift-dev/sift"

[workspace.dependencies]
sift-core = { path = "crates/sift-core" }
sift-api  = { path = "crates/sift-api" }

tokio = { version = "1.40", features = ["full"] }
axum = { version = "0.7", features = ["ws", "macros"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["fs", "trace", "cors"] }

serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }

clap = { version = "4", features = ["derive"] }
anyhow = "1"
thiserror = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
futures-util = "0.3"

[workspace.lints.rust]
unsafe_code = "deny"
unused_must_use = "warn"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
```

- [ ] **Step 5: Write `rust-toolchain.toml`**

```toml
[toolchain]
channel = "1.94"
components = ["rustfmt", "clippy"]
targets = ["wasm32-unknown-unknown"]
profile = "minimal"
```

- [ ] **Step 6: Write `README.md`**

```markdown
# Sift

Self-hosted Rust-native log platform. See [PLAN.md](PLAN.md) for the design and milestones.

## Status

M0 — walking skeleton.

## Quick start

```
cargo run -p sift-server -- serve --data ./data
```

Then in another terminal:

```
cargo run -p sift-cli -- send --host localhost --app demo 'hello world'
cargo run -p sift-cli -- search 'hello'
cargo run -p sift-cli -- tail
```

## Development

- `cargo build --workspace` — build server + CLI.
- `cargo test --workspace` — run all tests.
- `cd ui/sift-ui && trunk serve` — run the frontend in dev mode.
- `cd ui/sift-ui && trunk build --release` — build production UI bundle into `ui/sift-ui/dist/`.

## License

MIT OR Apache-2.0.
```

- [ ] **Step 7: Scaffold four placeholder crates**

Create `crates/sift-core/Cargo.toml`:

```toml
[package]
name = "sift-core"
edition.workspace = true
version.workspace = true
license.workspace = true
description = "Shared core types for Sift (record, stream id, level)."

[lints]
workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
chrono = { workspace = true }
thiserror = { workspace = true }
```

Create `crates/sift-core/src/lib.rs`:

```rust
//! sift-core — shared record types.

// Filled in by Task 2.
```

Create `crates/sift-api/Cargo.toml`:

```toml
[package]
name = "sift-api"
edition.workspace = true
version.workspace = true
license.workspace = true
description = "REST and WebSocket API types for Sift."

[lints]
workspace = true

[dependencies]
sift-core = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
```

Create `crates/sift-api/src/lib.rs`:

```rust
//! sift-api — wire types for Sift's REST + WebSocket surface.

// Filled in by Task 3.
```

Create `crates/sift-server/Cargo.toml`:

```toml
[package]
name = "sift-server"
edition.workspace = true
version.workspace = true
license.workspace = true
description = "Sift server binary — ingest, storage, query, UI."
default-run = "sift-server"

[lints]
workspace = true

[[bin]]
name = "sift-server"
path = "src/main.rs"

[dependencies]
sift-core = { workspace = true }
sift-api = { workspace = true }
tokio = { workspace = true }
axum = { workspace = true }
tower = { workspace = true }
tower-http = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
chrono = { workspace = true }
clap = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
futures-util = { workspace = true }

[dev-dependencies]
reqwest = { workspace = true }
tokio-tungstenite = { workspace = true }
tempfile = "3"
```

Create `crates/sift-server/src/main.rs`:

```rust
fn main() {
    println!("sift-server placeholder — see Task 4");
}
```

Create `crates/sift-cli/Cargo.toml`:

```toml
[package]
name = "sift-cli"
edition.workspace = true
version.workspace = true
license.workspace = true
description = "Sift command-line interface."
default-run = "sift"

[lints]
workspace = true

[[bin]]
name = "sift"
path = "src/main.rs"

[dependencies]
sift-core = { workspace = true }
sift-api = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
chrono = { workspace = true }
clap = { workspace = true }
anyhow = { workspace = true }
reqwest = { workspace = true }
tokio-tungstenite = { workspace = true }
futures-util = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
```

Create `crates/sift-cli/src/main.rs`:

```rust
fn main() {
    println!("sift-cli placeholder — see Task 8");
}
```

- [ ] **Step 8: Verify the workspace builds**

Run: `cargo build --workspace`
Expected: Compilation succeeds. `target/debug/sift-server.exe` and `target/debug/sift.exe` exist.

- [ ] **Step 9: Commit the scaffold**

```bash
git add .gitignore .gitattributes Cargo.toml rust-toolchain.toml README.md crates/ docs/
git commit -m "chore: bootstrap cargo workspace and placeholder crates"
```

---

## Task 2: `sift-core` — `Record` and `Level` types (TDD)

**Files:**
- Modify: `crates/sift-core/src/lib.rs`
- Create: `crates/sift-core/src/record.rs`
- Create: `crates/sift-core/src/stream.rs`

- [ ] **Step 1: Write the failing test for `Level` serde**

Create `crates/sift-core/src/record.rs` with just the test module so we bootstrap the file, then immediately see it fail:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Severity level. Stored as lowercase strings on the wire.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl Default for Level {
    fn default() -> Self {
        Self::Info
    }
}

/// A single log record. Normalized shape used everywhere inside Sift.
///
/// `labels` are low-cardinality indexed key/value pairs.
/// `fields` are schema-flexible structured data.
/// `raw` preserves the original untouched payload when the ingest stage had one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub timestamp: DateTime<Utc>,
    pub host: String,
    pub app: String,
    pub stream_id: String,
    #[serde(default)]
    pub level: Level,
    pub body: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "_raw", skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn level_roundtrips_as_lowercase() {
        let json = serde_json::to_string(&Level::Warn).unwrap();
        assert_eq!(json, "\"warn\"");
        let back: Level = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Level::Warn);
    }

    #[test]
    fn level_defaults_to_info() {
        assert_eq!(Level::default(), Level::Info);
    }

    #[test]
    fn record_roundtrips_through_json() {
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        let mut fields = BTreeMap::new();
        fields.insert("user_id".to_string(), serde_json::json!(42));

        let rec = Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap(),
            host: "box-1".into(),
            app: "api".into(),
            stream_id: "api".into(),
            level: Level::Error,
            body: "boom".into(),
            labels,
            fields,
            raw: Some("boom\n".into()),
        };

        let json = serde_json::to_string(&rec).unwrap();
        let back: Record = serde_json::from_str(&json).unwrap();
        assert_eq!(back, rec);
    }

    #[test]
    fn record_parses_minimal_input_using_defaults() {
        let json = r#"{
            "timestamp":"2026-04-22T12:00:00Z",
            "host":"h",
            "app":"a",
            "stream_id":"s",
            "body":"hi"
        }"#;
        let rec: Record = serde_json::from_str(json).unwrap();
        assert_eq!(rec.level, Level::Info);
        assert!(rec.labels.is_empty());
        assert!(rec.fields.is_empty());
        assert!(rec.raw.is_none());
    }

    #[test]
    fn record_skips_empty_optional_fields_when_serializing() {
        let rec = Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 22, 0, 0, 0).unwrap(),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: Level::Info,
            body: "x".into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        };
        let json = serde_json::to_string(&rec).unwrap();
        assert!(!json.contains("labels"));
        assert!(!json.contains("fields"));
        assert!(!json.contains("_raw"));
    }
}
```

Create `crates/sift-core/src/stream.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable identifier for a logical log stream. Assigned at ingest.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StreamId(pub String);

impl StreamId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for StreamId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_id_is_transparent_in_json() {
        let id = StreamId::new("api.prod");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"api.prod\"");
        let back: StreamId = serde_json::from_str("\"api.prod\"").unwrap();
        assert_eq!(back, id);
    }
}
```

Replace `crates/sift-core/src/lib.rs`:

```rust
//! sift-core — shared record types used across Sift.

mod record;
mod stream;

pub use record::{Level, Record};
pub use stream::StreamId;
```

- [ ] **Step 2: Run the tests — expect them to pass**

Run: `cargo test -p sift-core`
Expected: 5 tests pass.

If any fail, fix the code, not the tests. The tests encode the contract stated above.

- [ ] **Step 3: Run clippy — zero warnings**

Run: `cargo clippy -p sift-core --all-targets -- -D warnings`
Expected: No diagnostics.

- [ ] **Step 4: Commit**

```bash
git add crates/sift-core
git commit -m "feat(core): add Record, Level, and StreamId types"
```

---

## Task 3: `sift-api` — ingest, search, and tail wire types (TDD)

**Files:**
- Modify: `crates/sift-api/src/lib.rs`
- Create: `crates/sift-api/src/ingest.rs`
- Create: `crates/sift-api/src/search.rs`
- Create: `crates/sift-api/src/tail.rs`

- [ ] **Step 1: Write tests and types for the ingest response**

Create `crates/sift-api/src/ingest.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Response from `POST /v1/ingest`. Summarizes how many NDJSON lines we
/// accepted versus rejected in the request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestResponse {
    pub accepted: usize,
    pub rejected: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_response_roundtrips() {
        let r = IngestResponse { accepted: 7, rejected: 1 };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"accepted":7,"rejected":1}"#);
        let back: IngestResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
```

- [ ] **Step 2: Write tests and types for the search query/response**

Create `crates/sift-api/src/search.rs`:

```rust
use serde::{Deserialize, Serialize};
use sift_core::Record;

/// Query string for `GET /v1/search`.
///
/// `q` is a case-sensitive substring match against `Record::body` for M0.
/// `limit` caps the number of returned records (server enforces an upper
/// bound).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    100
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self { q: String::new(), limit: default_limit() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub records: Vec<Record>,
    /// Number of records scanned from disk, regardless of whether they matched.
    pub scanned: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_query_parses_from_form_like_input() {
        let q: SearchQuery = serde_json::from_str(r#"{"q":"boom"}"#).unwrap();
        assert_eq!(q.q, "boom");
        assert_eq!(q.limit, 100);
    }

    #[test]
    fn search_query_default_limit_is_100() {
        assert_eq!(SearchQuery::default().limit, 100);
    }

    #[test]
    fn search_response_roundtrips_empty() {
        let r = SearchResponse { records: vec![], scanned: 0 };
        let json = serde_json::to_string(&r).unwrap();
        let back: SearchResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
```

- [ ] **Step 3: Write tests and types for tail frames**

Create `crates/sift-api/src/tail.rs`:

```rust
use serde::{Deserialize, Serialize};
use sift_core::Record;

/// A single frame sent over the live-tail WebSocket.
///
/// `Record` carries a newly ingested record. `Heartbeat` is sent periodically
/// so idle proxies keep the connection open. `Error` surfaces a server-side
/// problem the client should display and then reconnect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TailFrame {
    Record(Record),
    Heartbeat,
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use sift_core::{Level, Record};
    use std::collections::BTreeMap;

    #[test]
    fn heartbeat_serializes_as_tagged_object() {
        let f = TailFrame::Heartbeat;
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(json, r#"{"type":"heartbeat"}"#);
    }

    #[test]
    fn record_frame_roundtrips() {
        let rec = Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 22, 0, 0, 0).unwrap(),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: Level::Info,
            body: "hi".into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        };
        let frame = TailFrame::Record(rec.clone());
        let json = serde_json::to_string(&frame).unwrap();
        let back: TailFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TailFrame::Record(rec));
    }

    #[test]
    fn error_frame_roundtrips() {
        let f = TailFrame::Error { message: "boom".into() };
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(json, r#"{"type":"error","message":"boom"}"#);
    }
}
```

Replace `crates/sift-api/src/lib.rs`:

```rust
//! sift-api — REST + WebSocket wire types shared by server, CLI, and UI.

mod ingest;
mod search;
mod tail;

pub use ingest::IngestResponse;
pub use search::{SearchQuery, SearchResponse};
pub use tail::TailFrame;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sift-api`
Expected: 7 tests pass.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p sift-api --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/sift-api
git commit -m "feat(api): add ingest, search, and tail wire types"
```

---

## Task 4: `sift-server` — binary skeleton, args, `/health` endpoint (TDD)

**Files:**
- Modify: `crates/sift-server/src/main.rs`
- Create: `crates/sift-server/src/state.rs`
- Create: `crates/sift-server/src/routes/mod.rs`
- Create: `crates/sift-server/src/routes/health.rs`

- [ ] **Step 1: Write the integration test for `/health`**

Create `crates/sift-server/tests/health.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_200_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("data.ndjson")).unwrap();
    let app = sift_server::router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], b"ok");
}
```

Add `tower` as a dev-dep by appending to `crates/sift-server/Cargo.toml` `[dev-dependencies]`:

```toml
tower = { workspace = true }
```

Also add a `[lib]` section so integration tests can import `sift_server`. Replace the `[[bin]]` block and add `[lib]` in `crates/sift-server/Cargo.toml`:

```toml
[lib]
name = "sift_server"
path = "src/lib.rs"

[[bin]]
name = "sift-server"
path = "src/main.rs"
```

- [ ] **Step 2: Run the test — expect it to fail**

Run: `cargo test -p sift-server --test health`
Expected: FAIL — `sift_server::AppState` and `sift_server::router` do not exist.

- [ ] **Step 3: Implement the minimum to compile**

Create `crates/sift-server/src/lib.rs`:

```rust
//! sift-server — library surface used by integration tests and the binary.

pub mod routes;
pub mod state;

pub use state::AppState;

use axum::Router;

/// Build the top-level router. The same function is used by the binary entry
/// point in `main.rs` and by integration tests.
pub fn router(state: AppState) -> Router {
    Router::new().merge(routes::all()).with_state(state)
}
```

Create `crates/sift-server/src/state.rs`:

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Shared application state. Cheap to clone — everything is `Arc`-shared.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    data_path: PathBuf,
}

impl AppState {
    /// Open the app state rooted at `data_path`. Creates the parent directory
    /// if it doesn't exist, but does not touch the file itself yet — that
    /// happens on first write.
    pub fn open(data_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data_path = data_path.as_ref().to_path_buf();
        if let Some(parent) = data_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        Ok(Self { inner: Arc::new(Inner { data_path }) })
    }

    pub fn data_path(&self) -> &Path {
        &self.inner.data_path
    }
}
```

Create `crates/sift-server/src/routes/mod.rs`:

```rust
use axum::Router;

use crate::state::AppState;

pub mod health;

pub fn all() -> Router<AppState> {
    Router::new().merge(health::router())
}
```

Create `crates/sift-server/src/routes/health.rs`:

```rust
use axum::{routing::get, Router};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(|| async { "ok" }))
}
```

Replace `crates/sift-server/src/main.rs`:

```rust
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "sift-server", version, about = "Sift log platform")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run the HTTP server.
    Serve {
        /// Path to the NDJSON data file. Created if missing.
        #[arg(long, default_value = "./data/sift.ndjson")]
        data: PathBuf,

        /// Bind address.
        #[arg(long, default_value = "127.0.0.1:7789")]
        bind: SocketAddr,

        /// Path to a pre-built UI dist directory (from `trunk build`). If
        /// absent, UI serving is disabled and `/` returns 404.
        #[arg(long)]
        ui_dist: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sift_server=debug")))
        .init();

    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Serve { data, bind, ui_dist: _ } => {
            let state = sift_server::AppState::open(&data)
                .with_context(|| format!("opening data file at {}", data.display()))?;
            let app = sift_server::router(state);

            tracing::info!(%bind, data = %data.display(), "starting sift-server");
            let listener = tokio::net::TcpListener::bind(bind).await?;
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}
```

Note: `ui_dist` is parsed but not wired up yet — Task 9 fills that in. We accept the dead-code warning by prefixing with `_`.

- [ ] **Step 4: Run the test — expect it to pass**

Run: `cargo test -p sift-server --test health`
Expected: 1 test passes.

- [ ] **Step 5: Smoke-test the binary**

Run:
```
cargo run -p sift-server -- serve --data ./data/sift.ndjson --bind 127.0.0.1:7789
```

In a second terminal:
```
curl http://127.0.0.1:7789/health
```
Expected: `ok`

Stop the server with Ctrl+C.

- [ ] **Step 6: Clippy + commit**

Run: `cargo clippy -p sift-server --all-targets -- -D warnings`
Expected: No warnings.

```bash
git add crates/sift-server
git commit -m "feat(server): add axum skeleton with /health endpoint"
```

---

## Task 5: `sift-server` — `POST /v1/ingest` writes NDJSON and broadcasts (TDD)

**Files:**
- Create: `crates/sift-server/src/store.rs`
- Modify: `crates/sift-server/src/state.rs`
- Create: `crates/sift-server/src/routes/ingest.rs`
- Modify: `crates/sift-server/src/routes/mod.rs`

- [ ] **Step 1: Write the integration test**

Create `crates/sift-server/tests/ingest.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use sift_api::IngestResponse;
use tower::ServiceExt;

fn sample_records() -> String {
    // Two valid records + one malformed line.
    let a = r#"{"timestamp":"2026-04-22T12:00:00Z","host":"h","app":"a","stream_id":"s","body":"hello"}"#;
    let b = r#"{"timestamp":"2026-04-22T12:00:01Z","host":"h","app":"a","stream_id":"s","body":"world"}"#;
    let bad = r#"{"not":"a record"}"#;
    format!("{a}\n{b}\n{bad}\n")
}

#[tokio::test]
async fn ingest_appends_valid_records_and_reports_rejects() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data.ndjson");
    let state = sift_server::AppState::open(&data).unwrap();
    let app = sift_server::router(state);

    let res = app
        .oneshot(
            Request::post("/v1/ingest")
                .header("content-type", "application/x-ndjson")
                .body(Body::from(sample_records()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    let resp: IngestResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.accepted, 2);
    assert_eq!(resp.rejected, 1);

    let written = std::fs::read_to_string(&data).unwrap();
    let lines: Vec<&str> = written.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"body\":\"hello\""));
    assert!(lines[1].contains("\"body\":\"world\""));
}

#[tokio::test]
async fn ingest_rejects_empty_body_with_200_and_zero_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("d.ndjson")).unwrap();
    let app = sift_server::router(state);

    let res = app
        .oneshot(
            Request::post("/v1/ingest")
                .header("content-type", "application/x-ndjson")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
    let resp: IngestResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp, IngestResponse { accepted: 0, rejected: 0 });
}
```

- [ ] **Step 2: Run the test — expect it to fail**

Run: `cargo test -p sift-server --test ingest`
Expected: FAIL — route not registered.

- [ ] **Step 3: Implement the store**

Create `crates/sift-server/src/store.rs`:

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use sift_core::Record;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, Mutex};

const BROADCAST_CAPACITY: usize = 1024;
const SEARCH_MAX_LIMIT: usize = 10_000;

/// File-backed record store for M0. Appends NDJSON to a single file and
/// publishes new records on a broadcast channel for live-tail subscribers.
///
/// This is a throwaway backend; M1 replaces it with a WAL + Arrow segments.
#[derive(Clone)]
pub struct Store {
    inner: Arc<StoreInner>,
}

struct StoreInner {
    path: PathBuf,
    writer: Mutex<File>,
    tx: broadcast::Sender<Record>,
}

impl Store {
    pub async fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("opening {}", path.display()))?;
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Ok(Self {
            inner: Arc::new(StoreInner { path, writer: Mutex::new(file), tx }),
        })
    }

    /// Append a single record as one NDJSON line and broadcast it.
    /// Returns an error if the record cannot be serialized or written.
    pub async fn append(&self, record: Record) -> anyhow::Result<()> {
        let mut line = serde_json::to_vec(&record).context("serializing record")?;
        line.push(b'\n');
        let mut guard = self.inner.writer.lock().await;
        guard.write_all(&line).await.context("writing record")?;
        guard.flush().await.context("flushing record")?;
        drop(guard);
        // Best-effort broadcast; if there are no subscribers we still succeed.
        let _ = self.inner.tx.send(record);
        Ok(())
    }

    /// Scan the file and collect up to `limit` records whose `body` contains
    /// `query` as a case-sensitive substring (empty query matches everything).
    /// Returns `(matches, scanned_total)`.
    pub async fn search(&self, query: &str, limit: usize) -> anyhow::Result<(Vec<Record>, usize)> {
        let limit = limit.min(SEARCH_MAX_LIMIT);
        let file = match tokio::fs::File::open(&self.inner.path).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Vec::new(), 0));
            }
            Err(e) => return Err(e.into()),
        };
        let mut reader = BufReader::new(file).lines();
        let mut matches: Vec<Record> = Vec::new();
        let mut scanned = 0usize;
        while let Some(line) = reader.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            scanned += 1;
            let rec: Record = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if query.is_empty() || rec.body.contains(query) {
                matches.push(rec);
            }
        }
        // Return the most recent `limit` records.
        if matches.len() > limit {
            let drop = matches.len() - limit;
            matches.drain(..drop);
        }
        Ok((matches, scanned))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Record> {
        self.inner.tx.subscribe()
    }
}
```

Replace `crates/sift-server/src/state.rs`:

```rust
use std::path::Path;

use crate::store::Store;

/// Shared application state. Cheap to clone — everything inside is `Arc`-shared.
#[derive(Clone)]
pub struct AppState {
    store: Store,
}

impl AppState {
    /// Open the app state rooted at `data_path`. Creates parent directories
    /// as needed. The data file itself is created lazily on first write.
    pub fn open(data_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        // Use `block_in_place`? No — callers are sync (e.g. `main`). For M0
        // we accept a brief blocking open because tokio's `File::open` needs
        // a runtime. Instead we spin up our own tiny runtime to open the store.
        let path = data_path.as_ref().to_path_buf();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let store = rt.block_on(async move { Store::open(path).await })?;
        Ok(Self { store })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }
}
```

Update `crates/sift-server/src/lib.rs` to export the store:

```rust
//! sift-server — library surface used by integration tests and the binary.

pub mod routes;
pub mod state;
pub mod store;

pub use state::AppState;
pub use store::Store;

use axum::Router;

pub fn router(state: AppState) -> Router {
    Router::new().merge(routes::all()).with_state(state)
}
```

Create `crates/sift-server/src/routes/ingest.rs`:

```rust
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use sift_api::IngestResponse;
use sift_core::Record;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/ingest", post(ingest))
}

async fn ingest(State(state): State<AppState>, body: Bytes) -> impl IntoResponse {
    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let text = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "request body is not valid UTF-8").into_response();
        }
    };
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Record>(line) {
            Ok(record) => match state.store().append(record).await {
                Ok(()) => accepted += 1,
                Err(err) => {
                    tracing::error!(%err, "append failed");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "append failed").into_response();
                }
            },
            Err(err) => {
                tracing::debug!(%err, "rejecting malformed ingest line");
                rejected += 1;
            }
        }
    }
    (StatusCode::OK, Json(IngestResponse { accepted, rejected })).into_response()
}
```

Update `crates/sift-server/src/routes/mod.rs`:

```rust
use axum::Router;

use crate::state::AppState;

pub mod health;
pub mod ingest;

pub fn all() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(ingest::router())
}
```

- [ ] **Step 4: Run the tests — expect them to pass**

Run: `cargo test -p sift-server`
Expected: All tests pass (health + both ingest tests).

- [ ] **Step 5: End-to-end smoke test**

Terminal 1:
```
cargo run -p sift-server -- serve --data ./data/sift.ndjson
```

Terminal 2 (use single quotes so bash doesn't expand):
```
curl -X POST http://127.0.0.1:7789/v1/ingest -H 'content-type: application/x-ndjson' --data-binary $'{"timestamp":"2026-04-22T12:00:00Z","host":"h","app":"a","stream_id":"s","body":"hello"}\n'
```
Expected: `{"accepted":1,"rejected":0}`

Check: `cat ./data/sift.ndjson` should show one line.

Stop the server.

- [ ] **Step 6: Clippy + commit**

Run: `cargo clippy -p sift-server --all-targets -- -D warnings`

```bash
git add crates/sift-server
git commit -m "feat(server): add POST /v1/ingest with NDJSON store and broadcast"
```

---

## Task 6: `sift-server` — `GET /v1/search` (TDD)

**Files:**
- Create: `crates/sift-server/src/routes/search.rs`
- Modify: `crates/sift-server/src/routes/mod.rs`

- [ ] **Step 1: Write the integration test**

Create `crates/sift-server/tests/search.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use sift_api::{IngestResponse, SearchResponse};
use tower::ServiceExt;

async fn ingest_batch(app: &axum::Router, ndjson: &str) {
    let res = app
        .clone()
        .oneshot(
            Request::post("/v1/ingest")
                .header("content-type", "application/x-ndjson")
                .body(Body::from(ndjson.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    let _: IngestResponse = serde_json::from_slice(&body).unwrap();
}

#[tokio::test]
async fn search_returns_matching_records_only() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("d.ndjson")).unwrap();
    let app = sift_server::router(state);

    ingest_batch(
        &app,
        concat!(
            r#"{"timestamp":"2026-04-22T12:00:00Z","host":"h","app":"a","stream_id":"s","body":"hello world"}"#,
            "\n",
            r#"{"timestamp":"2026-04-22T12:00:01Z","host":"h","app":"a","stream_id":"s","body":"goodbye"}"#,
            "\n",
        ),
    )
    .await;

    let res = app
        .oneshot(
            Request::get("/v1/search?q=world&limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 65_536).await.unwrap();
    let resp: SearchResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.records.len(), 1);
    assert_eq!(resp.records[0].body, "hello world");
    assert_eq!(resp.scanned, 2);
}

#[tokio::test]
async fn search_empty_query_returns_all_records() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("d.ndjson")).unwrap();
    let app = sift_server::router(state);

    ingest_batch(
        &app,
        concat!(
            r#"{"timestamp":"2026-04-22T12:00:00Z","host":"h","app":"a","stream_id":"s","body":"one"}"#,
            "\n",
            r#"{"timestamp":"2026-04-22T12:00:01Z","host":"h","app":"a","stream_id":"s","body":"two"}"#,
            "\n",
        ),
    )
    .await;

    let res = app
        .oneshot(Request::get("/v1/search").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 65_536).await.unwrap();
    let resp: SearchResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.records.len(), 2);
}

#[tokio::test]
async fn search_honors_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("d.ndjson")).unwrap();
    let app = sift_server::router(state);

    let mut ndjson = String::new();
    for i in 0..5 {
        ndjson.push_str(&format!(
            r#"{{"timestamp":"2026-04-22T12:00:0{i}Z","host":"h","app":"a","stream_id":"s","body":"n{i}"}}"#
        ));
        ndjson.push('\n');
    }
    ingest_batch(&app, &ndjson).await;

    let res = app
        .oneshot(
            Request::get("/v1/search?limit=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(res.into_body(), 65_536).await.unwrap();
    let resp: SearchResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.records.len(), 2);
    assert_eq!(resp.scanned, 5);
    // `search()` returns the MOST RECENT `limit` records, so n3 and n4.
    assert_eq!(resp.records[0].body, "n3");
    assert_eq!(resp.records[1].body, "n4");
}
```

- [ ] **Step 2: Run the tests — expect them to fail**

Run: `cargo test -p sift-server --test search`
Expected: FAIL — `/v1/search` returns 404.

- [ ] **Step 3: Implement the handler**

Create `crates/sift-server/src/routes/search.rs`:

```rust
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use sift_api::{SearchQuery, SearchResponse};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/search", get(search))
}

async fn search(State(state): State<AppState>, Query(q): Query<SearchQuery>) -> impl IntoResponse {
    match state.store().search(&q.q, q.limit).await {
        Ok((records, scanned)) => {
            (StatusCode::OK, Json(SearchResponse { records, scanned })).into_response()
        }
        Err(err) => {
            tracing::error!(%err, "search failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "search failed").into_response()
        }
    }
}
```

Update `crates/sift-server/src/routes/mod.rs`:

```rust
use axum::Router;

use crate::state::AppState;

pub mod health;
pub mod ingest;
pub mod search;

pub fn all() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(ingest::router())
        .merge(search::router())
}
```

- [ ] **Step 4: Run the tests — expect them to pass**

Run: `cargo test -p sift-server`
Expected: All tests pass (health + ingest + search).

- [ ] **Step 5: Clippy + commit**

Run: `cargo clippy -p sift-server --all-targets -- -D warnings`

```bash
git add crates/sift-server
git commit -m "feat(server): add GET /v1/search substring query"
```

---

## Task 7: `sift-server` — `GET /v1/tail` WebSocket with broadcast fanout (TDD)

**Files:**
- Create: `crates/sift-server/src/routes/tail.rs`
- Modify: `crates/sift-server/src/routes/mod.rs`

- [ ] **Step 1: Write the integration test**

Tail tests need a real bound socket. Add `tokio-tungstenite` to `[dev-dependencies]` in `crates/sift-server/Cargo.toml`:

```toml
tokio-tungstenite = { workspace = true }
futures-util = { workspace = true }
```

Create `crates/sift-server/tests/tail.rs`:

```rust
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use sift_api::TailFrame;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

async fn spawn_server() -> (std::net::SocketAddr, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("d.ndjson")).unwrap();
    let app = sift_server::router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, tmp)
}

#[tokio::test]
async fn tail_receives_ingested_records() {
    let (addr, _tmp) = spawn_server().await;

    let ws_url = format!("ws://{addr}/v1/tail");
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();

    // Post one record via the REST API.
    let client = reqwest::Client::new();
    let line = r#"{"timestamp":"2026-04-22T12:00:00Z","host":"h","app":"a","stream_id":"s","body":"hello tail"}"#;
    let resp = client
        .post(format!("http://{addr}/v1/ingest"))
        .header("content-type", "application/x-ndjson")
        .body(format!("{line}\n"))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Expect a Record frame within 1s.
    let msg = tokio::time::timeout(Duration::from_secs(1), ws.next())
        .await
        .expect("tail frame timeout")
        .expect("stream closed")
        .expect("ws error");
    let text = match msg {
        Message::Text(t) => t,
        Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Close(_) | Message::Frame(_) => {
            panic!("unexpected frame: {msg:?}")
        }
    };
    let frame: TailFrame = serde_json::from_str(&text).unwrap();
    match frame {
        TailFrame::Record(r) => assert_eq!(r.body, "hello tail"),
        other => panic!("expected Record frame, got {other:?}"),
    }

    // Close the socket cleanly so the server doesn't spam warnings.
    let _ = ws.send(Message::Close(None)).await;
}
```

Add the `tokio-tungstenite` workspace dep (already in workspace) and `futures-util` to `[dev-dependencies]`.

- [ ] **Step 2: Run the test — expect it to fail**

Run: `cargo test -p sift-server --test tail`
Expected: FAIL — WebSocket handshake returns 404 (route not registered) or connection refused.

- [ ] **Step 3: Implement the handler**

Create `crates/sift-server/src/routes/tail.rs`:

```rust
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use sift_api::TailFrame;
use sift_core::Record;
use tokio::sync::broadcast::error::RecvError;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/tail", get(tail))
}

async fn tail(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    let rx = state.store().subscribe();
    ws.on_upgrade(move |socket| handle_tail(socket, rx))
}

async fn handle_tail(socket: WebSocket, mut rx: tokio::sync::broadcast::Receiver<Record>) {
    let (mut sink, mut stream) = socket.split();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.tick().await; // fire the first tick immediately

    loop {
        tokio::select! {
            next = rx.recv() => match next {
                Ok(record) => {
                    if !send_frame(&mut sink, &TailFrame::Record(record)).await {
                        break;
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    let msg = format!("tail lagged by {n} records — reconnect");
                    if !send_frame(&mut sink, &TailFrame::Error { message: msg }).await {
                        break;
                    }
                    // After lag, the receiver is still valid; continue.
                }
                Err(RecvError::Closed) => break,
            },
            _ = heartbeat.tick() => {
                if !send_frame(&mut sink, &TailFrame::Heartbeat).await {
                    break;
                }
            }
            maybe_msg = stream.next() => {
                match maybe_msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        tracing::debug!(%e, "tail client error");
                        break;
                    }
                    // Ignore pings/pongs and any client-sent payload.
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

async fn send_frame(
    sink: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    frame: &TailFrame,
) -> bool {
    match serde_json::to_string(frame) {
        Ok(text) => sink.send(Message::Text(text)).await.is_ok(),
        Err(err) => {
            tracing::error!(%err, "serialize tail frame");
            false
        }
    }
}
```

Update `crates/sift-server/src/routes/mod.rs`:

```rust
use axum::Router;

use crate::state::AppState;

pub mod health;
pub mod ingest;
pub mod search;
pub mod tail;

pub fn all() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(ingest::router())
        .merge(search::router())
        .merge(tail::router())
}
```

- [ ] **Step 4: Run the test — expect it to pass**

Run: `cargo test -p sift-server --test tail`
Expected: 1 test passes.

- [ ] **Step 5: Run the whole server test suite**

Run: `cargo test -p sift-server`
Expected: All tests pass.

- [ ] **Step 6: Clippy + commit**

Run: `cargo clippy -p sift-server --all-targets -- -D warnings`

```bash
git add crates/sift-server
git commit -m "feat(server): add WebSocket /v1/tail with broadcast fanout"
```

---

## Task 8: `sift-cli` — `send`, `search`, `tail` commands

**Files:**
- Modify: `crates/sift-cli/src/main.rs`

- [ ] **Step 1: Replace `crates/sift-cli/src/main.rs`**

```rust
use std::collections::BTreeMap;

use anyhow::{bail, Context};
use chrono::Utc;
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use sift_api::{IngestResponse, SearchResponse, TailFrame};
use sift_core::{Level, Record};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Parser, Debug)]
#[command(name = "sift", version, about = "Sift command-line client")]
struct Cli {
    /// HTTP base URL of the Sift server.
    #[arg(long, default_value = "http://127.0.0.1:7789", global = true)]
    server: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Send a single record.
    Send {
        /// Log body (stdin if omitted).
        body: Option<String>,
        #[arg(long, default_value = "localhost")]
        host: String,
        #[arg(long, default_value = "cli")]
        app: String,
        #[arg(long)]
        stream: Option<String>,
        #[arg(long, default_value = "info")]
        level: String,
    },
    /// Search records by body substring.
    Search {
        query: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Live-tail records.
    Tail,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Send { body, host, app, stream, level } => {
            send(&cli.server, body, host, app, stream, &level).await
        }
        Cmd::Search { query, limit } => search(&cli.server, &query, limit).await,
        Cmd::Tail => tail(&cli.server).await,
    }
}

async fn send(
    base: &str,
    body: Option<String>,
    host: String,
    app: String,
    stream: Option<String>,
    level: &str,
) -> anyhow::Result<()> {
    let body = match body {
        Some(b) => b,
        None => {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)?;
            s
        }
    };
    let level: Level = serde_json::from_value(serde_json::Value::String(level.to_string()))
        .with_context(|| format!("unknown level {level:?}"))?;
    let stream_id = stream.unwrap_or_else(|| app.clone());
    let record = Record {
        timestamp: Utc::now(),
        host,
        app,
        stream_id,
        level,
        body,
        labels: BTreeMap::new(),
        fields: BTreeMap::new(),
        raw: None,
    };
    let ndjson = format!("{}\n", serde_json::to_string(&record)?);
    let url = format!("{}/v1/ingest", base.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .post(&url)
        .header("content-type", "application/x-ndjson")
        .body(ndjson)
        .send()
        .await?
        .error_for_status()?;
    let r: IngestResponse = resp.json().await?;
    println!("accepted={} rejected={}", r.accepted, r.rejected);
    Ok(())
}

async fn search(base: &str, query: &str, limit: usize) -> anyhow::Result<()> {
    let url = format!("{}/v1/search", base.trim_end_matches('/'));
    let resp: SearchResponse = reqwest::Client::new()
        .get(&url)
        .query(&[("q", query), ("limit", &limit.to_string())])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    for rec in resp.records {
        println!(
            "{ts} {host} {app} {lvl:?} {body}",
            ts = rec.timestamp.to_rfc3339(),
            host = rec.host,
            app = rec.app,
            lvl = rec.level,
            body = rec.body
        );
    }
    eprintln!("(scanned {})", resp.scanned);
    Ok(())
}

async fn tail(base: &str) -> anyhow::Result<()> {
    let ws_url = base
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    let ws_url = format!("{}/v1/tail", ws_url.trim_end_matches('/'));
    let (mut ws, _) = connect_async(&ws_url)
        .await
        .with_context(|| format!("connecting to {ws_url}"))?;
    while let Some(msg) = ws.next().await {
        let msg = msg?;
        match msg {
            Message::Text(text) => match serde_json::from_str::<TailFrame>(&text)? {
                TailFrame::Record(r) => println!(
                    "{ts} {host} {app} {lvl:?} {body}",
                    ts = r.timestamp.to_rfc3339(),
                    host = r.host,
                    app = r.app,
                    lvl = r.level,
                    body = r.body
                ),
                TailFrame::Heartbeat => {}
                TailFrame::Error { message } => eprintln!("server error: {message}"),
            },
            Message::Close(_) => break,
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
    bail!("tail stream closed")
}
```

- [ ] **Step 2: Build the CLI**

Run: `cargo build -p sift-cli`
Expected: Build succeeds.

- [ ] **Step 3: End-to-end smoke test**

Terminal 1: `cargo run -p sift-server -- serve --data ./data/sift.ndjson`
Terminal 2: `cargo run -p sift-cli -- tail`
Terminal 3: `cargo run -p sift-cli -- send 'hello from cli'`

Expected:
- Terminal 2 prints a line like `2026-04-22T... localhost cli Info hello from cli`
- Terminal 3 prints `accepted=1 rejected=0`

Then: `cargo run -p sift-cli -- search hello`
Expected: The record we just sent is printed.

Stop server.

- [ ] **Step 4: Clippy + commit**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings`

```bash
git add crates/sift-cli
git commit -m "feat(cli): add send, search, and tail subcommands"
```

---

## Task 9: `sift-ui` — Leptos live-tail page (manual verification)

**Files:**
- Create: `ui/sift-ui/Cargo.toml`
- Create: `ui/sift-ui/Trunk.toml`
- Create: `ui/sift-ui/index.html`
- Create: `ui/sift-ui/style.css`
- Create: `ui/sift-ui/src/main.rs`
- Create: `ui/sift-ui/src/app.rs`

- [ ] **Step 1: Install `trunk`**

Run: `cargo install trunk --locked`
Expected: Installs successfully. `trunk --version` prints a version.

This can take several minutes.

- [ ] **Step 2: Create the Leptos crate manifest**

Create `ui/sift-ui/Cargo.toml`:

```toml
[package]
name = "sift-ui"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
publish = false

[dependencies]
leptos = { version = "0.6", features = ["csr"] }
wasm-bindgen = "0.2"
console_error_panic_hook = "0.1"
web-sys = { version = "0.3", features = [
    "WebSocket",
    "MessageEvent",
    "CloseEvent",
    "Event",
    "BinaryType",
] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
wasm-bindgen-futures = "0.4"
log = "0.4"
console_log = { version = "1", features = ["color"] }

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
strip = true
```

We intentionally *do not* depend on `sift-api` or `sift-core` from the UI in M0. Those crates aren't configured for wasm32 yet (chrono features), and duplicating a tiny local frame type avoids a target-cfg rabbit hole. Wire them up in M1 when we harden the API boundary.

- [ ] **Step 3: Write `ui/sift-ui/Trunk.toml`**

```toml
[build]
target = "index.html"
dist = "dist"

[serve]
address = "127.0.0.1"
port = 8080
```

- [ ] **Step 4: Write `ui/sift-ui/index.html`**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Sift — live tail</title>
    <link data-trunk rel="css" href="style.css" />
    <link data-trunk rel="rust" data-wasm-opt="z" />
  </head>
  <body>
    <div id="app"></div>
  </body>
</html>
```

- [ ] **Step 5: Write `ui/sift-ui/style.css`**

```css
:root {
  --bg: #0e0f12;
  --fg: #d8dee4;
  --muted: #7a8290;
  --accent: #67c98d;
  --warn: #e6c07b;
  --err: #e06c75;
}
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; background: var(--bg); color: var(--fg); font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; height: 100%; }
header { padding: 10px 16px; border-bottom: 1px solid #22252b; display: flex; align-items: center; justify-content: space-between; }
header h1 { margin: 0; font-size: 14px; letter-spacing: 0.1em; text-transform: uppercase; color: var(--muted); }
.status { font-size: 12px; color: var(--muted); }
.status.ok { color: var(--accent); }
.status.err { color: var(--err); }
main { padding: 0 16px; }
.row { display: grid; grid-template-columns: 22ch 12ch 10ch 8ch 1fr; gap: 12px; padding: 4px 0; border-bottom: 1px dotted #1a1c21; }
.row .ts { color: var(--muted); }
.row .lvl.error { color: var(--err); }
.row .lvl.warn { color: var(--warn); }
.row .lvl.info { color: var(--accent); }
.empty { padding: 24px; color: var(--muted); }
```

- [ ] **Step 6: Write `ui/sift-ui/src/main.rs`**

```rust
mod app;

fn main() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);
    leptos::mount_to_body(app::App);
}
```

- [ ] **Step 7: Write `ui/sift-ui/src/app.rs`**

```rust
use leptos::*;
use serde::Deserialize;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

/// Minimal local mirror of `sift_api::TailFrame`. See the note in the crate
/// manifest for why the UI does not depend on sift-api in M0.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TailFrame {
    Record(TailRecord),
    Heartbeat,
    Error { message: String },
}

#[derive(Debug, Clone, Deserialize)]
struct TailRecord {
    timestamp: String,
    host: String,
    app: String,
    #[serde(default)]
    level: String,
    body: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Connecting,
    Open,
    Closed,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Open => "connected",
            Self::Closed => "disconnected",
        }
    }
    fn css(self) -> &'static str {
        match self {
            Self::Open => "status ok",
            Self::Closed => "status err",
            Self::Connecting => "status",
        }
    }
}

#[component]
pub fn App() -> impl IntoView {
    let records = create_rw_signal(Vec::<TailRecord>::new());
    let status = create_rw_signal(Status::Connecting);

    connect(records, status);

    view! {
        <header>
            <h1>"sift · live tail"</h1>
            <span class=move || status.get().css()>
                { move || status.get().label() }
            </span>
        </header>
        <main>
            { move || {
                let rs = records.get();
                if rs.is_empty() {
                    view! { <div class="empty">"waiting for records…"</div> }.into_view()
                } else {
                    view! {
                        <For
                            each=move || records.get().into_iter().enumerate().collect::<Vec<_>>()
                            key=|(i, _)| *i
                            children=move |(_, r)| view! {
                                <div class="row">
                                    <span class="ts">{r.timestamp.clone()}</span>
                                    <span class="host">{r.host.clone()}</span>
                                    <span class="app">{r.app.clone()}</span>
                                    <span class=format!("lvl {}", r.level)>{r.level.clone()}</span>
                                    <span class="body">{r.body.clone()}</span>
                                </div>
                            }
                        />
                    }.into_view()
                }
            }}
        </main>
    }
}

fn connect(records: RwSignal<Vec<TailRecord>>, status: RwSignal<Status>) {
    let location = web_sys::window().and_then(|w| w.location().host().ok()).unwrap_or_default();
    let proto = web_sys::window()
        .and_then(|w| w.location().protocol().ok())
        .unwrap_or_else(|| "http:".to_string());
    let ws_scheme = if proto == "https:" { "wss" } else { "ws" };
    let url = format!("{ws_scheme}://{location}/v1/tail");

    let ws = match WebSocket::new(&url) {
        Ok(ws) => ws,
        Err(_) => {
            status.set(Status::Closed);
            return;
        }
    };

    let on_open_status = status;
    let on_open = Closure::<dyn FnMut(Event)>::new(move |_| on_open_status.set(Status::Open));
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();

    let records_for_msg = records;
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
        let Some(text) = evt.data().as_string() else { return };
        match serde_json::from_str::<TailFrame>(&text) {
            Ok(TailFrame::Record(r)) => records_for_msg.update(|v| {
                v.push(r);
                if v.len() > 500 { v.drain(..v.len() - 500); }
            }),
            Ok(TailFrame::Heartbeat) => {}
            Ok(TailFrame::Error { message }) => {
                log::warn!("server error frame: {message}");
            }
            Err(e) => log::error!("bad frame: {e}"),
        }
    });
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();

    let on_close_status = status;
    let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_| on_close_status.set(Status::Closed));
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    on_close.forget();

    let on_error_status = status;
    let on_error = Closure::<dyn FnMut(Event)>::new(move |_| on_error_status.set(Status::Closed));
    ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    on_error.forget();
}
```

- [ ] **Step 8: Build the UI**

Run: `cd ui/sift-ui && trunk build --release`
Expected: Succeeds. `ui/sift-ui/dist/` contains `index.html`, a hashed `.wasm`, a hashed `.js`, and `style.css`.

- [ ] **Step 9: Check bundle size budget**

Run (bash):
```
find ui/sift-ui/dist -name '*.wasm' -exec ls -l {} \;
```

Expected: The `.wasm` is under ~900 KB uncompressed. If trunk emitted a `.wasm.br` or `.wasm.gz`, check that. The PLAN's 600 KB *gzipped* budget is a CI rule we enforce in Task 10 — this step is informational.

- [ ] **Step 10: Manual smoke test (dev mode)**

Terminal 1: `cargo run -p sift-server -- serve --data ./data/sift.ndjson`
Terminal 2: `cd ui/sift-ui && trunk serve --proxy-backend http://127.0.0.1:7789`

Open http://127.0.0.1:8080 in a browser. Status should say "connected".

Terminal 3: `cargo run -p sift-cli -- send 'hello from browser'`

Expected: The record appears in the browser within a second.

Stop everything.

- [ ] **Step 11: Commit**

```bash
git add ui/
git commit -m "feat(ui): add Leptos live-tail page"
```

---

## Task 10: `sift-server` — serve the UI `dist/` directory

**Files:**
- Modify: `crates/sift-server/src/main.rs`
- Create: `crates/sift-server/src/ui.rs`
- Modify: `crates/sift-server/src/lib.rs`

- [ ] **Step 1: Write an integration test for static file serving**

Create `crates/sift-server/tests/ui.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn serves_index_html_from_dist_when_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let dist = tmp.path().join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), b"<!doctype html>hello").unwrap();

    let state = sift_server::AppState::open(tmp.path().join("d.ndjson")).unwrap();
    let app = sift_server::router_with_ui(state, Some(dist));

    let res = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    assert_eq!(&body[..], b"<!doctype html>hello");
}

#[tokio::test]
async fn root_returns_404_when_ui_not_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sift_server::AppState::open(tmp.path().join("d.ndjson")).unwrap();
    let app = sift_server::router_with_ui(state, None);

    let res = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run the test — expect it to fail**

Run: `cargo test -p sift-server --test ui`
Expected: FAIL — `router_with_ui` is not defined.

- [ ] **Step 3: Implement static UI serving**

Create `crates/sift-server/src/ui.rs`:

```rust
use std::path::PathBuf;

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

use crate::state::AppState;

/// Serve the pre-built `dist/` directory at `/`, with SPA-style fallback to
/// `index.html` for unmatched paths (no SPA routing in M0, but this keeps the
/// behaviour consistent when we add client-side routes in M2+).
pub fn router(dist: PathBuf) -> Router<AppState> {
    let index = dist.join("index.html");
    let service = ServeDir::new(dist).not_found_service(ServeFile::new(index));
    Router::new().fallback_service(service)
}
```

Replace `crates/sift-server/src/lib.rs`:

```rust
//! sift-server — library surface used by integration tests and the binary.

use std::path::PathBuf;

use axum::Router;

pub mod routes;
pub mod state;
pub mod store;
pub mod ui;

pub use state::AppState;
pub use store::Store;

/// Router with API routes only. Root path returns 404. Used by tests that
/// don't care about UI serving.
pub fn router(state: AppState) -> Router {
    router_with_ui(state, None)
}

/// Router with optional static UI serving mounted at `/`.
pub fn router_with_ui(state: AppState, ui_dist: Option<PathBuf>) -> Router {
    let mut app = Router::new().merge(routes::all());
    if let Some(dist) = ui_dist {
        app = app.merge(ui::router(dist));
    }
    app.with_state(state)
}
```

Update `crates/sift-server/src/main.rs` to pass `ui_dist`:

```rust
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "sift-server", version, about = "Sift log platform")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    Serve {
        #[arg(long, default_value = "./data/sift.ndjson")]
        data: PathBuf,
        #[arg(long, default_value = "127.0.0.1:7789")]
        bind: SocketAddr,
        #[arg(long)]
        ui_dist: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sift_server=debug")))
        .init();

    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Serve { data, bind, ui_dist } => {
            let state = sift_server::AppState::open(&data)
                .with_context(|| format!("opening data file at {}", data.display()))?;
            let app = sift_server::router_with_ui(state, ui_dist.clone());

            tracing::info!(%bind, data = %data.display(), ui = ?ui_dist, "starting sift-server");
            let listener = tokio::net::TcpListener::bind(bind).await?;
            axum::serve(listener, app).await?;
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run the test — expect it to pass**

Run: `cargo test -p sift-server --test ui`
Expected: 2 tests pass.

- [ ] **Step 5: End-to-end smoke test through the single binary**

Prereq: Task 9's `trunk build --release` completed so `ui/sift-ui/dist/` exists.

Terminal 1:
```
cargo run -p sift-server -- serve --data ./data/sift.ndjson --ui-dist ./ui/sift-ui/dist
```

Browser: open http://127.0.0.1:7789 — the UI loads from the same port, status "connected".

Terminal 2: `cargo run -p sift-cli -- send 'via single binary'`
Expected: record appears in the browser.

Stop server.

- [ ] **Step 6: Clippy + commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/sift-server
git commit -m "feat(server): serve UI dist/ directory from the same binary"
```

---

## Task 11: CI — GitHub Actions workflow

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: -D warnings

jobs:
  workspace:
    name: build · test · lint (workspace)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.94
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace --all-targets

  ui:
    name: build · size-check (sift-ui)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.94
        with:
          targets: wasm32-unknown-unknown
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: ui/sift-ui
      - name: install trunk
        run: cargo install trunk --locked --version ^0.21
      - name: build ui
        working-directory: ui/sift-ui
        run: trunk build --release
      - name: check gzipped wasm size budget (600 KB)
        run: |
          set -euo pipefail
          wasm=$(ls ui/sift-ui/dist/*.wasm | head -n1)
          gz=$(gzip -c -9 "$wasm" | wc -c)
          echo "gzipped wasm size: $gz bytes"
          if [ "$gz" -gt 614400 ]; then
            echo "::error::gzipped wasm ${gz} bytes exceeds 600 KB budget"
            exit 1
          fi
```

Notes:
- The budget check is a hard fail, matching PLAN §4.5 ("Fail CI if gzipped `.wasm` > 600 KB").
- We pin the toolchain to 1.94 to match `rust-toolchain.toml`; when we bump it we bump both in one commit.
- `RUSTFLAGS: -D warnings` makes any warnings a build failure in CI, which matches local clippy discipline.

- [ ] **Step 2: (Optional) verify YAML syntax locally**

Run (only if `yamllint` or similar is installed — skip otherwise): `yamllint .github/workflows/ci.yml`

- [ ] **Step 3: Commit**

```bash
git add .github
git commit -m "ci: add workspace build/test/lint + ui size-budget workflow"
```

---

## Self-review

**Spec coverage (against PLAN.md §10 "Immediate next step" and M0 in §7):**
- "Scaffold the Cargo workspace with `sift-core`, `sift-server`, `sift-cli`, and `sift-ui`" — Tasks 1, 2, 3, 4, 8, 9.
- "Wire `POST /ingest` → flat file → WS tail" — Tasks 5, 7. (Route path is `/v1/ingest` and `/v1/tail` to match §4.1 URI style.)
- "A Leptos page that live-tails via WebSocket" — Task 9.
- "`GET /search?q=…` greps it" — Task 6. (Route is `/v1/search`, same versioning reason.)
- "No storage engine yet" — Task 5's `Store` is a flat NDJSON appender, intentionally not a segmented store.
- "End-to-end loop" — Task 10 wires the UI into the server binary; verified via the manual smoke test.
- "CI, release pipeline" — Task 11 covers CI. A release pipeline (tagged builds, artifact upload) is *not* covered here; M0 only needs CI green. Flag for M6 hardening.
- "Workspace layout" — Task 1 establishes the layout from PLAN §6. Future-milestone crates (`sift-ingest`, `sift-storage`, `sift-query`, `sift-plugin-host`) are intentionally not scaffolded yet — we add them when they have real content to hold. This is a deliberate deviation from §6 to keep M0 lean; note it in the M0 wrap-up.
- "Run the crates.io / NPM / USPTO availability check on 'Sift'" — out of scope for engineer execution; this is a founder task. Not in any plan task on purpose.

**Deliberate deviations from PLAN.md worth calling out:**
- `sift-api` is split out immediately (not in §6) so tests can use it; harmless.
- Broadcast capacity is 1024; PLAN doesn't specify. Reasonable default; revisit at M1.
- UI does not depend on `sift-api`/`sift-core` in M0 — see Task 9, Step 2 note.

**Placeholder scan:** searched the plan for "TBD", "TODO", "…", "fill in", "similar to" — none present in task content. Every code step shows the full code. Every command shows the exact invocation and expected result.

**Type consistency:**
- `AppState::open(path)` — sync in Task 4, kept sync in Task 5 by spinning up a small tokio runtime inside `open` so callers don't change.
- `Store::append(record)`, `Store::search(query, limit)`, `Store::subscribe()` — same signatures used in Tasks 5, 6, 7.
- `TailFrame` — `{ "type": "record" | "heartbeat" | "error", ... }` tagged form used by server (Task 7) and UI/CLI mirror type (Task 8, 9).
- `IngestResponse { accepted, rejected }`, `SearchQuery { q, limit }`, `SearchResponse { records, scanned }` — consistent across Tasks 3, 5, 6, 8.
- `router()` in Task 4 is replaced by `router()` + `router_with_ui()` in Task 10; Task 4's test keeps using `router()` and still compiles because Task 10 preserves that function's signature.

**One consistency fix applied inline:** the ingest handler in Task 5 returns `IngestResponse` via `Json`, which matches the `SearchResponse` JSON return convention in Task 6.

---

Plan complete and saved to `docs/superpowers/plans/2026-04-22-m0-walking-skeleton.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
