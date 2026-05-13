# Machine Setup — Prereq Checks, NSSM + IIS Auto-Config, Deploy Pre-Flight

**Status:** accepted design, awaiting implementation plan
**Date:** 2026-05-13
**Area:** Platform (PowerShell scripts at repo root)
**Milestone:** infrastructure — unblocks reliable deploy on second/third dev box and closes a long-standing "silent prerequisite" hole exposed by today's `deploy.ps1` failure.

## 1. Context

`.\deploy.ps1` today assumes a number of one-time machine-setup steps have already happened:

- A `Tracecure` NSSM service is installed and pointed at `target\release\tracecure-server.exe`.
- IIS is installed and a site exists pointing at `ui\tracecure-ui\dist`.
- The IIS site reverse-proxies `/v1/*` and the `/v1/tail` WebSocket to `http://127.0.0.1:7789`.
- Rust toolchain (`cargo`/`rustup`), the `wasm32-unknown-unknown` target, `trunk`, and `nssm` are all installed and on `PATH`.
- Several IIS modules — URL Rewrite, Application Request Routing (ARR), WebSocket protocol — are present.

None of these prerequisites are documented in the vault's `50-Environment/Sift Environment — Dev.md`, in the repo's `README.md`, or anywhere else. They were configured by hand once, the knowledge ages out, and a fresh-machine deploy fails in confusing ways.

Today's discovery: the project rename Sift → Tracecure on 2026-04-24 (6 commits, audited 2026-05-11) updated everything except the NSSM service, which is still installed as `Sift` pointing at the now-deleted `C:\Users\vz\Programming\sift\` path. The deploy script was updated to reference `Tracecure`, so it failed three times in succession with "service does not exist." This is the second-machine bootstrap problem, surfacing as a rename-cleanup bug.

A patch to `deploy.ps1` (commit pending) added idempotent NSSM service install — but only the NSSM half of the prereq stack. The IIS half, the toolchain half, and the "tell the user what's missing" UX are still gaps. This spec closes them.

## 2. Goal

A user on a clean Windows dev box can:

1. Clone the repo.
2. Install Rust, `trunk`, and `nssm` by following printed instructions (one `winget install` each — fast, well-known, kept manual because they need elevation anyway).
3. Run `.\setup.ps1` once (elevated) — it auto-installs the NSSM service, the IIS app pool, the IIS site with the right host header + proxy rule, and a hosts-file entry.
4. Run `.\deploy.ps1` and have it work.

If a step is missed, the next script run says exactly what's missing and how to fix it. No silent failures, no spelunking into NSSM error messages.

**Non-goals (deliberately out of scope):**

- Auto-install of Windows features (`IIS-WebServerRole`) or IIS modules (URL Rewrite, ARR) via MSI download. Both can require reboot or service restart and are well-known one-time installs — printed instructions are sufficient.
- HTTPS / TLS. The current Rust server is HTTP-only; IIS could terminate TLS in front of it but that's a separate enhancement.
- Multi-machine orchestration. Each machine runs its own `setup.ps1` locally.
- Production hardening (dedicated service account, hardened app pool identity, log rotation, etc.). Dev-host scope.
- Replacing `deploy.ps1` itself. It stays the inner-loop tool — `setup.ps1` is the per-machine bootstrap that runs much less often.
- Migrating data from the old `Sift` service location. The new `Tracecure` service starts fresh in `tracecure\data\`; the data dir already exists in M1 layout.
- A Pester (PowerShell unit-test) suite. Manual smoke testing matrix is sufficient for three short scripts.

## 3. Files

| File | Status | Purpose |
|---|---|---|
| `Tracecure.Common.psm1` | **new** at repo root | Shared module: helpers used by both scripts. Single source of truth for service/IIS config. |
| `setup.ps1` | **new** at repo root | One-time per-machine setup. Two-mode: non-elevated inspection, elevated auto-config. |
| `deploy.ps1` | **modified** | Adds `Test-Prereqs` pre-flight at top; existing `Test-IsAdmin` / `Ensure-NssmService` helpers move to the shared module (no behavior change for an already-configured machine). |

Three files at repo root. No `scripts/` subdir — the existing convention is flat (`deploy.ps1` lives at root), and three short scripts don't justify a subfolder.

## 4. Shared module — `Tracecure.Common.psm1`

PowerShell module imported by both `setup.ps1` and `deploy.ps1` via `Import-Module .\Tracecure.Common.psm1 -Force`.

### 4.1 Exported functions

```
Test-IsAdmin                                      -> [bool]
Test-Prereqs                                      -> [hashtable]   # see 4.2
Write-PrereqStatus  -Result <hashtable>           -> void          # prints status grid + actionable steps
Ensure-NssmService  -Name -ExePath -WorkDir       -> void          # idempotent
Ensure-IisAppPool   -Name                         -> void          # idempotent
Ensure-IisSite      -Name -PhysicalPath -HostName -Port -AppPool -BackendUrl
                                                  -> void          # idempotent, includes rewrite rule
