# Install and run

Two ways to run Tracecure: as a Windows service behind IIS (the supported local
setup), or straight from cargo for development.

## Prerequisites

None of these are installed automatically. Run each once.

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

`rust-toolchain.toml` pins the toolchain to 1.94 and already declares the
`wasm32-unknown-unknown` target, so rustup installs both on the first cargo
invocation. The manual `rustup target add` above is belt-and-braces — but
`deploy.ps1` hard-fails if the target is missing, so it is worth running.

## Windows service + IIS

```powershell
.\setup.ps1    # once per machine, elevated
.\deploy.ps1   # after every code change
```

`setup.ps1` installs the NSSM service, creates the IIS app pool and a site
bound to `tracecure.localhost:80`, and adds the hosts-file entry. Run
non-elevated first if you want a dry run: it is read-only and prints the exact
install command for anything missing. Both scripts are idempotent.

When both have completed, the UI is at <http://tracecure.localhost/> and the API
is at `/v1/*` on the same origin — one origin means no CORS, and the
`/v1/tail` WebSocket upgrades through ARR transparently.

See [Operating the service](./operations.md) for switches, logs and day-2
commands.

## From cargo, without IIS

The server can serve the UI itself, which is the simplest way to run the whole
stack locally:

```
cd ui/tracecure-ui && trunk build --release && cd ../..
cargo run -p tracecure-server -- serve \
  --data ./data \
  --bind 127.0.0.1:7789 \
  --ui-dist ui/tracecure-ui/dist
```

`serve` takes exactly three flags:

| Flag | Default | Meaning |
|---|---|---|
| `--data` | `./data` | WAL + segment directory |
| `--bind` | `127.0.0.1:7789` | listen address |
| `--ui-dist` | *(none)* | serve a `trunk build` output at `/`; without it, `/` returns 404 |

`RUST_LOG` controls tracing verbosity. Nothing else is configurable at runtime —
the storage tunables are compile-time constants.

> **`trunk serve` cannot show live logs.** Its WebSocket proxy accepts the
> `/v1/tail` upgrade and then forwards no frames, so the UI backfills history
> and then sits on "connecting…". Use `--ui-dist` as above, or
> `.\deploy.ps1 -Ui`.

## Smoke test

```
curl http://127.0.0.1:7789/health            # -> ok

cargo run -p tracecure-cli -- send --host localhost --app demo 'hello world'
cargo run -p tracecure-cli -- search hello
cargo run -p tracecure-cli -- tail
```

Add `--server http://tracecure.localhost` to any of these to target the IIS
deployment instead of the default `http://127.0.0.1:7789`.
