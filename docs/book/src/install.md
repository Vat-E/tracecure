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

`serve` takes these flags:

| Flag | Default | Meaning |
|---|---|---|
| `--data` | `./data` | WAL + segment directory |
| `--bind` | `127.0.0.1:7789` | listen address |
| `--ui-dist` | *(none)* | serve a `trunk build` output at `/`; without it, `/` returns 404 |
| `--retention` | *(none)* | delete records older than this — see [Retention](#retention) |
| `--retention-stream` | *(none)* | a different age for one stream; repeatable |
| `--ingest-auth` | `off` | `required` makes `/v1/ingest` demand a token — see [Ingest tokens](./integration.md#ingest-tokens) |
| `--oidc-issuer` and friends | *(none)* | require signing in to read — see [Sign-in](#sign-in) |

`tracecure-server token create | list | revoke` manages those tokens.

`RUST_LOG` controls tracing verbosity. Nothing else is configurable at runtime —
the storage tunables are compile-time constants.

## Retention

By default nothing is ever deleted. To cap how long records are kept:

```
tracecure-server serve --retention 30d
```

A duration is a whole number and a unit: `s`, `m`, `h`, `d` or `w`.

To give one stream a different age — shorter for a noisy one, longer for one
you need to keep — add `--retention-stream`, once per stream. It overrides
`--retention` for that `stream_id`:

```
tracecure-server serve \
  --retention 30d \
  --retention-stream docker/chatty-service=3d \
  --retention-stream audit=52w
```

With only `--retention-stream` and no `--retention`, the named streams expire
and every other stream is kept forever.

Both can come from the environment instead, which is the convenient form for a
container: `TRACECURE_RETENTION=30d` and
`TRACECURE_RETENTION_STREAMS=docker/chatty-service=3d,audit=52w`. A flag wins
over its variable.

What to expect:

- **Deletion is final.** There is no trash and no undo. The server logs the
  rules in effect at startup; read that line the first time you set one.
- **It is not exact to the second.** Storage is split into one-hour
  partitions, and a stream's records leave a partition once the whole
  partition is older than the rule. Add the compactor's half-hour grace and
  five-minute tick, and a record can outlive its rule by up to about an hour
  and a half.
- **Shortening a rule takes effect within minutes. Lengthening one does not
  bring anything back.**
- A value that does not parse, or the same stream given twice, stops the
  server at startup rather than being guessed at.
- There is no size-based limit. Retention bounds age, not disk use.

The design is recorded in `docs/decisions/ADR-003-m1-retention.md` in the
repository.

> **`trunk serve` cannot show live logs.** Its WebSocket proxy accepts the
> `/v1/tail` upgrade and then forwards no frames, so the UI backfills history
> and then sits on "connecting…". Use `--ui-dist` as above, or
> `.\deploy.ps1 -Ui`.

## Sign-in

By default anyone who can reach the server can read every log. To require
signing in, point the server at an OpenID Connect provider. Tracecure keeps no
accounts or passwords of its own.

At the provider, register a client for Tracecure:

- redirect URI: `<public URL>/auth/callback`
- post-logout redirect URI: `<public URL>/auth/signed-out`
- authorization code flow with PKCE; a public client (no secret) is enough
- scopes `openid profile email groups`, or as many of those as it has

Then start the server with:

```
tracecure-server serve \
  --oidc-issuer https://login.example.com \
  --oidc-client-id tracecure \
  --public-url https://logs.example.com \
  --oidc-allow-email you@example.com
```

| Flag | Variable | Meaning |
|---|---|---|
| `--oidc-issuer` | `TRACECURE_OIDC_ISSUER` | the provider's issuer URL; setting it turns sign-in on |
| `--oidc-client-id` | `TRACECURE_OIDC_CLIENT_ID` | this server's client id at the provider |
| — | `TRACECURE_OIDC_CLIENT_SECRET` | the client secret, if the client has one; environment only |
| `--public-url` | `TRACECURE_PUBLIC_URL` | where browsers reach this server |
| `--oidc-allow-email` | `TRACECURE_OIDC_ALLOW_EMAILS` | an address that may sign in; repeat, or comma-separate |
| `--oidc-allow-group` | `TRACECURE_OIDC_ALLOW_GROUPS` | a group whose members may sign in |
| `--oidc-allow-any-user` | `TRACECURE_OIDC_ALLOW_ANY=true` | anyone the provider authenticates may sign in |

What to expect:

- **You must say who is allowed.** Being able to log in at the provider is not
  enough, because a provider may let anyone register. One of the three allow
  options is required; with none the server refuses to start.
- **An allowed e-mail must be verified** by the provider (`email_verified`).
  An unverified address is refused even if it is on the list.
- **A mistake stops the server instead of leaving it open.** Half a
  configuration, a misspelled value, or a provider that cannot be reached at
  startup are all errors.
- **Ingest tokens become required** as soon as sign-in is on, unless you pass
  `--ingest-auth off`. Otherwise writing would be the one unguarded door.
- **A session lasts 30 days** and lives in the server's memory, so a restart
  signs everyone out. If the provider still has its own session, that costs a
  redirect, not a password.
- **Removing someone from the list or the group** takes effect when their
  session ends or the server restarts — not immediately.
- **Signing out goes through the provider.** Ending only Tracecure's session
  would be invisible: the next page load would redirect to the provider, which
  would sign the browser straight back in. So the sign-out button ends the
  session here and then sends the browser to the provider's own sign-out
  (its `end_session_endpoint`), which returns it to a "signed out" page. If
  the post-logout redirect URI is not registered, the provider keeps the
  browser on its own page instead. A provider without such an endpoint keeps
  its session, and the next sign-in will not ask for a password.
- **There are no roles.** Everyone who may sign in may read everything.
- The health probe (`/health`) stays open.

The design is recorded in `docs/decisions/ADR-005-sign-in-via-oidc.md` in the
repository.

## Smoke test

```
curl http://127.0.0.1:7789/health            # -> ok

cargo run -p tracecure-cli -- send --host localhost --app demo 'hello world'
cargo run -p tracecure-cli -- search hello
cargo run -p tracecure-cli -- tail
```

Add `--server http://tracecure.localhost` to any of these to target the IIS
deployment instead of the default `http://127.0.0.1:7789`.
