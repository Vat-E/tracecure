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
