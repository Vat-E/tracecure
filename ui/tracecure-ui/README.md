# sift-ui

Leptos 0.8 (CSR) web UI for Sift — single unified log stream.

## Routes

Just `/`. Legacy `/tail` and `/search` URLs redirect to `/` on load, preserving `?q=` if present.

## Keyboard

See the in-app `?` overlay for the complete list. Highlights:

- `/` focus query, `:` open command palette, `?` help.
- `j`/`k`/`g`/`G` / arrows / Home / End for navigation.
- `Space` pause/resume. `G` resumes and jumps to latest.
- `I`/`W`/`E`/`F`/`D`/`T` toggle level visibility.
- `Ctrl+T` toggle absolute / relative timestamps.

## Command palette (`:`)

- `:pause` / `:resume` / `:clear` / `:retry`
- `:save path=<file>` — downloads current buffer as NDJSON
- `:search q=<str> from=<iso> to=<iso>`
- `:level <name> on|off`
- `:help` / `:quit`

## Dev

```
cd ui/sift-ui
trunk serve
```

Needs `sift-server` running on the same origin.

## Build

```
cd ui/sift-ui
trunk build --release
```

Output goes to `ui/sift-ui/dist/`.
