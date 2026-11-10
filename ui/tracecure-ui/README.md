# tracecure-ui

Leptos 0.8 (CSR) web UI for Tracecure — single unified log stream.

## Routes

Just `/`. Legacy `/tail` and `/search` URLs redirect to `/` on load, preserving `?q=` if present.

The view is carried in the URL so it can be bookmarked or shared: `?q=` the query box,
`?w=` the preset range in seconds, `?lv=` the enabled levels as letters (`tdiwef`).
Anything at its default is omitted, so the default view is a bare `/`.

## Sign-in

The UI has no login screen. If the server requires sign-in, it redirects a page
load to its provider before the UI is served at all. What the UI does itself:

- asks `/auth/me` at start and, when signed in, shows a sign-out button in the
  rail. The button posts to `/auth/logout` and goes where the reply says —
  the provider's sign-out, or the server's "signed out" page. It never just
  reloads: that would walk into the sign-in redirect and come back signed in;
- turns a `401` from `/v1/search`, or a live tail that will not connect, into a
  redirect to `/auth/login` that comes back to the same view — a session can
  expire, or the server restart, while the page is open.

Against a server without sign-in none of this is visible.

## What is on screen

The list is a **buffer**, not the store: the newest records of the selected time range
that match what the controls ask for — 500 at load, 2000 after an explicit search or a
range or level change, at most 5000. The server does the matching (text, level, app, host,
stream) before it applies that limit, so "errors in the last 24 h" really is the newest
errors, not the errors among the newest records. The histogram and the rate are computed
over the buffer. The UI says so whenever it matters: the count reads "shown of loaded", the status
bar says "(newest only)" when the server had more, and a note above the oldest row offers
to load up to the cap.

Times are shown in the browser's time zone (labelled in the status bar); a record from
an earlier day shows its date instead of milliseconds. The detail pane has both the local
time and the exact UTC instant.

## Query box

Typing filters the loaded buffer immediately. **Enter** sends the whole filter to the
server and reloads the buffer for the time range — the only way to reach records older
than what is loaded. While the two differ, the status bar says so.

The **level buttons** are part of the same request: changing one reloads the buffer, so
a hidden level does not take up room in it.

```
level:error,fatal app:billing-* host:edge-1 stream:api-prod connection timeout
```

Input that starts with `{` or `|` is a **TracecureQL** query instead — the same box,
a second syntax, for what the first cannot say: labels, "is not", more than one
text condition.

```
{app="billing-*", env="prod"} | level!="debug,trace" | body ~ "timeout" | body !~ "retry"
```

The language is described in the integration guide (`docs/book/src/integration.md`,
"TracecureQL"). In the UI: typing filters the loaded buffer with it as with the plain
syntax; a query that does not parse yet hides nothing; Enter sends it to `/v1/query`,
or reports why it is not a query. The level buttons apply to it as one more stage.

- `level:` (alias `lvl:`), `app:`, `host:`, `stream:` are structured and comma-separated.
  A value ending in `*` is a prefix match. `level:` narrows within the levels whose
  buttons are on; it does not switch on a level that is off.
- Everything else is free text: one **case-sensitive** substring of the message, the same
  rule `/v1/search` applies. A token like `postgres-lan:5432` is free text.
- Matches of the free text are highlighted in the rows.

## Live tail

LIVE follows the newest record. It pauses when you scroll up, open a row, or move the
selection with the keyboard — never on its own because the layout shifted. While paused,
new records are held and counted in the banner; resuming shows them.

## Keyboard

The `?` overlay is generated from `src/keys.rs` and matches what is bound.

- `/` focus query, `:` command palette, `?` help.
- `Esc` closes the innermost thing: palette, help, the query box, then the detail pane.
- `j`/`↓`, `k`/`↑` move the selection; `Enter` toggles its detail.
- `g`/`Home` oldest loaded record; `G`/`End` latest and resume live. `Space` pause/resume.
- `t` toggle absolute / relative timestamps. `r` reconnect.
- `I`/`W`/`E`/`F`/`D`/`T` toggle level visibility.

No `Ctrl+` chords: the browser acts on `Ctrl+T` and `Ctrl+C` before the page sees them.

## Command palette (`:`)

Lists the verbs matching what has been typed; `Tab` completes, `Enter` runs.

- `:pause` / `:resume` / `:clear` / `:retry`
- `:save path=<file>` — downloads the loaded buffer as NDJSON
- `:search q=<str> from=<iso> to=<iso>` — the same server search as Enter in the query
  box (`q` becomes the box's text), optionally over a fixed window
- `:level <name> on|off`
- `:help` / `:quit`

## Dev

Build the UI and let `tracecure-server` serve it, so the page and the API share an origin:

```
cd ui/tracecure-ui
trunk build
cd ../..
cargo run -p tracecure-server -- serve --ui-dist ui/tracecure-ui/dist
```

That puts the UI at `http://127.0.0.1:7789/`. Or use `.\deploy.ps1 -Ui` to rebuild the
UI into the deployed IIS site without restarting the service.

Not `trunk serve`: it binds `127.0.0.1:8080` (`Trunk.toml`) and `Trunk.toml` declares no
`[[proxy]]`, while both clients derive their target from the page origin
(`src/query.rs`, `src/ws.rs`) — so `/v1/search` and `/v1/tail` resolve to port 8080 and
nothing answers. `--proxy-backend` does not rescue it either: the upgrade to `/v1/tail`
is accepted but no frames are forwarded, so the stream stays empty.

## Build

```
cd ui/tracecure-ui
trunk build --release
```

Output goes to `ui/tracecure-ui/dist/`.
