# tracecure-ui

Leptos 0.8 (CSR) web UI for Tracecure — single unified log stream.

## Routes

Just `/`. Legacy `/tail` and `/search` URLs redirect to `/` on load, preserving `?q=` if present.

## Keyboard

The in-app `?` overlay lists more than is wired up — it still advertises `Ctrl+C`,
`Enter`, `r`, and `j`/`k`/`g`/`Home`, none of which the dispatcher handles. What is
actually bound:

- `/` focus query, `:` open command palette, `?` help, `Esc` close overlay / collapse detail.
- `G`/`End` jump to latest and resume live. Row selection is mouse-only — there is no keyboard row navigation.
- `Space` pause/resume.
- `I`/`W`/`E`/`F`/`D`/`T` toggle level visibility.
- `Ctrl+T` toggle absolute / relative timestamps.

## Command palette (`:`)

- `:pause` / `:resume` / `:clear` / `:retry`
- `:save path=<file>` — downloads current buffer as NDJSON
- `:search q=<str> from=<iso> to=<iso>`
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
