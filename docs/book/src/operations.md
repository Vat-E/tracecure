# Operating the service

The supported local deployment is a Windows service (NSSM) behind IIS, which
reverse-proxies `/v1/*` to the server and serves the built WASM UI as static
files. `setup.ps1` installs it; `deploy.ps1` updates it.

## Where things live

| Thing | Path |
|---|---|
| Service name | `Tracecure` |
| Service stdout | `<repo>\service-stdout.log` |
| Service stderr | `<repo>\service-stderr.log` |
| Data (WAL + segments) | `<repo>\data` (server default `./data`, resolved against the service's working directory) |
| UI assets served by IIS | `<repo>\ui\tracecure-ui\dist` |
| Backend | `http://127.0.0.1:7789` |
| Site host header | `tracecure.localhost` (port 80 by default) |

Both log files are gitignored. They are the first place to look when the
service will not start.

## Deploy switches

`deploy.ps1` does different things depending on the switch, and only the full
run touches the service:

| Command | Builds | Restarts service | Needs elevation |
|---|---|---|---|
| `.\deploy.ps1` | server + UI + CLI | yes | yes |
| `.\deploy.ps1 -Ui` | UI only | no | no |
| `.\deploy.ps1 -Server` | server only | yes | yes |
| `.\deploy.ps1 -Cli` | CLI only | no | no |
| `.\deploy.ps1 -Debug` | uses debug binaries | — | — |

`-Ui` is the cheap inner loop: IIS serves `dist\` directly, so a UI rebuild
needs no restart and no admin shell.

> `cargo build -p tracecure-server` fails with **"Access is denied (os error 5)"**
> while the service is running — it holds the exe open. Stop the service first,
> or use `-Ui` / `-Cli` which do not rebuild it.

## Changing the server's arguments

The service always runs bare `serve`, so it picks up the defaults `--data ./data`
and `--bind 127.0.0.1:7789` resolved against its working directory.

**Do not use `nssm set Tracecure AppParameters ...` to change this.**
`Ensure-NssmService` in `Tracecure.Common.psm1` re-applies `Application`,
`AppDirectory`, `AppParameters serve`, `AppStdout`, `AppStderr` and `Start` on
**every** call, and it is called by both `setup.ps1` and `deploy.ps1`. Any
manual `nssm set` is silently reverted by the next deploy.

To change them durably, edit the `nssm set ... AppParameters` line in
`Tracecure.Common.psm1` and re-run the deploy.

Note also that the storage tunables (fsync interval, flush thresholds,
partition granularity, compaction tick) are **compile-time constants** — there
is no config file and no environment variable for them. See
`crates/tracecure-storage/README.md`.

## Day-2 commands

```powershell
nssm status  Tracecure
nssm stop    Tracecure
nssm start   Tracecure
nssm restart Tracecure
```

Health check — this exact string comparison is what `deploy.ps1` gates on, so
the body is part of the deploy contract:

```powershell
(Invoke-WebRequest http://127.0.0.1:7789/health).Content   # -> ok
```

Teardown:

```powershell
nssm stop Tracecure
nssm remove Tracecure confirm
Remove-Website -Name Tracecure          # requires the WebAdministration module
```

`setup.ps1 -Force` replaces an existing IIS site whose configuration has
drifted, rather than failing.

## Two sources of IIS rewrite rules

This trips people up when the proxy misbehaves. There are **two** places rules
come from, and IIS *merges* them rather than one overriding the other:

1. **`Tracecure.Common.psm1`** installs one site-level rule programmatically:
   `ReverseProxyToTracecureServer`, matching `^v1/(.*)` and rewriting to
   `http://127.0.0.1:7789/v1/{R:1}`. It is permanently active, not a bootstrap.
2. **`ui/tracecure-ui/web.config`** declares three more — `TracecureApi`,
   `TracecureHealth`, `SpaFallback` — plus the `.wasm` MIME mapping. It is
   copied into `dist\` on every `trunk build` (via a `copy-file` link in
   `index.html`).

`/v1/*` therefore works either way, but `/health`, the SPA fallback and the
`.wasm` MIME type exist **only** in `web.config`. Since `dist\` is gitignored
and regenerated on every build, edit `web.config` — not `dist\web.config`.

## Setup options

```powershell
.\setup.ps1                          # defaults: port 80, host tracecure.localhost
.\setup.ps1 -Port 8088               # different port
.\setup.ps1 -HostName tracecure.dev  # different host header
.\setup.ps1 -NoHostsFile             # skip the hosts-file entry
.\setup.ps1 -Force                   # replace a drifted IIS site
```
