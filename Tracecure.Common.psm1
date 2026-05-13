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

# Idempotent: creates the app pool if missing, then re-applies the canonical
# property set so the pool stays aligned with this script's notion of truth.
function Ensure-IisAppPool {
    param([Parameter(Mandatory)][string]$Name)

    Import-Module WebAdministration -ErrorAction Stop

    if (-not (Get-WebAppPool -Name $Name -ErrorAction SilentlyContinue)) {
        Write-Host "==> Creating IIS app pool '$Name'" -ForegroundColor Cyan
        New-WebAppPool -Name $Name | Out-Null
        if (-not (Get-WebAppPool -Name $Name -ErrorAction SilentlyContinue)) {
            throw "Ensure-IisAppPool: failed to create app pool '$Name'"
        }
    }
    $poolPath = "IIS:\AppPools\$Name"
    # No Managed Code: static-file + reverse-proxy site has no .NET code path.
    Set-ItemProperty -Path $poolPath -Name managedRuntimeVersion -Value ''
    # Integrated pipeline is the default; explicit for clarity.
    Set-ItemProperty -Path $poolPath -Name managedPipelineMode -Value 'Integrated'
    # OnDemand: start the pool on first request. Lower idle resource use.
    Set-ItemProperty -Path $poolPath -Name startMode -Value 'OnDemand'
}

Export-ModuleMember -Function `
    Test-IsAdmin, `
    Ensure-NssmService, `
    Test-Prereqs, `
    Write-PrereqStatus, `
    Ensure-IisAppPool
