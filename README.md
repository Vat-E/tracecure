# Tracecure

Self-hosted Rust-native log platform.

- **User documentation** — [`docs/book/`](docs/book/) (mdBook). Install, operate,
  integrate. Build it with `cargo install --locked mdbook` then
  `mdbook serve docs/book --open`.
- **Design and milestones** — [PLAN.md](PLAN.md).
- **Decisions** — [`docs/decisions/`](docs/decisions/).

## Status

M1 — real storage. WAL + Parquet segment store with background compaction, time-partitioned on disk. Still outstanding in M1: time-based retention, the label index, and criterion benches. See [PLAN.md](PLAN.md) §7 for what M2 (query engine) adds, and [docs/book/src/integration.md](docs/book/src/integration.md) for how to send records from other apps.

## Setup (Windows service + IIS)

The standard local setup: Tracecure runs as an NSSM-managed Windows service that survives reboots, with IIS serving the UI and reverse-proxying `/v1/*` (plus the `/v1/tail` WebSocket) on a `tracecure.localhost` host header. Two PowerShell scripts at the repo root do the work:

```powershell
.\setup.ps1   # one-time per machine, elevated. Installs the NSSM
              # service, creates the IIS app pool + site bound to
              # tracecure.localhost:80, adds the hosts file entry.

.\deploy.ps1  # every code change. With no switches: builds server + UI +
              # CLI, bounces the service, health-checks. Needs elevation
              # (the service leg only).
```

`.\setup.ps1` in a **non-elevated** shell is read-only: it inspects prereqs and prints the exact install command for any that are missing. Re-run elevated to actually apply changes. Both scripts are idempotent — safe to re-run.

`deploy.ps1` takes switches that build only part of the tree:

| Switch | Builds | Elevation | Restarts service |
|---|---|---|---|
| *(none)* | server + UI + CLI | required | yes |
| `-Server` | `tracecure-server` | required | yes |
| `-Ui` | `trunk build --release` into `ui/tracecure-ui/dist/` | not needed | no |
| `-Cli` | `tracecure.exe` (CLI + TUI) | not needed | no |
| `-Debug` | uses the `debug` profile instead of `release` | — | — |

Only the server leg touches NSSM, so only that leg checks for Administrator and stops/starts the service (then polls `http://127.0.0.1:7789/health` for up to 5s). **`.\deploy.ps1 -Ui` is the cheap UI inner loop** — no elevation, no service bounce; IIS serves `dist\` directly, so a browser refresh picks up the new bundle.

Prereqs not auto-installed (run these manually, once):

| Tool | Install |
|---|---|
| Rust toolchain | `winget install Rustlang.Rustup` |
| wasm32 target | `rustup target add wasm32-unknown-unknown` |
| trunk | `cargo install --locked trunk` |
| NSSM | `winget install nssm` |
| IIS feature | `Enable-WindowsOptionalFeature -Online -FeatureName IIS-WebServerRole -All` (elevated) |
| WebAdministration module | Windows feature: IIS Management Console + Scripts and Tools |
| WebSocket protocol | `Enable-WindowsOptionalFeature -Online -FeatureName IIS-WebSockets` (elevated) |
| URL Rewrite module | <https://www.iis.net/downloads/microsoft/url-rewrite> |
| ARR module | <https://www.iis.net/downloads/microsoft/application-request-routing> |

Once `setup.ps1` and `deploy.ps1` have both completed, the UI is at <http://tracecure.localhost/> and the API lives at `/v1/*` on the same origin (single-origin = no CORS, WebSocket tail upgrades through ARR transparently).

Customize the host/port at install time with `-Port <int>` / `-HostName <string>` (default `tracecure.localhost:80`, host-header-bound so it coexists with other IIS sites on port 80). `-NoHostsFile` skips the hosts-file edit; `-Force` replaces an existing IIS site whose config has drifted.

Then use the CLI to send records, search, and tail:

```
cargo run -p tracecure-cli -- send --host localhost --app demo 'hello world' --server http://tracecure.localhost
cargo run -p tracecure-cli -- search hello --server http://tracecure.localhost
cargo run -p tracecure-cli -- tail --server http://tracecure.localhost
cargo run -p tracecure-cli -- tui --server http://tracecure.localhost   # interactive terminal UI with filter + scroll
```

(After `cargo build --release`, the same binary is at `target/release/tracecure.exe`.)

## Dev mode (`cargo run`, no IIS)

For hot iteration without the IIS layer — short-lived process, no `tracecure.localhost` domain, no auto-restart:

```
cargo run -p tracecure-server -- serve
```

Stores data under `./data/` by default; binds to `127.0.0.1:7789`. Override with `--data <path>` and `--bind <addr>`. CLI commands work the same way as above but with `--server http://127.0.0.1:7789` (or omit — that's the default).

To serve the UI from the same binary, first build it:

```
(cd ui/tracecure-ui && trunk build --release)
cargo run -p tracecure-server -- serve --ui-dist ./ui/tracecure-ui/dist
```

Then open <http://127.0.0.1:7789/> in a browser.

## Development

- `cargo build --workspace` — build server + CLI.
- `cargo test --workspace` — run the workspace tests. Note this does **not** cover `ui/tracecure-ui` or `bench/format-fit`: both are in `exclude` in the root `Cargo.toml`, so their tests only run if you cd into them and run `cargo test` there.
- `cd ui/tracecure-ui && trunk build --release` — build production UI bundle into `ui/tracecure-ui/dist/`.

### Lint gate

`.github/workflows/ci.yml` runs, with `RUSTFLAGS: -D warnings`:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

The root `Cargo.toml` also turns on `clippy::pedantic` across the workspace (warn-level, with `module_name_repetitions`, `missing_errors_doc` and `missing_panics_doc` allowed) and denies `unsafe_code`. Combined with `-D warnings`, a pedantic lint fails the build — so run `cargo fmt --all` and the clippy line above before you commit.

This repo has no remote, so CI never actually runs. **Local runs are the only gate.**

### UI dev loop

`trunk serve` is not a working dev loop here. Trunk binds `127.0.0.1:8080` and `ui/tracecure-ui/Trunk.toml` declares no `[[proxy]]`, while the UI derives its API base from the page origin with no override (`src/query.rs`, `src/ws.rs`) — so the page asks `127.0.0.1:8080` for `/v1/search` and `/v1/tail` and nothing answers. Adding `--proxy-backend http://127.0.0.1:7789` fixes the HTTP backfill but not the tail: trunk's proxy does not forward WebSocket frames, so `/v1/tail` stays dead.

Use one of these instead:

```
cargo run -p tracecure-server -- serve --ui-dist ui/tracecure-ui/dist   # after a trunk build
.\deploy.ps1 -Ui                                                        # rebuild the bundle IIS already serves
```

Both put the UI and the API on one origin, which is what the UI expects.

## License

MIT OR Apache-2.0.
