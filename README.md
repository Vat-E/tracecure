# Tracecure

Self-hosted, Rust-native log platform. One binary that ingests, stores,
queries, and serves its own web UI. Logs are the only first-class citizen.

**Status: M1 — real storage.** Write-ahead log with group-commit fsync, an
in-memory memtable, time-partitioned Parquet segments with zstd, and a
background compactor. Retention by time and write-only ingest tokens are
there but off until you turn them on. Not yet: authentication for reading
(anyone who can reach the port can read every log), a query language, or a
full-text index. Treat it as an internal tool behind something that
authenticates.

## Run it

```
cargo run -p tracecure-server -- serve
```

Binds `127.0.0.1:7789`, stores under `./data/`. To serve the web UI from the
same binary:

```
(cd ui/tracecure-ui && trunk build --release)
cargo run -p tracecure-server -- serve --ui-dist ui/tracecure-ui/dist
```

Then send something through it:

```
cargo run -p tracecure-cli -- send --host localhost --app demo 'hello world'
cargo run -p tracecure-cli -- search hello
cargo run -p tracecure-cli -- tui          # interactive terminal UI
```

## Documentation

Everything user-facing lives in the mdBook under [`docs/book/`](docs/book/):

```
cargo install --locked mdbook
mdbook serve docs/book --open
```

| | |
|---|---|
| [Install and run](docs/book/src/install.md) | prerequisites; running from cargo; the Windows service + IIS deployment |
| [Docker and Cloudflare Tunnel](docs/book/src/deploy-docker.md) | the image, the compose file, publishing behind Cloudflare Access |
| [Operating the service](docs/book/src/operations.md) | NSSM/IIS specifics, logs, day-2 commands |
| [Integration API](docs/book/src/integration.md) | the HTTP + WebSocket contract |
| [CLI and TUI](docs/book/src/cli.md) | terminal clients |

Design and milestones are in [PLAN.md](PLAN.md); decisions are recorded as
ADRs in [`docs/decisions/`](docs/decisions/).

There is also a scripted Windows deployment (`setup.ps1`, `deploy.ps1`) that
runs the server as an NSSM service behind IIS. It is one deployment option,
not a requirement — see the install chapter.

## Development

```
cargo build --workspace
cargo test --workspace
```

`cargo test --workspace` does **not** cover `ui/tracecure-ui` or
`bench/format-fit` — both are in `exclude` in the root `Cargo.toml`. Test them
by cd-ing into the directory.

### Lint gate

The root `Cargo.toml` enables `clippy::pedantic` across the workspace and
denies `unsafe_code`. CI sets `RUSTFLAGS: -D warnings`, so a pedantic lint is
a build failure. Run this before committing:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

**This repo has no remote, so CI never runs. Local runs are the only gate.**

### UI dev loop

`trunk serve` does not work here: its proxy does not forward WebSocket
frames, so `/v1/tail` stays dead and the UI sits on "connecting…". Build the
bundle and serve it from the server instead, so the UI and API share one
origin:

```
(cd ui/tracecure-ui && trunk build --release)
cargo run -p tracecure-server -- serve --ui-dist ui/tracecure-ui/dist
```

## License

MIT OR Apache-2.0.
