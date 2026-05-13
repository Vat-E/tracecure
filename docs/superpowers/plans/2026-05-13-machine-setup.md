# Machine Setup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a checks-driven `setup.ps1` for per-machine bootstrap (prereq inspection, idempotent NSSM service + IIS site + URL Rewrite rule + hosts entry installation) plus a `Test-Prereqs` pre-flight in `deploy.ps1` so a missing prerequisite fails fast with actionable instructions instead of mid-build.

**Architecture:** A shared `Tracecure.Common.psm1` PowerShell module exports idempotent `Ensure-*` helpers and a `Test-Prereqs` inspector. `setup.ps1` is two-mode: non-elevated inspection + advice, elevated auto-config. `deploy.ps1` imports the module and calls `Test-Prereqs` first; its existing NSSM-install helper (now in the module) does the runtime service plumbing.

**Tech Stack:** PowerShell 5.1+ (Windows-builtin), `WebAdministration` cmdlets for IIS, `nssm` CLI, Windows `hosts` file at `%SystemRoot%\System32\drivers\etc\hosts`. No new external deps; no Pester.

**Spec reference:** [docs/superpowers/specs/2026-05-13-machine-setup-design.md](../specs/2026-05-13-machine-setup-design.md). The plan implements that spec; section numbers below refer to it.

---

## Branch + starting state

This plan executes on a new branch `feat/machine-setup` off `main`. Before Task 1, the current `ui-polish-pass` working tree has two pending changes that belong with this work:

- `deploy.ps1` — modified with inline NSSM service auto-install (the immediate fix that exposed the rest of the setup-script gap).
- `docs/superpowers/specs/2026-05-13-machine-setup-design.md` — untracked; the design this plan implements.

Task 1 below handles the branch creation + commits these as the starting point. Subsequent tasks refactor and extend.

---

## Task 1: Branch + commit existing in-flight work

**Files:**
- Modify (commit): `deploy.ps1` (already dirty)
- Create (commit): `docs/superpowers/specs/2026-05-13-machine-setup-design.md` (already on disk, untracked)

- [ ] **Step 1: Create `feat/machine-setup` branch off `main`**

```powershell
git checkout main
git checkout -b feat/machine-setup
git status
```

Expected: `Switched to a new branch 'feat/machine-setup'`. `git status` should show:
```
 M deploy.ps1
?? docs/superpowers/specs/2026-05-13-machine-setup-design.md
```

