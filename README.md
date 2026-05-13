# Tracecure

Self-hosted Rust-native log platform. See [PLAN.md](PLAN.md) for the design and milestones.

## Status

M0 — walking skeleton. See [docs/integration.md](docs/integration.md) for how to send records from other apps.

## Setup (Windows service + IIS)

The standard local setup: Tracecure runs as an NSSM-managed Windows service that survives reboots, with IIS serving the UI and reverse-proxying `/v1/*` (plus the `/v1/tail` WebSocket) on a `tracecure.localhost` host header. Two PowerShell scripts at the repo root do the work:

```powershell
.\setup.ps1   # one-time per machine, elevated. Installs the NSSM
              # service, creates the IIS app pool + site bound to
              # tracecure.localhost:80, adds the hosts file entry.

.\deploy.ps1  # every code change, elevated. Builds server + UI + CLI,
              # bounces the service, health-checks.
```

`.\setup.ps1` in a **non-elevated** shell is read-only: it inspects prereqs and prints the exact install command for any that are missing. Re-run elevated to actually apply changes. Both scripts are idempotent — safe to re-run.

Prereqs not auto-installed (run these manually, once):

| Tool | Install |
|---|---|
| Rust toolchain | `winget install Rustlang.Rustup` |
| wasm32 target | `rustup target add wasm32-unknown-unknown` |
| trunk | `cargo install --locked trunk` |
| NSSM | `winget install nssm` |
| IIS feature | `Enable-WindowsOptionalFeature -Online -FeatureName IIS-WebServerRole -All` (elevated) |
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
- `cargo test --workspace` — run all tests.
- `cd ui/tracecure-ui && trunk serve` — run the frontend in dev mode.
- `cd ui/tracecure-ui && trunk build --release` — build production UI bundle into `ui/tracecure-ui/dist/`.

## License

MIT OR Apache-2.0.