Ensure-HostsEntry   -HostName -Ip ('127.0.0.1')   -> void          # idempotent
```

`Test-IsAdmin` and `Ensure-NssmService` move verbatim from the current `deploy.ps1` patch into this module. The rest are new.

### 4.2 `Test-Prereqs` return shape

```powershell
@{
  AllOk = $true   # or $false
  Items = @(
    @{ Name='cargo';             Scope='build';  Present=$true;   InstallCmd='winget install Rustlang.Rustup' }
    @{ Name='rustup';            Scope='build';  Present=$true;   InstallCmd='winget install Rustlang.Rustup' }
    @{ Name='wasm32 target';     Scope='ui';     Present=$true;   InstallCmd='rustup target add wasm32-unknown-unknown' }
    @{ Name='trunk';             Scope='ui';     Present=$true;   InstallCmd='cargo install --locked trunk' }
    @{ Name='nssm on PATH';      Scope='server'; Present=$true;   InstallCmd='winget install nssm' }
    @{ Name='WebAdministration'; Scope='iis';    Present=$true;   InstallCmd='Optional Windows feature: IIS Management Console + Scripts and Tools' }
    @{ Name='IIS feature';       Scope='iis';    Present=$null;   Note='requires elevation to check'; InstallCmd='Enable-WindowsOptionalFeature -Online -FeatureName IIS-WebServerRole -All' }
    @{ Name='URL Rewrite';       Scope='iis';    Present=$null;   Note='requires elevation';          InstallCmd='Download from https://www.iis.net/downloads/microsoft/url-rewrite' }
    @{ Name='ARR';               Scope='iis';    Present=$null;   Note='requires elevation';          InstallCmd='Download from https://www.iis.net/downloads/microsoft/application-request-routing' }
    @{ Name='WebSocket protocol'; Scope='iis';   Present=$null;   Note='requires elevation';          InstallCmd='Enable-WindowsOptionalFeature -Online -FeatureName IIS-WebSockets' }
  )
}
```

Three states for `Present`:
- `$true` — verified present.
- `$false` — verified missing. Blocking; `Write-PrereqStatus` renders an `[✗]` and prints the install command.
- `$null` — could not be checked from this shell (e.g., needs elevation to query IIS modules). Rendered as `[?]`. Not blocking on its own — `setup.ps1` non-elevated mode treats `$null` as "I can't tell, run me elevated to find out."

`Scope` lets `deploy.ps1` filter to only the prereqs needed for the current deploy mode (e.g., `-Cli` deploy only blocks on `build` scope).

### 4.3 Detection methods

| Item | Check |
|---|---|
| `cargo`, `rustup`, `trunk`, `nssm` | `Get-Command <name> -ErrorAction SilentlyContinue` |
| wasm32 target | `rustup target list --installed` contains `wasm32-unknown-unknown` |
| WebAdministration module | `Get-Module -ListAvailable WebAdministration` |
| IIS feature | `(Get-WindowsOptionalFeature -Online -FeatureName IIS-WebServerRole).State -eq 'Enabled'` (requires elevation; returns `$null` if not elevated) |
| URL Rewrite, ARR, WebSocket protocol | `Get-WebGlobalModule \| Where Name -in 'RewriteModule','ApplicationRequestRouting','WebSocketModule'` (requires elevation + WebAdministration loaded) |

All checks are fast (<300ms total). Failure modes are caught (each check wrapped in try/catch returning `$null` on access errors).

## 5. `setup.ps1` — behavior

Two-mode, idempotent, safe to re-run.

### 5.1 Flow

```
1. Print banner.
2. Import Tracecure.Common.psm1.
3. $result = Test-Prereqs; Write-PrereqStatus $result
     Renders a grid like:
       [✓] cargo               (build)
       [✓] rustup              (build)
       [✓] wasm32 target       (ui)
       [✓] trunk               (ui)
       [✓] nssm on PATH        (server)
       [✓] WebAdministration   (iis)
       [?] IIS feature         (iis)  — run elevated to check
       [?] URL Rewrite         (iis)  — run elevated to check
       [?] ARR                 (iis)  — run elevated to check
       [?] WebSocket protocol  (iis)  — run elevated to check
4. If any [✗]:
     for each: print "    Missing: <name>"
                     "    Run: <install-cmd>"
     exit 1
5. If non-admin:
     print "Prereqs look good. Re-run this script in an elevated PowerShell"
     print "to install the NSSM service, IIS site, app pool, and hosts entry."
     exit 0
