# deploy.ps1 -- rebuild + bounce Tracecure in place.
#
# Usage (from repo root):
#   .\deploy.ps1                 # full rebuild (server + UI + CLI), release mode
#   .\deploy.ps1 -Ui             # UI only (no service restart)
#   .\deploy.ps1 -Server         # server only
#   .\deploy.ps1 -Cli            # CLI/TUI only (tracecure.exe)
#   .\deploy.ps1 -Debug          # use debug binaries (faster compile,
#                                # bigger exe, worse runtime perf)
#
# Assumes:
#   * NSSM service is named "Tracecure".
#   * tracecure-server.exe path is switched to whichever target we built.
#   * IIS serves ui\tracecure-ui\dist directly (no copy step required).

param(
    [switch]$Ui,
    [switch]$Server,
    [switch]$Cli,
    [switch]$Debug
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $repo

$profile_ = if ($Debug) { 'dev' } else { 'release' }
$targetDir = if ($Debug) { 'debug' } else { 'release' }
$exePath = Join-Path $repo "target\$targetDir\tracecure-server.exe"
$full = -not ($Ui -or $Server -or $Cli)

Import-Module (Join-Path $repo 'Tracecure.Common.psm1') -Force -DisableNameChecking

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
    if (-not (Test-IsAdmin)) {
        Write-Host "==> ERROR: deploy.ps1 must be run from an elevated PowerShell. NSSM service control requires Administrator." -ForegroundColor Red
        exit 1
    }
    Ensure-NssmService -Name 'Tracecure' -ExePath $exePath -WorkDir $repo

    Write-Host "==> Stopping Tracecure service" -ForegroundColor Cyan
    nssm stop Tracecure | Out-Null

    Write-Host "==> Building tracecure-server ($profile_)" -ForegroundColor Cyan
    $buildArgs = @('build', '-p', 'tracecure-server')
    if (-not $Debug) { $buildArgs += '--release' }
    cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    Write-Host "==> Pointing NSSM at $exePath" -ForegroundColor Cyan
    nssm set Tracecure Application $exePath | Out-Null

    Write-Host "==> Starting Tracecure service" -ForegroundColor Cyan
    nssm start Tracecure | Out-Null

    # Wait up to 5s for health.
    $healthy = $false
    for ($i = 0; $i -lt 10; $i++) {
        Start-Sleep -Milliseconds 500
        try {
            $r = Invoke-WebRequest -Uri http://127.0.0.1:7789/health -UseBasicParsing -TimeoutSec 1
            if ($r.Content -eq 'ok') { $healthy = $true; break }
        } catch { }
    }
    if ($healthy) {
        Write-Host "==> Service healthy" -ForegroundColor Green
    } else {
        Write-Host "==> Service did NOT become healthy within 5s -- check service-stderr.log" -ForegroundColor Red
        exit 1
    }
}

if ($full -or $Ui) {
    Write-Host "==> Building tracecure-ui (release via trunk)" -ForegroundColor Cyan
    Push-Location ui\tracecure-ui
    try {
        trunk build --release
        if ($LASTEXITCODE -ne 0) { throw "trunk build failed" }
    } finally {
        Pop-Location
    }
    Write-Host "==> UI rebuilt. IIS serves dist\ directly; refresh your browser." -ForegroundColor Green
}

if ($full -or $Cli) {
    Write-Host "==> Building tracecure-cli ($profile_)" -ForegroundColor Cyan
    $cliArgs = @('build', '-p', 'tracecure-cli')
    if (-not $Debug) { $cliArgs += '--release' }
    cargo @cliArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo build (cli) failed" }
    $cliExe = Join-Path $repo "target\$targetDir\tracecure.exe"
    Write-Host "==> CLI ready at $cliExe" -ForegroundColor Green
}

Write-Host "==> Deploy complete." -ForegroundColor Green