(The dirty changes follow the branch checkout because they don't conflict with main.)

- [ ] **Step 2: Commit the deploy.ps1 NSSM patch**

```powershell
git add deploy.ps1
git commit -m "fix(deploy): NSSM service auto-install closes rename gap

deploy.ps1 assumed a one-time ``nssm install Tracecure`` had happened
out-of-band. After the 2026-04-24 rename Sift -> Tracecure, only the
stale ``Sift`` service existed (pointing at a deleted path); the
``Tracecure`` service was never installed, so every nssm call in the
script failed with ``Can't open service!``.

Add Test-IsAdmin + Ensure-NssmService helpers inline. Ensure-NssmService
detects a missing service and installs it with the correct exe,
working directory, args, log paths, and SERVICE_AUTO_START. Re-runs
are idempotent (the nssm set lines are a safe no-op when state is
already correct).

The stale ``Sift`` service should be removed manually once
(``nssm remove Sift confirm`` in an elevated shell).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

Expected: `[feat/machine-setup <sha>] fix(deploy): NSSM service auto-install closes rename gap`. 1 file changed, ~32 insertions.

- [ ] **Step 3: Commit the spec doc**

```powershell
git add docs/superpowers/specs/2026-05-13-machine-setup-design.md
git commit -m "docs(setup): design spec for setup.ps1 + deploy pre-flight

Checks-driven setup.ps1 inspects prereqs (cargo, rustup, wasm32, trunk,
nssm, IIS feature, URL Rewrite, ARR, WebSocket protocol), prints
actionable install commands for missing items, and (when elevated)
auto-installs the NSSM service, IIS app pool + site with custom
host-header binding (tracecure.localhost:80), ARR-based reverse proxy
for /v1/* + /v1/tail WebSocket, and a hosts file entry.

deploy.ps1 gains a fast Test-Prereqs pre-flight that fails in <300ms
with the same actionable instructions if anything's missing.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

Expected: `[feat/machine-setup <sha>] docs(setup): design spec for setup.ps1 + deploy pre-flight`. 1 file changed, ~360 insertions.

- [ ] **Step 4: Verify clean starting state**

```powershell
git status
git log --oneline -3
```

Expected:
```
On branch feat/machine-setup
nothing to commit, working tree clean

<sha> docs(setup): design spec for setup.ps1 + deploy pre-flight
<sha> fix(deploy): NSSM service auto-install closes rename gap
69e8e6fa chore(rename): finish Sift → Tracecure across CI + docs + code
```

---

## Task 2: Move `Test-IsAdmin` + `Ensure-NssmService` into `Tracecure.Common.psm1`

This is a pure refactor — no behavior change. We extract the two helpers that currently live inline in `deploy.ps1` into a shared module so `setup.ps1` (next tasks) can use them too. After this task, `deploy.ps1` should behave identically to before.

**Files:**
- Create: `Tracecure.Common.psm1`
- Modify: `deploy.ps1` (remove inline helpers; add `Import-Module`)

- [ ] **Step 1: Create the module file with both helpers**

Create `C:\Users\vz\Programming\tracecure\Tracecure.Common.psm1`:

```powershell
# Tracecure.Common.psm1
# Shared PowerShell helpers used by deploy.ps1 and setup.ps1.
# All Ensure-* functions are idempotent: safe to re-run; no-op when
# state already matches.

function Test-IsAdmin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $p  = [Security.Principal.WindowsPrincipal]::new($id)
    return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# Idempotent: installs the NSSM service if missing, then re-applies the
# full config every run so the service stays aligned with the script's
# notion of truth.
function Ensure-NssmService {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$ExePath,
        [Parameter(Mandatory)][string]$WorkDir
    )
    if (-not (Get-Service -Name $Name -ErrorAction SilentlyContinue)) {
        Write-Host "==> Installing NSSM service '$Name'" -ForegroundColor Cyan
        nssm install $Name $ExePath | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "nssm install $Name failed" }
    }
    nssm set $Name Application   $ExePath                                  | Out-Null
    nssm set $Name AppDirectory  $WorkDir                                  | Out-Null
    nssm set $Name AppParameters serve                                     | Out-Null
    nssm set $Name AppStdout     (Join-Path $WorkDir 'service-stdout.log') | Out-Null
    nssm set $Name AppStderr     (Join-Path $WorkDir 'service-stderr.log') | Out-Null
    nssm set $Name Start         SERVICE_AUTO_START                        | Out-Null
}

Export-ModuleMember -Function Test-IsAdmin, Ensure-NssmService
```

- [ ] **Step 2: Verify the module parses cleanly**

```powershell
$errs = $null; $toks = $null
[System.Management.Automation.Language.Parser]::ParseFile(
    'C:\Users\vz\Programming\tracecure\Tracecure.Common.psm1',
    [ref]$toks, [ref]$errs
) | Out-Null
if ($errs.Count -gt 0) { $errs } else { 'OK' }
```

Expected: `OK`.

- [ ] **Step 3: Modify `deploy.ps1` — remove inline helpers, add `Import-Module`**

Open `deploy.ps1`. Locate the existing helper definitions (between `$full = -not (...)` and `if ($full -or $Server) {`). The current state has both `Test-IsAdmin` and `Ensure-NssmService` defined inline. Replace those definitions with a single `Import-Module` line.

The replacement target text (current content, post-NSSM-patch):

```powershell
$full = -not ($Ui -or $Server -or $Cli)

function Test-IsAdmin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $p  = [Security.Principal.WindowsPrincipal]::new($id)
    return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# Idempotent: installs the NSSM service if missing, then re-applies the full
# config every run so the service stays aligned with this script's truth.
function Ensure-NssmService {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$ExePath,
        [Parameter(Mandatory)][string]$WorkDir
    )
    if (-not (Get-Service -Name $Name -ErrorAction SilentlyContinue)) {
        Write-Host "==> Installing NSSM service '$Name'" -ForegroundColor Cyan
        nssm install $Name $ExePath | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "nssm install $Name failed" }
    }
    nssm set $Name Application   $ExePath                                  | Out-Null
    nssm set $Name AppDirectory  $WorkDir                                  | Out-Null
    nssm set $Name AppParameters serve                                     | Out-Null
    nssm set $Name AppStdout     (Join-Path $WorkDir 'service-stdout.log') | Out-Null
    nssm set $Name AppStderr     (Join-Path $WorkDir 'service-stderr.log') | Out-Null
    nssm set $Name Start         SERVICE_AUTO_START                        | Out-Null
}

if ($full -or $Server) {
```

Replace with:

```powershell
$full = -not ($Ui -or $Server -or $Cli)

Import-Module (Join-Path $repo 'Tracecure.Common.psm1') -Force

if ($full -or $Server) {
```

- [ ] **Step 4: Verify `deploy.ps1` parses cleanly**

```powershell
$errs = $null; $toks = $null
[System.Management.Automation.Language.Parser]::ParseFile(
    'C:\Users\vz\Programming\tracecure\deploy.ps1',
    [ref]$toks, [ref]$errs
) | Out-Null
if ($errs.Count -gt 0) { $errs } else { 'OK' }
```

Expected: `OK`.

- [ ] **Step 5: Smoke test — `deploy.ps1 -Cli` (no service ops, just builds CLI)**

This exercises the import + the existing flow without needing elevation. CLI deploy doesn't touch the service, but it does exercise the module import path.

```powershell
.\deploy.ps1 -Cli
```

Expected output:
```
==> Building tracecure-cli (release)
    Finished `release` profile [optimized] target(s) in <N>s
==> CLI ready at C:\Users\vz\Programming\tracecure\target\release\tracecure.exe
==> Deploy complete.
```

If any error mentions "Test-IsAdmin not defined" or "Ensure-NssmService not defined", the `Import-Module` line didn't load or didn't export those names — go back to Step 1 and verify the `Export-ModuleMember` line.

- [ ] **Step 6: Commit**

```powershell
git add Tracecure.Common.psm1 deploy.ps1
git commit -m "refactor(setup): extract Test-IsAdmin + Ensure-NssmService into shared module

Move both helpers verbatim from deploy.ps1 to Tracecure.Common.psm1.
deploy.ps1 imports the module via Import-Module ... -Force; behavior
unchanged. Prepares for setup.ps1 to use the same helpers.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Add `Test-Prereqs` + `Write-PrereqStatus` to the module

The inspector function checks 10 prereqs across `build` / `server` / `ui` / `iis` scopes and returns a structured result. The renderer prints a status grid + actionable install commands for missing items. Together they're the read-only half of `setup.ps1` (used identically in non-elevated and elevated modes).

**Files:**
- Modify: `Tracecure.Common.psm1` (add 2 functions + extend export list)

- [ ] **Step 1: Add `Test-Prereqs` to the module**

Append to `Tracecure.Common.psm1` (before the `Export-ModuleMember` line):

```powershell
# Internal helper: check whether a named command resolves on PATH.
function Test-OnPath {
    param([Parameter(Mandatory)][string]$Name)
    [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

# Internal helper: check IIS-scope items. Each returns $true/$false/$null.
# $null means "couldn't check from this shell" (typically: needs elevation).
function Test-IisFeatureEnabled {
    try {
        $f = Get-WindowsOptionalFeature -Online -FeatureName IIS-WebServerRole -ErrorAction Stop
        return ($f.State -eq 'Enabled')
    } catch { return $null }
}
function Test-IisModuleInstalled {
    param([Parameter(Mandatory)][string]$ModuleName)
    try {
        Import-Module WebAdministration -ErrorAction Stop
        $m = Get-WebGlobalModule -ErrorAction Stop | Where-Object Name -eq $ModuleName
        return [bool]$m
    } catch { return $null }
}

# Returns a structured prereq inventory. Shape:
#   @{ AllOk = $bool; Items = @(@{Name; Scope; Present; InstallCmd; Note}, ...) }
function Test-Prereqs {
    $items = New-Object System.Collections.ArrayList

    function Add-Item([hashtable]$i) { [void]$items.Add($i) }

    Add-Item @{
        Name='cargo'; Scope='build'
        Present=(Test-OnPath cargo)
        InstallCmd='winget install Rustlang.Rustup'
    }
    Add-Item @{
        Name='rustup'; Scope='build'
        Present=(Test-OnPath rustup)
        InstallCmd='winget install Rustlang.Rustup'
    }
    $wasmOk = $null
    if (Test-OnPath rustup) {
        try {
            $installed = & rustup target list --installed 2>$null
            $wasmOk = [bool]($installed | Select-String -SimpleMatch 'wasm32-unknown-unknown' -Quiet)
        } catch { $wasmOk = $false }
    } else { $wasmOk = $false }
    Add-Item @{
        Name='wasm32 target'; Scope='ui'
        Present=$wasmOk
        InstallCmd='rustup target add wasm32-unknown-unknown'
    }
    Add-Item @{
        Name='trunk'; Scope='ui'
        Present=(Test-OnPath trunk)
        InstallCmd='cargo install --locked trunk'
    }
    Add-Item @{
        Name='nssm on PATH'; Scope='server'
        Present=(Test-OnPath nssm)
        InstallCmd='winget install nssm'
    }
    Add-Item @{
        Name='WebAdministration module'; Scope='iis'
        Present=([bool](Get-Module -ListAvailable WebAdministration))
        InstallCmd='Windows feature: IIS Management Console + Scripts and Tools'
    }
    Add-Item @{
        Name='IIS feature'; Scope='iis'
        Present=(Test-IisFeatureEnabled)
        Note='requires elevation to check'
        InstallCmd='Enable-WindowsOptionalFeature -Online -FeatureName IIS-WebServerRole -All'
    }
    Add-Item @{
        Name='URL Rewrite module'; Scope='iis'
        Present=(Test-IisModuleInstalled 'RewriteModule')
        Note='requires elevation to check'
        InstallCmd='https://www.iis.net/downloads/microsoft/url-rewrite'
    }
    Add-Item @{
        Name='ARR module'; Scope='iis'
        Present=(Test-IisModuleInstalled 'ApplicationRequestRouting')
        Note='requires elevation to check'
        InstallCmd='https://www.iis.net/downloads/microsoft/application-request-routing'
    }
    Add-Item @{
        Name='WebSocket protocol'; Scope='iis'
        Present=(Test-IisModuleInstalled 'WebSocketModule')
        Note='requires elevation to check'
        InstallCmd='Enable-WindowsOptionalFeature -Online -FeatureName IIS-WebSockets'
    }

    $allOk = -not ($items | Where-Object { $_.Present -eq $false })
    return @{ AllOk = $allOk; Items = $items }
}
```

- [ ] **Step 2: Add `Write-PrereqStatus` to the module**

Append after `Test-Prereqs`:

```powershell
# Renders the prereq inventory as a status grid + actionable steps for missing items.
function Write-PrereqStatus {
    param([Parameter(Mandatory)][hashtable]$Result)

    Write-Host ""
    Write-Host "Prerequisite check:" -ForegroundColor Cyan
    foreach ($i in $Result.Items) {
        $glyph = switch ($i.Present) {
            $true  { '[+]' }
            $false { '[X]' }
            $null  { '[?]' }
        }
        $color = switch ($i.Present) {
            $true  { 'Green' }
            $false { 'Red' }
            $null  { 'DarkGray' }
        }
        $line = "  $glyph $($i.Name.PadRight(28)) ($($i.Scope))"
        if ($i.ContainsKey('Note') -and $i.Note) { $line += "  -- $($i.Note)" }
        Write-Host $line -ForegroundColor $color
    }
    Write-Host ""

    $missing = $Result.Items | Where-Object { $_.Present -eq $false }
    if ($missing) {
        Write-Host "Missing prerequisites:" -ForegroundColor Red
        foreach ($m in $missing) {
            Write-Host "  $($m.Name)" -ForegroundColor Red
            Write-Host "    Run: $($m.InstallCmd)" -ForegroundColor Yellow
        }
        Write-Host ""
    }
}
```

- [ ] **Step 3: Update the export list**

At the bottom of `Tracecure.Common.psm1`, change:
```powershell
Export-ModuleMember -Function Test-IsAdmin, Ensure-NssmService
```
to:
```powershell
Export-ModuleMember -Function `
    Test-IsAdmin, `
    Ensure-NssmService, `
    Test-Prereqs, `
    Write-PrereqStatus
```

Note: `Test-OnPath`, `Test-IisFeatureEnabled`, `Test-IisModuleInstalled` are intentionally NOT exported — they're internal helpers.

- [ ] **Step 4: Smoke test (non-elevated) — `Test-Prereqs` + `Write-PrereqStatus`**

```powershell
Import-Module .\Tracecure.Common.psm1 -Force
$r = Test-Prereqs
Write-PrereqStatus -Result $r
$r.AllOk
```

Expected on this dev machine (non-elevated):
```
Prerequisite check:
  [+] cargo                        (build)
  [+] rustup                       (build)
  [+] wasm32 target                (ui)
  [+] trunk                        (ui)
  [+] nssm on PATH                 (server)
  [+] WebAdministration module     (iis)
  [?] IIS feature                  (iis)  -- requires elevation to check
  [?] URL Rewrite module           (iis)  -- requires elevation to check
  [?] ARR module                   (iis)  -- requires elevation to check
  [?] WebSocket protocol           (iis)  -- requires elevation to check

True
```

`$r.AllOk` is `$true` because `$null` (unverifiable) is not the same as `$false` (verified missing). Only hard `$false` makes `AllOk` false.

- [ ] **Step 5: Smoke test — simulate a missing prereq**

Temporarily rename `nssm.exe` to simulate it being off PATH, run again, then restore:

```powershell
$nssm = (Get-Command nssm).Source
Rename-Item $nssm "${nssm}.disabled"
try {
    Import-Module .\Tracecure.Common.psm1 -Force
    $r = Test-Prereqs
    Write-PrereqStatus -Result $r
    "AllOk: $($r.AllOk)"
} finally {
    Rename-Item "${nssm}.disabled" $nssm
}
```

Expected:
```
Prerequisite check:
  [+] cargo                        (build)
  [+] rustup                       (build)
  [+] wasm32 target                (ui)
  [+] trunk                        (ui)
  [X] nssm on PATH                 (server)
  [+] WebAdministration module     (iis)
  [?] IIS feature                  (iis)  -- requires elevation to check
  ... (rest as before)

Missing prerequisites:
  nssm on PATH
    Run: winget install nssm

AllOk: False
```

Confirms missing-prereq rendering + the actionable install command.

- [ ] **Step 6: Commit**

```powershell
git add Tracecure.Common.psm1
git commit -m "feat(setup): add Test-Prereqs + Write-PrereqStatus

Test-Prereqs inspects 10 prereqs across build/server/ui/iis scopes and
returns @{ AllOk; Items = @{Name; Scope; Present; InstallCmd; Note} }.
Tri-state Present: \$true verified, \$false verified missing, \$null
can't tell (typically needs elevation). \$null is non-blocking.

Write-PrereqStatus renders the inventory as a status grid with
[+]/[X]/[?] glyphs and prints exact install commands for [X] items.

Internal helpers Test-OnPath, Test-IisFeatureEnabled,
Test-IisModuleInstalled are not exported.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Add `Ensure-IisAppPool`

Creates the dedicated `TracecurePool` if missing, then re-applies the canonical settings (No Managed Code, OnDemand). Idempotent.

**Files:**
- Modify: `Tracecure.Common.psm1`

- [ ] **Step 1: Add `Ensure-IisAppPool`**

Append to the module before the `Export-ModuleMember` line:

```powershell
# Idempotent: creates the app pool if missing, then re-applies the canonical
# property set so the pool stays aligned with this script's notion of truth.
function Ensure-IisAppPool {
    param([Parameter(Mandatory)][string]$Name)

    Import-Module WebAdministration -ErrorAction Stop

    if (-not (Get-WebAppPool -Name $Name -ErrorAction SilentlyContinue)) {
        Write-Host "==> Creating IIS app pool '$Name'" -ForegroundColor Cyan
        New-WebAppPool -Name $Name | Out-Null
    }
    # No Managed Code: static-file + reverse-proxy site has no .NET code path.
    Set-ItemProperty -Path "IIS:\AppPools\$Name" -Name managedRuntimeVersion -Value ''
    # Integrated pipeline is the default; explicit for clarity.
    Set-ItemProperty -Path "IIS:\AppPools\$Name" -Name managedPipelineMode -Value 'Integrated'
    # OnDemand: start the pool on first request. Lower idle resource use.
    Set-ItemProperty -Path "IIS:\AppPools\$Name" -Name startMode -Value 'OnDemand'
}
```

- [ ] **Step 2: Update the export list**

```powershell
Export-ModuleMember -Function `
    Test-IsAdmin, `
    Ensure-NssmService, `
    Test-Prereqs, `
    Write-PrereqStatus, `
    Ensure-IisAppPool
```

- [ ] **Step 3: Smoke test (elevated) — pool doesn't exist yet**

In an **elevated** PowerShell:

```powershell
cd C:\Users\vz\Programming\tracecure
Import-Module .\Tracecure.Common.psm1 -Force
Ensure-IisAppPool -Name 'TracecurePool'
Get-WebAppPool -Name 'TracecurePool' | Select-Object Name, State, managedRuntimeVersion, @{n='startMode';e={(Get-ItemProperty "IIS:\AppPools\TracecurePool" startMode).Value}}
```

Expected (first run):
```
==> Creating IIS app pool 'TracecurePool'

Name           State   managedRuntimeVersion startMode
----           -----   --------------------- ---------
TracecurePool  Started                       OnDemand
```

(`managedRuntimeVersion` is intentionally empty — that's "No Managed Code".)

- [ ] **Step 4: Smoke test — idempotent re-run**

```powershell
Ensure-IisAppPool -Name 'TracecurePool'
```

Expected: no `==> Creating ...` line. No errors. The function silently re-applies the property set; no observable change.

- [ ] **Step 5: Commit**

```powershell
git add Tracecure.Common.psm1
git commit -m "feat(setup): add Ensure-IisAppPool

Idempotent: creates a dedicated app pool with No Managed Code,
Integrated pipeline, OnDemand startMode. Re-applies the property set
every run so pool config stays aligned with the script.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Add `Ensure-IisSite`

The most complex helper. Creates the `Tracecure` IIS site (or updates a matching existing one), sets the `*:80:tracecure.localhost` host-header binding, applies the URL Rewrite reverse-proxy rule for `/v1/*`, and enables the ARR proxy global flag. Throws on conflict (different site already owns the name with mismatched config), unless `-Force` is passed.

**Files:**
- Modify: `Tracecure.Common.psm1`

- [ ] **Step 1: Add `Ensure-IisSite`**

Append to the module before the `Export-ModuleMember` line:

```powershell
# Idempotent: creates the IIS site with the given physical path, host-header
# binding, and ARR-based reverse-proxy rule. If a site of the same name exists
# with mismatched physical-path/port/host header, throws unless -Force.
function Ensure-IisSite {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$PhysicalPath,
        [Parameter(Mandatory)][string]$HostName,
        [Parameter(Mandatory)][int]   $Port,
        [Parameter(Mandatory)][string]$AppPool,
        [Parameter(Mandatory)][string]$BackendUrl,
        [switch]$Force
    )

    Import-Module WebAdministration -ErrorAction Stop

    if (-not (Test-Path $PhysicalPath)) {
        throw "Ensure-IisSite: physical path does not exist: $PhysicalPath"
    }

    $existing = Get-Website -Name $Name -ErrorAction SilentlyContinue
    if ($existing) {
        $currentPath = $existing.PhysicalPath
        $currentBinding = ($existing.Bindings.Collection | Select-Object -First 1).BindingInformation
        $expectedBinding = "*:${Port}:${HostName}"
        $mismatch = ($currentPath -ne $PhysicalPath) -or ($currentBinding -ne $expectedBinding)
        if ($mismatch -and -not $Force) {
            throw @"
Ensure-IisSite: site '$Name' exists with conflicting config.
  current:  path=$currentPath binding=$currentBinding
  expected: path=$PhysicalPath binding=$expectedBinding
Remove the existing site or pass -Force to overwrite.
"@
        }
        if ($mismatch) {
            Write-Host "==> Force-removing existing site '$Name' (conflicting config)" -ForegroundColor Cyan
            Remove-Website -Name $Name
            $existing = $null
        }
    }

    if (-not $existing) {
        Write-Host "==> Creating IIS site '$Name' bound to $HostName`:$Port" -ForegroundColor Cyan
        New-Website -Name $Name `
            -PhysicalPath   $PhysicalPath `
            -Port           $Port `
            -HostHeader     $HostName `
            -ApplicationPool $AppPool `
            -Force | Out-Null
    } else {
        # Idempotent: ensure app pool assignment is correct.
        Set-ItemProperty -Path "IIS:\Sites\$Name" -Name applicationPool -Value $AppPool
    }

    # Enable ARR proxy globally if not already.
    $arrEnabled = (Get-WebConfigurationProperty `
        -PSPath 'MACHINE/WEBROOT/APPHOST' `
        -Filter 'system.webServer/proxy' `
        -Name 'enabled').Value
    if (-not $arrEnabled) {
        Write-Host "==> Enabling ARR proxy globally" -ForegroundColor Cyan
        Set-WebConfigurationProperty `
            -PSPath 'MACHINE/WEBROOT/APPHOST' `
            -Filter 'system.webServer/proxy' `
            -Name 'enabled' `
            -Value 'True'
    }

    # Apply the URL Rewrite rule for /v1/* -> $BackendUrl/v1/*.
    # Clear any existing rule of the same name, then add fresh — idempotent.
    $sitePath = "IIS:\Sites\$Name"
    $ruleName = 'ReverseProxyToTracecureServer'
    $rulesFilter = '/system.webServer/rewrite/rules'
    $existingRule = Get-WebConfiguration -PSPath $sitePath -Filter "$rulesFilter/rule[@name='$ruleName']" -ErrorAction SilentlyContinue
    if ($existingRule) {
        Clear-WebConfiguration -PSPath $sitePath -Filter "$rulesFilter/rule[@name='$ruleName']"
    }
    Add-WebConfigurationProperty -PSPath $sitePath -Filter $rulesFilter -Name '.' -Value @{
        name = $ruleName
        stopProcessing = 'True'
    }
    Set-WebConfigurationProperty -PSPath $sitePath -Filter "$rulesFilter/rule[@name='$ruleName']/match" -Name 'url' -Value '^v1/(.*)'
    Set-WebConfigurationProperty -PSPath $sitePath -Filter "$rulesFilter/rule[@name='$ruleName']/action" -Name 'type' -Value 'Rewrite'
    Set-WebConfigurationProperty -PSPath $sitePath -Filter "$rulesFilter/rule[@name='$ruleName']/action" -Name 'url' -Value "$BackendUrl/v1/{R:1}"
}
```

- [ ] **Step 2: Update the export list**

```powershell
Export-ModuleMember -Function `
    Test-IsAdmin, `
    Ensure-NssmService, `
    Test-Prereqs, `
    Write-PrereqStatus, `
    Ensure-IisAppPool, `
    Ensure-IisSite
```

- [ ] **Step 3: Smoke test (elevated) — site doesn't exist yet**

In an **elevated** PowerShell:

```powershell
cd C:\Users\vz\Programming\tracecure
Import-Module .\Tracecure.Common.psm1 -Force
# Pool must exist first.
Ensure-IisAppPool -Name 'TracecurePool'
Ensure-IisSite `
    -Name 'Tracecure' `
    -PhysicalPath (Join-Path (Get-Location) 'ui\tracecure-ui\dist') `
    -HostName 'tracecure.localhost' `
    -Port 80 `
    -AppPool 'TracecurePool' `
    -BackendUrl 'http://127.0.0.1:7789'

Get-Website -Name 'Tracecure' | Select-Object Name, State, PhysicalPath, applicationPool
Get-WebBinding -Name 'Tracecure'
Get-WebConfigurationProperty -PSPath 'IIS:\Sites\Tracecure' -Filter "/system.webServer/rewrite/rules/rule[@name='ReverseProxyToTracecureServer']/action" -Name 'url'
(Get-WebConfigurationProperty -PSPath 'MACHINE/WEBROOT/APPHOST' -Filter 'system.webServer/proxy' -Name 'enabled').Value
```

Expected:
```
==> Creating IIS site 'Tracecure' bound to tracecure.localhost:80
==> Enabling ARR proxy globally        # (only if not already on)

Name      State   PhysicalPath                                              applicationPool
----      -----   ------------                                              ---------------
Tracecure Started C:\Users\vz\Programming\tracecure\ui\tracecure-ui\dist    TracecurePool

protocol bindingInformation
-------- ------------------
http     *:80:tracecure.localhost

http://127.0.0.1:7789/v1/{R:1}

True
```

- [ ] **Step 4: Smoke test — idempotent re-run**

```powershell
Ensure-IisSite `
    -Name 'Tracecure' `
    -PhysicalPath (Join-Path (Get-Location) 'ui\tracecure-ui\dist') `
    -HostName 'tracecure.localhost' `
    -Port 80 `
    -AppPool 'TracecurePool' `
    -BackendUrl 'http://127.0.0.1:7789'
```

Expected: no `==> Creating ...` line, no `==> Enabling ARR ...` line. No errors. Rule property re-set silently (clear + add). Re-query the rule URL to verify it's still `http://127.0.0.1:7789/v1/{R:1}`.

- [ ] **Step 5: Smoke test — conflict detection**

```powershell
# Should throw because the existing site has port 80 but we ask for 8088.
try {
    Ensure-IisSite `
        -Name 'Tracecure' `
        -PhysicalPath (Join-Path (Get-Location) 'ui\tracecure-ui\dist') `
        -HostName 'tracecure.localhost' `
        -Port 8088 `
        -AppPool 'TracecurePool' `
        -BackendUrl 'http://127.0.0.1:7789'
} catch {
    Write-Host "Caught expected throw:" -ForegroundColor Yellow
    $_.Exception.Message
}
```

Expected output includes:
```
Caught expected throw:
Ensure-IisSite: site 'Tracecure' exists with conflicting config.
  current:  path=... binding=*:80:tracecure.localhost
  expected: path=... binding=*:8088:tracecure.localhost
Remove the existing site or pass -Force to overwrite.
```

- [ ] **Step 6: Commit**

```powershell
git add Tracecure.Common.psm1
git commit -m "feat(setup): add Ensure-IisSite with ARR reverse proxy

Idempotent: creates IIS site with custom host-header binding
(\`*:Port:HostName\`), assigns app pool, enables ARR globally, and
applies a URL Rewrite rule for /v1/* -> \$BackendUrl/v1/{R:1} that
also carries the /v1/tail WebSocket upgrade.

Conflict-aware: if an existing site of the same name has mismatched
physical path or binding, throws unless -Force. With -Force the
existing site is removed and recreated.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Add `Ensure-HostsEntry`

Appends `127.0.0.1 tracecure.localhost` to the Windows hosts file if not already present. Idempotent. Single-line addition; preserves the rest of the file verbatim.

**Files:**
- Modify: `Tracecure.Common.psm1`

- [ ] **Step 1: Add `Ensure-HostsEntry`**

Append before the `Export-ModuleMember` line:

```powershell
# Idempotent: appends "<Ip>  <HostName>" to the Windows hosts file
# if no existing entry maps $HostName.
function Ensure-HostsEntry {
    param(
        [Parameter(Mandatory)][string]$HostName,
        [string]$Ip = '127.0.0.1'
    )

    $hostsPath = Join-Path $env:SystemRoot 'System32\drivers\etc\hosts'
    if (-not (Test-Path $hostsPath)) {
        throw "Hosts file not found at $hostsPath"
    }

    # Read current content; check for any non-comment line that maps $HostName.
    $lines = Get-Content $hostsPath
    $pattern = "^\s*[^#\s]\S*\s+.*\b$([regex]::Escape($HostName))\b"
    if ($lines | Where-Object { $_ -match $pattern }) {
        return  # already present, no-op
    }

    Write-Host "==> Adding hosts entry: $Ip  $HostName" -ForegroundColor Cyan
    $newLine = "$Ip`t$HostName"
    # Append with a leading newline only if the file doesn't end with one.
    $content = Get-Content $hostsPath -Raw
    if ($content -and -not $content.EndsWith("`n")) { $newLine = "`n$newLine" }
    Add-Content -Path $hostsPath -Value $newLine -Encoding ASCII
}
```

- [ ] **Step 2: Update the export list**

```powershell
Export-ModuleMember -Function `
    Test-IsAdmin, `
    Ensure-NssmService, `
    Test-Prereqs, `
    Write-PrereqStatus, `
    Ensure-IisAppPool, `
    Ensure-IisSite, `
    Ensure-HostsEntry
```

- [ ] **Step 3: Smoke test (elevated) — entry doesn't exist yet**

In an **elevated** PowerShell:

```powershell
cd C:\Users\vz\Programming\tracecure
Import-Module .\Tracecure.Common.psm1 -Force
# Confirm absence first.
Select-String -Path "$env:SystemRoot\System32\drivers\etc\hosts" -Pattern 'tracecure\.localhost' -Quiet
Ensure-HostsEntry -HostName 'tracecure.localhost'
Select-String -Path "$env:SystemRoot\System32\drivers\etc\hosts" -Pattern 'tracecure\.localhost'
```

Expected:
```
False
==> Adding hosts entry: 127.0.0.1  tracecure.localhost

hosts:NN:127.0.0.1	tracecure.localhost
```

(`NN` = whatever line number the new entry landed on.)

- [ ] **Step 4: Smoke test — idempotent re-run**

```powershell
Ensure-HostsEntry -HostName 'tracecure.localhost'
Get-Content "$env:SystemRoot\System32\drivers\etc\hosts" | Select-String 'tracecure\.localhost' | Measure-Object | Select-Object -ExpandProperty Count
```

Expected: no `==> Adding ...` line. The match count is exactly `1` — function did not duplicate the entry.

- [ ] **Step 5: Smoke test — DNS resolution**

```powershell
Resolve-DnsName tracecure.localhost
```

Expected: an A record at `127.0.0.1`.

- [ ] **Step 6: Commit**

```powershell
git add Tracecure.Common.psm1
git commit -m "feat(setup): add Ensure-HostsEntry

Idempotent: appends "<Ip>  <HostName>" to the Windows hosts file if
no non-comment line already maps the host. Default Ip = 127.0.0.1.

Why: modern Chromium/Firefox resolve *.localhost to 127.0.0.1
internally without DNS, but CLI tools (curl, the tracecure CLI,
Rust's reqwest) don't. A hosts entry keeps CLI access consistent with
browser access.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Build `setup.ps1`

The top-level entry point. Two-mode: non-elevated inspection + advice, elevated auto-config. Idempotent in both modes.

**Files:**
- Create: `setup.ps1`

- [ ] **Step 1: Write `setup.ps1`**

Create `C:\Users\vz\Programming\tracecure\setup.ps1`:

```powershell
# setup.ps1 -- one-time per-machine setup for Tracecure.
#
# Usage:
#   .\setup.ps1                           # inspect prereqs; auto-config if elevated
#   .\setup.ps1 -NoHostsFile              # skip the hosts file edit
#   .\setup.ps1 -Port 8088                # override default IIS port (80)
#   .\setup.ps1 -HostName 'tracecure.dev' # override default host header (tracecure.localhost)
#   .\setup.ps1 -Force                    # replace existing IIS site if config mismatches
#
# Idempotent: safe to re-run. Non-elevated mode is read-only.

param(
    [switch]$Force,
    [switch]$NoHostsFile,
    [int]   $Port     = 80,
    [string]$HostName = 'tracecure.localhost'
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $repo

Write-Host ""
Write-Host "================================" -ForegroundColor Cyan
Write-Host " Tracecure machine setup" -ForegroundColor Cyan
Write-Host "================================" -ForegroundColor Cyan

Import-Module (Join-Path $repo 'Tracecure.Common.psm1') -Force

$pre = Test-Prereqs
Write-PrereqStatus -Result $pre

$blockingMissing = $pre.Items | Where-Object { $_.Present -eq $false }
if ($blockingMissing) {
    Write-Host "Resolve the missing prerequisites above, then re-run .\setup.ps1." -ForegroundColor Red
    exit 1
}

if (-not (Test-IsAdmin)) {
    Write-Host "Prereqs verifiable from this shell look good." -ForegroundColor Green
    Write-Host "Re-run .\setup.ps1 from an elevated PowerShell to install the NSSM" -ForegroundColor Yellow
    Write-Host "service, configure IIS, and add the hosts entry." -ForegroundColor Yellow
    exit 0
}

# Elevated: re-check IIS items now that we can see them.
$pre2 = Test-Prereqs
$iisBlocking = $pre2.Items | Where-Object { $_.Scope -eq 'iis' -and $_.Present -eq $false }
if ($iisBlocking) {
    Write-Host "IIS-side prereqs visible from elevated shell:" -ForegroundColor Cyan
    Write-PrereqStatus -Result $pre2
    Write-Host "Resolve the missing IIS prerequisites above, then re-run .\setup.ps1." -ForegroundColor Red
    exit 1
}

Write-Host "Configuring services and IIS..." -ForegroundColor Cyan

Ensure-IisAppPool -Name 'TracecurePool'

Ensure-NssmService `
    -Name    'Tracecure' `
    -ExePath (Join-Path $repo 'target\release\tracecure-server.exe') `
    -WorkDir $repo

Ensure-IisSite `
    -Name           'Tracecure' `
    -PhysicalPath   (Join-Path $repo 'ui\tracecure-ui\dist') `
    -HostName       $HostName `
    -Port           $Port `
    -AppPool        'TracecurePool' `
    -BackendUrl     'http://127.0.0.1:7789' `
    -Force:$Force

if (-not $NoHostsFile) {
    Ensure-HostsEntry -HostName $HostName -Ip '127.0.0.1'
}

Write-Host ""
Write-Host "Machine ready. Run .\deploy.ps1 to build and start the server." -ForegroundColor Green
```

- [ ] **Step 2: Verify the script parses cleanly**

```powershell
$errs = $null; $toks = $null
[System.Management.Automation.Language.Parser]::ParseFile(
    'C:\Users\vz\Programming\tracecure\setup.ps1',
    [ref]$toks, [ref]$errs
) | Out-Null
if ($errs.Count -gt 0) { $errs } else { 'OK' }
```

Expected: `OK`.

- [ ] **Step 3: Smoke test (non-elevated) — inspection mode**

```powershell
.\setup.ps1
```

Expected:
```
================================
 Tracecure machine setup
================================

Prerequisite check:
  [+] cargo                        (build)
  [+] rustup                       (build)
  [+] wasm32 target                (ui)
  [+] trunk                        (ui)
  [+] nssm on PATH                 (server)
  [+] WebAdministration module     (iis)
  [?] IIS feature                  (iis)  -- requires elevation to check
  [?] URL Rewrite module           (iis)  -- requires elevation to check
  [?] ARR module                   (iis)  -- requires elevation to check
  [?] WebSocket protocol           (iis)  -- requires elevation to check

Prereqs verifiable from this shell look good.
Re-run .\setup.ps1 from an elevated PowerShell to install the NSSM
service, configure IIS, and add the hosts entry.
```

Exit code 0. No system changes.

- [ ] **Step 4: Smoke test (elevated) — auto-config**

Open an **elevated** PowerShell, then:

```powershell
cd C:\Users\vz\Programming\tracecure
.\setup.ps1
```

Expected (first run after Task 4-6 smoke tests already created pool + site + hosts entry):
```
================================
 Tracecure machine setup
================================

Prerequisite check:
  [+] cargo                        (build)
  ... (all green from elevated)
  [+] WebSocket protocol           (iis)

Configuring services and IIS...
==> Installing NSSM service 'Tracecure'    # only if not already done in earlier task
==> Service healthy ...                    # (no — setup.ps1 doesn't health-check; that's deploy.ps1)

Machine ready. Run .\deploy.ps1 to build and start the server.
```

If the NSSM service from Task 2's deploy.ps1 patch ran already, the `==> Installing` line is suppressed and `nssm set` lines are silent (idempotent).

- [ ] **Step 5: Smoke test — idempotent re-run**

```powershell
.\setup.ps1
```

Expected: status grid, no `==> Installing/Creating/Adding` lines, ends with "Machine ready." cleanly.

- [ ] **Step 6: Commit**

```powershell
git add setup.ps1
git commit -m "feat(setup): create setup.ps1 with two-mode flow

Top-level per-machine bootstrap script.

Non-elevated mode: inspects prereqs, prints status grid + actionable
install commands for missing items, exits with advice to re-run
elevated. Read-only.

Elevated mode: same inspection, then idempotently installs/configures
the NSSM service (Tracecure), IIS app pool (TracecurePool), IIS site
with custom host-header binding (tracecure.localhost:80) and ARR
reverse-proxy rule, and the hosts file entry.

Params: -Force (replace conflicting IIS site), -NoHostsFile (skip
hosts file edit), -Port / -HostName (override defaults).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Add `Test-Prereqs` pre-flight to `deploy.ps1`

Make `deploy.ps1` fail fast when a needed prereq is missing, with the same actionable error format `setup.ps1` uses. Scope-filtered: a `-Cli` deploy only blocks on `build` scope; full deploy blocks on everything verifiable.

**Files:**
- Modify: `deploy.ps1`

- [ ] **Step 1: Add the pre-flight block to `deploy.ps1`**

Open `deploy.ps1`. Locate the section (current state after Task 2):

```powershell
$full = -not ($Ui -or $Server -or $Cli)

Import-Module (Join-Path $repo 'Tracecure.Common.psm1') -Force

if ($full -or $Server) {
```

Replace with:

```powershell
$full = -not ($Ui -or $Server -or $Cli)

Import-Module (Join-Path $repo 'Tracecure.Common.psm1') -Force

# Pre-flight: fast-fail on missing prereqs scoped to what this deploy needs.
# Skip 'iis' scope here: it's runtime-only (UI build doesn't actually need
# IIS to be installed correctly), and IIS checks return $null in a non-elevated
# shell. setup.ps1 is the right place to enforce IIS prereqs.
$needScopes = @('build')
if ($full -or $Server) { $needScopes += 'server' }
if ($full -or $Ui)     { $needScopes += 'ui' }

$pre = Test-Prereqs
$blocking = $pre.Items | Where-Object { $_.Scope -in $needScopes -and $_.Present -eq $false }
if ($blocking) {
    Write-Host "==> ERROR: prerequisites missing. Run .\setup.ps1 for guided setup." -ForegroundColor Red
    foreach ($b in $blocking) {
        Write-Host "    [X] $($b.Name) -- run: $($b.InstallCmd)" -ForegroundColor Red
    }
    exit 1
}

if ($full -or $Server) {
```

- [ ] **Step 2: Verify `deploy.ps1` parses cleanly**

```powershell
$errs = $null; $toks = $null
[System.Management.Automation.Language.Parser]::ParseFile(
    'C:\Users\vz\Programming\tracecure\deploy.ps1',
    [ref]$toks, [ref]$errs
) | Out-Null
if ($errs.Count -gt 0) { $errs } else { 'OK' }
```

Expected: `OK`.

- [ ] **Step 3: Smoke test — `deploy.ps1 -Cli` (build-scope only, no server ops)**

```powershell
.\deploy.ps1 -Cli
```

Expected: same behavior as before — the pre-flight passes silently (no `==> ERROR` line) because all `build`-scope prereqs (`cargo`, `rustup`) are present. Then the existing CLI build runs.

- [ ] **Step 4: Smoke test — `deploy.ps1 -Ui` (build + ui scope)**

```powershell
.\deploy.ps1 -Ui
```

Expected: pre-flight passes (`cargo`, `rustup`, `wasm32 target`, `trunk` all present), then trunk builds the UI.

- [ ] **Step 5: Smoke test — simulated missing prereq fails fast**

Same trick as Task 3 — rename `nssm.exe` temporarily and run `-Server`:

```powershell
$nssm = (Get-Command nssm).Source
Rename-Item $nssm "${nssm}.disabled"
try {
    Measure-Command { .\deploy.ps1 -Server 2>&1 | Out-String -OutVariable out } | Select-Object TotalMilliseconds
    $out
} finally {
    Rename-Item "${nssm}.disabled" $nssm
}
```

Expected: total elapsed < 1000 ms. Output includes:
```
==> ERROR: prerequisites missing. Run .\setup.ps1 for guided setup.
    [X] nssm on PATH -- run: winget install nssm
```

No build started; no service operations attempted.

- [ ] **Step 6: Commit**

```powershell
git add deploy.ps1
git commit -m "feat(deploy): add Test-Prereqs pre-flight

Before any build or service operation, deploy.ps1 inspects the prereqs
needed for the current deploy mode (build = always; server adds nssm;
ui adds wasm32+trunk) and exits with an actionable error in <300ms
if anything's verifiably missing.

IIS-scope prereqs are intentionally excluded from this pre-flight: they
are runtime-only (UI build doesn't need IIS to succeed), and IIS
checks return \$null in a non-elevated shell. setup.ps1 is where IIS
prereqs are enforced.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: End-to-end smoke verification

Confirms the whole pipeline works from a Tracecure user's perspective. Not a code task — just a manual matrix run. Capture outputs in a scratch file if useful for debugging; nothing gets committed from this step.

**Files:** none modified

- [ ] **Step 1: Stop the Tracecure service**

In an **elevated** PowerShell:

```powershell
cd C:\Users\vz\Programming\tracecure
nssm stop Tracecure
```

Expected: `Tracecure: STOP: The operation completed successfully.` (or `Service is not running.` if it wasn't running)

- [ ] **Step 2: Run `deploy.ps1` (full)**

```powershell
.\deploy.ps1
```

Expected sequence:
```
==> Stopping Tracecure service
==> Building tracecure-server (release)
    Finished `release` profile [optimized] target(s) in <N>s
==> Pointing NSSM at C:\Users\vz\Programming\tracecure\target\release\tracecure-server.exe
==> Starting Tracecure service
==> Service healthy
==> Building tracecure-ui (release via trunk)
==> UI rebuilt. IIS serves dist\ directly; refresh your browser.
==> Building tracecure-cli (release)
==> CLI ready at C:\Users\vz\Programming\tracecure\target\release\tracecure.exe
==> Deploy complete.
```

- [ ] **Step 3: HTTP — UI loads via IIS reverse proxy**

```powershell
$ui = Invoke-WebRequest -Uri 'http://tracecure.localhost/' -UseBasicParsing
$ui.StatusCode
$ui.Content.Substring(0, [Math]::Min(200, $ui.Content.Length))
```

Expected: status `200`. Content starts with `<!DOCTYPE html>` (the trunk-built `index.html`).

- [ ] **Step 4: HTTP — `/v1/search` reaches the Rust server through the rewrite rule**

```powershell
$search = Invoke-WebRequest -Uri 'http://tracecure.localhost/v1/search?q=&limit=1' -UseBasicParsing
$search.StatusCode
$search.Content
```

Expected: status `200`. JSON content like `{"matches":[...],"count":N}` (might be empty matches but valid JSON).

- [ ] **Step 5: WebSocket — `/v1/tail` upgrades through ARR**

```powershell
# Minimal WebSocket probe using .NET ClientWebSocket.
Add-Type -AssemblyName System.Net.WebSockets.Client
$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$uri = [Uri]'ws://tracecure.localhost/v1/tail'
$cts = [Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(3))
$ws.ConnectAsync($uri, $cts.Token).Wait()
Write-Host "State: $($ws.State)"
$ws.CloseAsync([System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure, 'bye', $cts.Token).Wait()
```

Expected: `State: Open`. If the upgrade fails (state goes to `Aborted` or throws), check that:
- `WebSocket protocol` IIS module is installed and reported `[+]` by `setup.ps1`.
- The ARR proxy global flag is `True`: `(Get-WebConfigurationProperty -PSPath 'MACHINE/WEBROOT/APPHOST' -Filter 'system.webServer/proxy' -Name 'enabled').Value` returns `True`.

- [ ] **Step 6: Probe the CLI**

```powershell
.\target\release\tracecure.exe search "" --limit 1 --server http://tracecure.localhost
```

Expected: same JSON shape as Step 4 (CLI hits IIS, IIS proxies to server). Confirms CLI tools work with the `tracecure.localhost` hostname (i.e., the hosts file entry is doing its job).

- [ ] **Step 7: Final state check**

```powershell
.\setup.ps1
```

Expected: all `[+]` after the elevated re-run, no `==> Installing/Creating/Adding` lines, ends with "Machine ready." No system changes.

If any step fails: troubleshoot at the failing layer first (IIS module installed? rule applied? service running? port not in use by another site?). The spec's section 9 testing matrix is the reference behavior.

---

## Self-review notes

**Spec coverage:**
- §3 Files: Tasks 1 (commits), 2 (Common.psm1 + deploy.ps1 refactor), 8 (deploy.ps1 pre-flight), 7 (setup.ps1). Covered.
- §4.1 Module exports: Test-IsAdmin (Task 2), Ensure-NssmService (Task 2), Test-Prereqs (Task 3), Write-PrereqStatus (Task 3), Ensure-NssmService (Task 2), Ensure-IisAppPool (Task 4), Ensure-IisSite (Task 5), Ensure-HostsEntry (Task 6). All accounted for.
- §4.2 Test-Prereqs result shape: implemented in Task 3 Step 1 (10 items across build/server/ui/iis scopes; tri-state Present).
- §4.3 Detection methods: Test-OnPath (Get-Command), rustup target list, Get-Module ListAvailable, Get-WindowsOptionalFeature, Get-WebGlobalModule — all in Task 3.
- §5.1 setup.ps1 flow: Task 7.
- §5.2 setup.ps1 params: Task 7 Step 1 — -Force, -NoHostsFile, -Port, -HostName.
- §6 deploy.ps1 pre-flight: Task 8.
- §7.1 Site config (Tracecure / tracecure.localhost:80 / TracecurePool): Task 5 + Task 7.
- §7.2 App pool (TracecurePool, No Managed Code, OnDemand): Task 4.
- §7.3 URL Rewrite rule via applicationHost.config + ARR proxy flag: Task 5.
- §7.4 Hosts entry: Task 6.
- §8 Idempotence: every Ensure-* function has a check-then-mutate pattern; Tasks 4-6 explicitly smoke-test the idempotent re-run.
- §9 Testing matrix: Task 9 covers the main rows (configured machine, missing prereq, browser+CLI access).

**Placeholder scan:** no TBD/TODO/implement-later strings. Every code block is complete and runnable.

**Type consistency:** `Tracecure.Common.psm1` exports the same function names referenced in `setup.ps1` and `deploy.ps1`. Parameter names match across spec and tasks (e.g., `Ensure-IisSite -Name -PhysicalPath -HostName -Port -AppPool -BackendUrl -Force`).

**One follow-up worth flagging at execution time:** in Task 9 Step 2, the existing `deploy.ps1` post-build line `nssm set Tracecure Application $exePath` is now redundant with `Ensure-NssmService` re-applying everything every run. Not worth a cleanup task on its own — but if execution time allows a small simplification, the line at the bottom of the server block can come out, reducing one source of confusion (which entity owns the Application property). Leaving in scope as cosmetic.
