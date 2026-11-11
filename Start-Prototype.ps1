param(
    [string]$ProviderFile,
    [Parameter(Mandatory)][string]$InternalDns,
    [ValidatePattern('^[a-zA-Z0-9-]+(\.[a-zA-Z0-9-]+)+$')]
    [string[]]$InternalDomains = @('intranet.example.com'),
    [ValidatePattern('^[a-zA-Z0-9-]+(\.[a-zA-Z0-9-]+)+$')]
    [string[]]$DirectDomains = @(),
    [string]$DirectInterface,
    [string]$WireGuardInterface,
    [switch]$UseFlyingBirdFallback,
    [ValidateRange(1024,65535)][int]$Port = 17890,
    [string]$Core = (Join-Path $PSScriptRoot 'ApexCore.exe'),
    [string]$HealthCheckUrl,
    [string]$StopFile,
    [switch]$SetSystemProxy,
    [switch]$CheckOnly
)

$ErrorActionPreference = 'Stop'
if ($ProviderFile -and $UseFlyingBirdFallback) {
    throw 'Choose either a provider file or FlyingBird fallback.'
}
if ($Port -eq 7890) { throw 'Choose a port separate from FlyingBird.' }
$dnsIp = $null
if (![ipaddress]::TryParse($InternalDns, [ref]$dnsIp)) {
    $dnsUri = $null
    if (!$UseFlyingBirdFallback -or ![uri]::TryCreate($InternalDns, [UriKind]::Absolute, [ref]$dnsUri) -or
        $dnsUri.Scheme -ne 'https' -or $dnsUri.Fragment -or $dnsUri.UserInfo) {
        throw 'Use a DNS IP or an HTTPS DoH URL with -UseFlyingBirdFallback.'
    }
    $InternalDns = $dnsUri.AbsoluteUri + '#FlyingBird'
}

$corePath = (Get-Item -LiteralPath $Core).FullName
$runDir = Join-Path ([IO.Path]::GetTempPath()) ('speedup-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $runDir | Out-Null
try {
    $dnsPolicy = @{}
    $outbounds = @()
    $wireGuardTarget = 'DIRECT'
    if ($WireGuardInterface) {
        $wireGuardTarget = 'WireGuard'
        $outbounds += @{ name = 'WireGuard'; type = 'direct'; 'interface-name' = $WireGuardInterface }
    }
    if ($DirectDomains.Count) {
        if (!$DirectInterface) { throw 'DirectDomains requires DirectInterface.' }
        $outbounds += @{ name = 'PhysicalDirect'; type = 'direct'; 'interface-name' = $DirectInterface }
    }
    $rules = foreach ($domain in $InternalDomains) {
        $dnsPolicy["+.$domain"] = @($InternalDns.ToString())
        "DOMAIN-SUFFIX,$domain,$wireGuardTarget"
    }
    $rules = @($rules) + @(foreach ($domain in $DirectDomains) {
        if ($InternalDomains -contains $domain) { throw "Duplicate domain rule: $domain" }
        $dnsPolicy["+.$domain"] = @($InternalDns)
        "DOMAIN-SUFFIX,$domain,PhysicalDirect"
    })
    $rules = @($rules | Sort-Object { ($_ -split ',')[1].Length } -Descending)
    $config = [ordered]@{
        'mixed-port' = $Port
        'bind-address' = '127.0.0.1'
        'allow-lan' = $false
        mode = 'rule'
        tun = @{ enable = $false }
        dns = @{
            enable = $true
            'enhanced-mode' = 'redir-host'
            nameserver = @('system')
            'nameserver-policy' = $dnsPolicy
        }
        rules = @($rules) + 'MATCH,REJECT'
    }
    if ($ProviderFile) {
        Copy-Item -LiteralPath $ProviderFile -Destination (Join-Path $runDir 'nodes.yaml')
        $config['proxy-providers'] = @{ Local = @{ type = 'file'; path = './nodes.yaml' } }
        $config['proxy-groups'] = @(@{ name = 'Upstream'; type = 'select'; use = @('Local') })
        $config.rules = @($rules) + 'MATCH,Upstream'
    }
    if ($outbounds.Count) { $config['proxies'] = $outbounds }
    if ($UseFlyingBirdFallback) {
        $config['proxies'] = $outbounds + @(@{
            name = 'FlyingBird'
            type = 'http'
            server = '127.0.0.1'
            port = 7890
        })
        $config.rules = @($rules) + 'MATCH,FlyingBird'
    }
    $configPath = Join-Path $runDir 'config.json'
    $config | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $configPath -Encoding utf8
    & $corePath -d $runDir -f $configPath -t
    if ($LASTEXITCODE -ne 0) { throw "Core rejected configuration ($LASTEXITCODE)." }
    if (!$CheckOnly -and $SetSystemProxy) {
        if ([Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners().Port -contains $Port) {
            throw "Port $Port is already in use."
        }
        $systemProxy = Join-Path $PSScriptRoot 'System-Proxy.ps1'
        $coreArgs = '-d "{0}" -f "{1}"' -f $runDir, $configPath
        $process = Start-Process -FilePath $corePath -ArgumentList $coreArgs -NoNewWindow -PassThru
        try {
            $checkUrls = @('https://www.example.com/')
            if ($HealthCheckUrl) { $checkUrls += $HealthCheckUrl }
            foreach ($url in $checkUrls) {
                & curl.exe -q --silent --show-error --fail --output NUL --noproxy no-bypass.invalid --proxy "http://127.0.0.1:$Port" --retry 2 --retry-connrefused --max-time 10 $url
                if ($LASTEXITCODE -ne 0 -or $process.HasExited) {
                    throw "Proxy health check failed for $url. System proxy was not switched."
                }
            }
            if ($StopFile -and (Test-Path -LiteralPath $StopFile)) { return }
            & $systemProxy -Action Enable -Port $Port
            Write-Host "System proxy is now 127.0.0.1:$Port. Press Ctrl+C to stop and restore."
            while (!$process.WaitForExit(500)) { if ($StopFile -and (Test-Path -LiteralPath $StopFile)) { break } }
        } finally {
            try {
                & $systemProxy -Action Restore
            } finally {
                if (!$process.HasExited) { Stop-Process -Id $process.Id }
                $process.Dispose()
            }
        }
    } elseif (!$CheckOnly) {
        Write-Host "Test proxy: http://127.0.0.1:$Port . Press Ctrl+C to stop."
        & $corePath -d $runDir -f $configPath
        if ($LASTEXITCODE -ne 0) { throw "Core exited with code $LASTEXITCODE." }
    }
} finally {
    foreach ($name in 'nodes.yaml', 'config.json') {
        $path = Join-Path $runDir $name
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path }
    }
    if (!(Get-ChildItem -LiteralPath $runDir -Force)) { Remove-Item -LiteralPath $runDir }
}

