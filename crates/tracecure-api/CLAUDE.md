# tracecure-api — the wire contract

Loaded when working under `crates/tracecure-api/`. These types are shared by
the server, the CLI/TUI and the WASM UI. **Any change here is a breaking
change to three clients and to `docs/book/src/integration.md` simultaneously.**

## The serde attributes are the contract, not an implementation detail

`TailFrame` is **internally tagged**:

```rust
#[serde(tag = "type", rename_all = "snake_case")]
```

So a record frame goes on the wire as `{"type":"record", ...record fields...}`
— the tag is merged into the record's own object, not nested. Two consequences
that have already caused bugs:

- Deserializing a frame as a bare `Record` *appears* to work, because serde
  ignores the unknown `"type"` key. It silently drops every non-record frame,
  including the server's `error` lag warning. Always match on `TailFrame`.
- `#[serde(other)] Unknown` exists so an older client does not hard-fail on a
  variant a newer server emits. Keep it, and keep new variants additive.

## `/v1/search` returns an envelope, not an array

`SearchResponse { records, scanned }`. A client deserializing `Vec<Record>`
fails at runtime with a parse error, not a compile error — this shipped once
and made the whole search path silently dead.

## `SearchQuery`'s filters are strings, and absent means "any"

`level`, `app`, `host` and `stream_id` are `Option<String>` holding a
comma-separated list, not `Vec<String>`: this type *is* a URL query string, and
a repeated key does not survive every client or `serde_urlencoded`. They are
`skip_serializing_if = "Option::is_none"`, so a request from a new client to
an older server contains nothing that server does not know.

Parsing and matching live in `tracecure_storage::SearchFilter`, not here. An
unknown `level` name is a `400` — never "no filter".

## `/v1/query`'s cursor is opaque on purpose

`QueryResponse::next_cursor` is a `String` the client hands back verbatim as
`QueryRequest::cursor`. Today it encodes a timestamp; the type is a string so
that it can come to encode more (a shard, a sequence number) without a wire
change. Do not give clients a reason to parse it — no typed cursor struct in
this crate. The decision is in `docs/decisions/ADR-006-tracecureql-without-datafusion.md`.

`next_cursor` is omitted, not `null`, on the last page; a `400` carries
`QueryError { error, at }` as JSON, unlike `/v1/search`'s plain-text `400`.

## Query-string timestamps must not carry a `+` offset

`SearchQuery::from`/`to` are `Option<DateTime<Utc>>`. `DateTime::to_rfc3339()`
emits `+00:00`; a raw `+` in a query string decodes to a space under
`application/x-www-form-urlencoded`, and axum's `Query<SearchQuery>` rejects
the request with **400 before the handler runs**.

Emit the `Z` form and percent-encode it:
`to_rfc3339_opts(SecondsFormat::Millis, true)` then URL-encode.

## Defaults live in serde, not in callers

`SearchQuery::limit` defaults to 100 via `default_limit()`, shared between the
serde attribute and the manual `Default` impl so the two cannot drift. There
is **no server-side maximum** on `limit` — do not document one that does not
exist.

## When you change anything here

Update in the same commit:
- `docs/book/src/` integration chapter (the published contract),
- `crates/tracecure-cli/` and `ui/tracecure-ui/src/query.rs` (the consumers),
- the round-trip tests in this crate — they are the only thing that catches a
  wire-shape change at compile/test time.
