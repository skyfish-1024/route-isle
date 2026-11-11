param(
    [string[]]$WireGuardDomains = @('intranet.example.com'),
    [string[]]$DirectDomains = @('direct.example.com', 'ippure.com', 'ipv6.icanhazip.com', 'api.ipify.org'),
    [string]$WireGuardInterface,
    [string]$DirectInterface = 'WLAN',
    [switch]$SetSystemProxy,
    [string]$StopFile,
    [switch]$CheckOnly
)

$ErrorActionPreference = 'Stop'
$rulesFile = Join-Path $PSScriptRoot 'proxy-rules.json'
if (Test-Path -LiteralPath $rulesFile) {
    $savedRules = Get-Content -Encoding UTF8 -Raw -LiteralPath $rulesFile | ConvertFrom-Json
    if (!$PSBoundParameters.ContainsKey('WireGuardDomains')) { $WireGuardDomains = @($savedRules.SystemRoute) }
    if (!$PSBoundParameters.ContainsKey('DirectDomains')) { $DirectDomains = @($savedRules.PhysicalDirect) }
}
if (!$CheckOnly) {
    $curlArgs = @('-q', '--silent', '--show-error', '--fail', '--output', 'NUL',
        '--write-out', '%{http_code}', '--noproxy', 'no-bypass.invalid',
        '--proxy', 'http://127.0.0.1:7890', '--max-time', '10', 'https://www.example.com/')
    $status = & curl.exe @curlArgs
    if ($LASTEXITCODE -ne 0 -or $status -ne '200') {
        throw 'FlyingBird HTTPS preflight failed on 127.0.0.1:7890.'
    }
    Write-Host 'FlyingBird HTTPS preflight passed.'
}

$options = @{
    InternalDns = 'https://dns.google/dns-query'
    InternalDomains = $WireGuardDomains
    WireGuardInterface = $WireGuardInterface
    DirectDomains = $DirectDomains
    DirectInterface = $DirectInterface
    UseFlyingBirdFallback = $true
    CheckOnly = $CheckOnly
    SetSystemProxy = $SetSystemProxy
    StopFile = $StopFile
}
if ($savedRules.Core) { $options.Core = $savedRules.Core }
if ($savedRules.HealthCheckUrl) { $options.HealthCheckUrl = $savedRules.HealthCheckUrl }
& (Join-Path $PSScriptRoot 'Start-Prototype.ps1') @options
