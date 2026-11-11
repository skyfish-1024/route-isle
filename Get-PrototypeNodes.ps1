param(
    [Parameter(Mandatory)][uri]$AccountSite,
    [Parameter(Mandatory)][string]$OutputFile
)

$ErrorActionPreference = 'Stop'
if ($AccountSite.Scheme -ne 'https' -or $AccountSite.UserInfo) {
    throw 'Use the official HTTPS account site.'
}
$origin = $AccountSite.GetLeftPart([UriPartial]::Authority)
$output = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputFile)
if (Test-Path -LiteralPath $output) { throw 'Output file already exists.' }
$credential = Get-Credential -Message 'Enter your FlyingBird account email and password'
if (!$credential) { return }

try {
    $login = Invoke-RestMethod -Method Post -Uri "$origin/api/v1/passport/auth/login" `
        -Body @{ email = $credential.UserName; password = $credential.GetNetworkCredential().Password } `
        -MaximumRedirection 0 -TimeoutSec 15
} catch {
    throw 'Login request failed. Check the official account site and credentials.'
}
if (!$login.data.auth_data) { throw 'Login response did not contain auth_data.' }
try {
    $info = Invoke-RestMethod -Uri "$origin/api/v1/user/getSubscribe" `
        -Headers @{ Authorization = [string]$login.data.auth_data } `
        -MaximumRedirection 0 -TimeoutSec 15
} catch {
    throw 'Account API did not return subscription details.'
}
$subscription = $null
if (![uri]::TryCreate([string]$info.data.subscribe_url, [UriKind]::Absolute, [ref]$subscription) -or
    $subscription.Scheme -ne 'https' -or $subscription.UserInfo) {
    throw 'The account API did not provide a standard HTTPS subscription URL.'
}
Write-Host "Subscription host: $($subscription.Host)"
if ((Read-Host 'Enter YES if this is your official subscription host') -cne 'YES') { return }
try {
    $response = Invoke-WebRequest -UseBasicParsing -Uri $subscription `
        -UserAgent 'Clash.Meta' -MaximumRedirection 0 -TimeoutSec 15
} catch {
    throw 'Subscription download failed.'
}
if ($response.Content -isnot [string] -or $response.Content -cnotmatch '(?m)^proxies\s*:') {
    throw 'Subscription did not return Clash YAML with a proxies list.'
}
$stream = [IO.File]::Open($output, [IO.FileMode]::CreateNew)
try {
    $writer = [IO.StreamWriter]::new($stream)
    try { $writer.Write($response.Content) } finally { $writer.Dispose() }
} finally {
    $stream.Dispose()
}
Write-Host 'Node YAML saved locally. It may contain node credentials.'
