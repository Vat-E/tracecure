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