6. If admin:
     Re-run Test-Prereqs with full IIS visibility; re-render.
     If any IIS-scoped [✗]: print install commands; exit 1.
     Else:
       Ensure-IisAppPool -Name 'TracecurePool'
       Ensure-NssmService -Name 'Tracecure' \
         -ExePath  (Join-Path $repo 'target\release\tracecure-server.exe') \
         -WorkDir  $repo
       Ensure-IisSite -Name 'Tracecure' \
         -PhysicalPath (Join-Path $repo 'ui\tracecure-ui\dist') \
         -HostName 'tracecure.localhost' -Port 80 \
         -AppPool 'TracecurePool' \
         -BackendUrl 'http://127.0.0.1:7789'
       Ensure-HostsEntry -HostName 'tracecure.localhost' -Ip '127.0.0.1'
     print "Machine ready. Run .\deploy.ps1."
     exit 0
```

### 5.2 Command-line parameters

```powershell
.\setup.ps1 [-Force] [-NoHostsFile] [-Port <int>] [-HostName <string>]
```

- `-Force`: re-apply all `Ensure-*` config even if items already exist (defensive — for "I think IIS got messed up, reset to known state").
- `-NoHostsFile`: skip the hosts-file edit. Default is to add the entry.
- `-Port <int>`: override the default IIS port (80).
- `-HostName <string>`: override the default IIS host header (`tracecure.localhost`).

The two override params exist so a user on a machine where port 80 is contested or `*.localhost` is hooked to something else has an escape hatch without editing the script.

## 6. `deploy.ps1` pre-flight

At line ~30 (right after `$full = -not (...)`), before any work:

```powershell
Import-Module (Join-Path $repo 'Tracecure.Common.psm1') -Force

$needScopes = @('build')
if ($full -or $Server) { $needScopes += 'server' }
if ($full -or $Ui)     { $needScopes += 'ui'; $needScopes += 'iis' }

$pre = Test-Prereqs
$blocking = $pre.Items |
    Where-Object { $_.Scope -in $needScopes -and $_.Present -eq $false }
if ($blocking) {
    Write-Host "==> ERROR: prerequisites missing. Run .\setup.ps1 for guided setup." -ForegroundColor Red
    $blocking | ForEach-Object {
        Write-Host "    [X] $($_.Name) -- run: $($_.InstallCmd)" -ForegroundColor Red
    }
    exit 1
}
```

The existing `Test-IsAdmin` check and `Ensure-NssmService` call inside `if ($full -or $Server)` stay — they now reference the shared-module versions instead of locally-defined ones (no behavior change, source-of-truth move only).

Note: pre-flight treats `$null` (couldn't check) as **non-blocking**. The user already saw their config working enough to invoke deploy; an unverifiable IIS module check shouldn't suddenly block. The pre-flight blocks only on hard `$false`.

## 7. IIS configuration specifics

### 7.1 Site `Tracecure`

| Property | Value |
|---|---|
| Name | `Tracecure` |
| Physical Path | `<repo>\ui\tracecure-ui\dist` |
| Bindings | `*:80:tracecure.localhost` (host-header binding — coexists with other sites on port 80) |
| App Pool | `TracecurePool` |
| Start automatically | yes |
| HTTPS | not configured (non-goal) |

Host-header binding is the key choice: multiple IIS sites can share port 80 cleanly because IIS routes requests by `Host:` header. No other dev sites on the machine break.

### 7.2 App Pool `TracecurePool`

| Property | Value |
|---|---|
| Name | `TracecurePool` |
| .NET CLR version | `No Managed Code` (static files + reverse proxy; no managed code path) |
| Pipeline mode | Integrated |
| Identity | `ApplicationPoolIdentity` (per-pool synthesized identity — isolated by default) |
| Start mode | OnDemand |

### 7.3 Reverse-proxy rule

Written into `applicationHost.config` via `Set-WebConfigurationProperty` (server-level config scoped to the `Tracecure` site). **Not** written into `web.config` inside `ui\tracecure-ui\dist` — the dist folder is a build artifact that `trunk build` regenerates, so a `web.config` there would be lost on every UI build.

Logical rule (PowerShell will produce the equivalent IIS XML):

```xml
<rule name="ReverseProxyToTracecureServer" stopProcessing="true">
  <match url="^v1/(.*)" />
  <action type="Rewrite" url="http://127.0.0.1:7789/v1/{R:1}" />
</rule>
```

WebSocket upgrade for `/v1/tail` works transparently through ARR when the WebSocket protocol module is enabled and the rule above matches — no separate rule is needed.

Additionally, ARR's "Enable proxy" server-level flag must be on. `Ensure-IisSite` flips it via `Set-WebConfigurationProperty -PSPath 'MACHINE/WEBROOT/APPHOST' -filter 'system.webServer/proxy' -name 'enabled' -value 'True'` if not already true.

### 7.4 Hosts-file entry

`%SystemRoot%\System32\drivers\etc\hosts` gets one line appended (idempotent — only if not already present):

```
127.0.0.1   tracecure.localhost
```

Why: modern Chromium/Firefox resolve `*.localhost` to 127.0.0.1 internally, so browser access to `http://tracecure.localhost/` works without this entry. But CLI tools (`curl`, the `tracecure` CLI itself, Rust's `reqwest`) generally don't, so a hosts entry keeps CLI access consistent with browser access. The entry is opt-out via `-NoHostsFile`.

