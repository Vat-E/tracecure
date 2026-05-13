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

Import-Module (Join-Path $repo 'Tracecure.Common.psm1') -Force -DisableNameChecking

$pre = Test-Prereqs
Write-PrereqStatus -Result $pre

$blockingMissing = $pre.Items | Where-Object { $_.Present -eq $false }
if ($blockingMissing) {
    Write-Host "==> ERROR: prerequisites missing. Resolve the items above, then re-run .\setup.ps1." -ForegroundColor Red
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
    Write-Host "==> ERROR: IIS prerequisites missing. Resolve the items above, then re-run .\setup.ps1." -ForegroundColor Red
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
