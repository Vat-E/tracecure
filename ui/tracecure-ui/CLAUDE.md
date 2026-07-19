# tracecure-ui — Leptos 0.8 CSR / wasm32

Loaded when working under `ui/tracecure-ui/`. Non-obvious things that have
cost real debugging time here.

## This crate is outside the cargo workspace

Root `Cargo.toml` excludes it (wasm32 target features are incompatible with
the server binaries). Consequences:

- `cargo test --workspace` from the repo root does **not** cover this crate.
  Run tests from inside `ui/tracecure-ui/`.
- `cargo clippy --target wasm32-unknown-unknown --all-targets` is the check
  that matters; the workspace clippy run never sees this code.
- `AppState` and everything in `src/state.rs`, `inbox.rs`, `command.rs`,
  `keys.rs`, `jsonfmt.rs`, `query.rs` is plain Rust in the **lib** target and
  is unit-testable on the host — `cargo test --lib`. Components and `ws.rs`
  are in the **bin** target and are not.

Keep logic in the lib so it stays testable off-wasm. That is why `AppState`
has no Leptos types in it.

## Reactivity: signal granularity is load-bearing, not style

`RwSignal::update` notifies **every** subscriber unconditionally — there is no
field-level diffing and no `PartialEq` gate. A single coarse
`RwSignal<AppState>` therefore means one arriving log record wakes every
closure in every mounted row.

That is why `Selection`, `ViewOpts` and the clock live in **separate
contexts** (`SelectionCtx`, `ViewCtx`, `ClockCtx` in `src/app.rs`) rather than
inside `AppState`. `components/row.rs` deliberately never reads `AppCtx`.
Before adding a signal read to a component that renders per-record, ask what
wakes it.

## The `<For>` key must be `Entry::seq`, never a positional index

`records` is a ring buffer capped at `RECORD_CAP`. At the cap, `push_record`
does `pop_front` + `push_back`, so a positional key set is **unchanged**
(`{0..CAP-1}`) even though every record shifted a slot. A keyed reconciler
correctly concludes there is nothing to patch, and the rendered rows silently
detach from the data while still costing full render work.

`Entry.seq` is a stable per-record identity that survives eviction. Selection
is keyed by it too, so it follows its record.

## Wall-clock rendering needs an explicit dependency

Anything whose output depends on `Utc::now()` rather than on state — relative
timestamps, the rolling histogram window — must `track()` `ClockCtx`, and
only on the branch that needs it (Leptos re-collects dependencies per run, so
an absolute-mode row takes no dependency). There is no ambient periodic
re-render to rely on.

## Do not scroll the container per batch

`scrollTop = scrollHeight` against a `content-visibility` container forces a
layout. Doing it per WS flush locked the main thread under load. See the note
on `AppState::push_batch`. Pausing should be driven by user-intent events
(wheel/drag/keys), not by computed scroll position.

## Verifying in a browser

`trunk serve`'s WebSocket proxy accepts the `/v1/tail` upgrade and then
forwards **no frames** — the UI backfills but sits on "connecting…" forever.
Use the server to serve the built assets instead:

```
trunk build --release
cargo run -p tracecure-server -- serve --data <tmp> --bind 127.0.0.1:7999 \
  --ui-dist ui/tracecure-ui/dist
```

**Measurement trap:** an occluded Chrome window reports
`document.visibilityState === "visible"` while starving `requestAnimationFrame`
(~2 frames/1.5 s) and throttling `setTimeout`. That looks identical to a frozen
renderer and has produced false conclusions here. Before trusting any perf
number, assert `document.hidden === false` **and** that a probe sees >20
frames per 1.5 s. Drive load from outside the browser.

## Bundle budget

CI fails if the gzipped `.wasm` exceeds 600 KB (currently ~247 KB). Note
`index.html` sets `data-wasm-opt="0"`, so trunk's `wasm-opt` pass is off; the
release profile (`opt-level = "z"`, `lto`, `codegen-units = 1`, `strip`)
carries the size instead.