## 8. Idempotence + error handling

- Every `Ensure-*` function: check current state first, no-op if already correct, otherwise mutate. Safe to re-run.
- `Ensure-IisSite`: if a site named `Tracecure` exists with different `PhysicalPath`/binding/AppPool, throw an explicit error rather than silently mutating ("Tracecure site exists with conflicting config: ... Remove it manually or pass -Force"). With `-Force`, replace.
- `Ensure-HostsEntry`: parse existing hosts file; only append the line if no entry maps `tracecure.localhost` already.
- Native commands (`cargo`, `nssm`, `rustup`, `trunk`): explicit `$LASTEXITCODE` check after each invocation; throw on non-zero.
- `$ErrorActionPreference = 'Stop'` everywhere — PowerShell cmdlet errors propagate immediately.
- Scripts exit non-zero on any failure with a final red `==> ERROR: ...` message including hints.

## 9. Testing (manual smoke matrix)

No Pester. Smoke tests:

| Scenario | Expected behavior |
|---|---|
| Non-elevated `setup.ps1` on a configured machine | Status grid prints all `[✓]` for non-IIS items; IIS items show `[?]`; exits 0 with "re-run elevated" message. No system changes. |
| Elevated `setup.ps1` on a configured machine | All checks pass; `Ensure-*` calls are no-ops (state already correct); exits 0 with "Machine ready." |
| Elevated `setup.ps1` on a fresh machine (NSSM/IIS missing) | Service installs; pool + site + rule + hosts entry created; deploy.ps1 works on next run. |
| Re-run elevated `setup.ps1` immediately after | Idempotent — no errors, no duplicate config, exits 0. |
| `deploy.ps1` with all prereqs present | Builds + deploys identically to today (pre-flight is a fast pass-through). |
| `deploy.ps1` with `nssm` removed from PATH | Pre-flight fails in <300ms with `[X] nssm on PATH -- run: winget install nssm`. No build attempted. |
| `deploy.ps1 -Cli` with `trunk` missing | Pre-flight passes (trunk not in `build` scope); CLI deploy proceeds. |
| `deploy.ps1 -Ui` with `trunk` missing | Pre-flight fails with `[X] trunk`. UI build not attempted. |
| Browse `http://tracecure.localhost/` after setup | UI loads. `/v1/search` returns Rust server JSON. WebSocket `/v1/tail` upgrades and streams. |
| `setup.ps1 -NoHostsFile` | Same as elevated default, but hosts file unchanged. Browser still works; CLI tools must use `127.0.0.1` explicitly. |

## 10. Followups (out of scope for this spec)

These came up during investigation but are separate work:

- **Vault: write ADR-003 — Project Rename Sift → Tracecure.** Flagged by 2026-05-11 hygiene audit. This spec completes the rename in its last unaccounted-for place (Windows service), so it's a natural ADR companion.
- **Vault: update `50-Environment/Sift Environment — Dev.md`.** Audit already flagged it stale on crate/binary names + M1 data path. Should also gain a "Service install" / "IIS site config" section pointing at `setup.ps1`.
- **Cleanup leftovers:** orphan `target\release\sift-server.exe` and `target\debug\sift-server.exe` from old builds; orphan `data\sift.ndjson` from M0. One-line `cargo clean` + manual delete.
- **Cleanup stale Sift NSSM service.** Manual one-time `nssm remove Sift confirm` in elevated shell. Setup.ps1 doesn't take this on automatically — too clobber-y for a script — but could print a warning if both `Sift` and `Tracecure` services exist.

## 11. References

- Current `deploy.ps1` (post-NSSM-install patch, today's commit pending).
- Vault: `30-Sessions/2026-05/2026-05-11 — Vault Hygiene Audit.md` — section 7, "Code-reality mismatches (rename drift)".
- Vault: `50-Environment/Sift Environment — Dev.md` — stale env doc.
- Repo: `docs/integration.md` — `/v1/*` HTTP + WS API surface.
- NSSM docs: <https://nssm.cc/usage>.
- IIS URL Rewrite + ARR proxy config: <https://learn.microsoft.com/en-us/iis/extensions/url-rewrite-module/reverse-proxy-with-url-rewrite-v2-and-application-request-routing>.
- RFC 6761 §6.3 + Chrome Status 4691634644451328 (`*.localhost` resolution).
