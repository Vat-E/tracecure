# sift-ui

Leptos 0.6 CSR browser UI for Sift. Served by IIS in development (see `web.config`) and by any static host in production. The server lives in `crates/sift-server`; this crate only proxies to it via `/v1/*`.

## Build

```powershell
cd ui/sift-ui
trunk build --release
```

Or use the repo-root wrapper:

```powershell
.\deploy.ps1 -Ui
```

## Features

- **Tail route (`/tail`)** — live WebSocket tail of `/v1/tail`; 500-record ring buffer; substring filter; pause/resume with pending queue.
- **Search route (`/search`)** — query + from/to (RFC3339 UTC) + quick ranges (1h/24h/7d); results table; click-to-detail.
- **Record detail panel** — right-side slide-in; full record shape including labels, JSON-highlighted fields, and `_raw`.
- **Level toggle** — pill row in the top shell; applies to tail + search results; `more…` exposes trace/debug.
- **Help modal** — press `?`; contains the keybinding reference.
- **Auto-reconnect** — exponential 1s→30s cap; `Retry now` button while in backoff.

## Smoke matrix

Run after every UI task. `S` = section headers to verify; `✓` = expected result.

### S1 — Routing
- `/` → redirects to `/tail`. ✓
- Click `Search` tab → URL becomes `/search`. ✓
- Click `Tail` tab → URL becomes `/tail`. ✓
- Browser back/forward navigates between the two. ✓

### S2 — Live tail
- Records arrive in real time (`sift send hello-1`, `hello-2`, …). ✓
- Ring buffer caps at 500 (send 600; oldest 100 drop off). ✓
- Level pills: toggle `warn` off → warn records disappear; toggle back → reappear. ✓
- Typing in the filter input narrows the list. ✓
- `Pause` button stops updates; counter ticks: `Resume (N new)`. ✓
- `Resume` appends the N pending records, counter resets. ✓

### S3 — Search
- Empty query + `Last 1h` → recent records. ✓
- Query `error` + `Search` button → matching records. ✓
- Query + manual `From`/`To` → filtered. ✓
- Status line: `N results · scanned M · X ms`. ✓
- Click a row → detail panel. ✓

### S4 — Detail panel
- Body, Labels, Fields, Raw sections visible. ✓
- `fields` JSON colorized (keys green, strings light, numbers yellow, punctuation grey). ✓
- `Esc`, `×`, and clicking outside all close the panel. ✓

### S5 — Help modal
- `?` key → opens modal (NOT while an input is focused). ✓
- `Esc` closes it. ✓

### S6 — Auto-reconnect
- `nssm restart Sift` → status pill shows `reconnecting (attempt N, next in Ks)`. ✓
- UI reconnects automatically when the service is back. ✓
- Click `Retry now` → immediate attempt. ✓

## Known gaps

- Times in the `<input type="datetime-local">` pickers are treated as UTC. A TZ-aware picker lands in a future UX pass.
- The UI crate is excluded from the workspace (wasm32 target).
- No native unit test harness for Leptos components; the `json_highlight` tokenizer is the exception (native-testable).
