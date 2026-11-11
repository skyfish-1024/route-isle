param([ValidateSet('Enable','Restore')][string]$Action = 'Restore', [int]$Port = 17890)

$ErrorActionPreference = 'Stop'
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
$backup = Join-Path $PSScriptRoot '.system-proxy-backup.xml'
$names = 'ProxyEnable', 'ProxyServer', 'ProxyOverride', 'AutoConfigURL'
$settingsPath = Join-Path $PSScriptRoot 'proxy-rules.json'
$gitKey = $null
if (Test-Path -LiteralPath $settingsPath) {
    $settings = Get-Content -Encoding UTF8 -Raw -LiteralPath $settingsPath | ConvertFrom-Json
    if ($settings.GitProxyHost) {
        if ($settings.GitProxyHost -notmatch '^[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+$') { throw 'Invalid GitProxyHost.' }
        $gitKey = "http.https://$($settings.GitProxyHost).proxy"
    }
}
function Restore-EmptyGitProxy([string]$operation) {
    $arguments = 'config --global {0} {1} ""' -f $operation, $gitKey
    $gitProcess = Start-Process git.exe -ArgumentList $arguments -NoNewWindow -Wait -PassThru
    if ($gitProcess.ExitCode -ne 0) { throw 'Could not restore the empty Git proxy.' }
}
if (!('SplitProxy.WinInet' -as [type])) {
    Add-Type -Namespace SplitProxy -Name WinInet -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("wininet.dll")]
public static extern bool InternetSetOption(System.IntPtr handle, int option, System.IntPtr buffer, int length);
'@
}
if ($Action -eq 'Enable') {
    $registry = Get-Item -LiteralPath $key
    $target = "127.0.0.1:$Port"
    if (Test-Path -LiteralPath $backup) {
        $saved = Import-Clixml -LiteralPath $backup
        if ($saved.Target -ne $target -or $registry.GetValue('ProxyServer') -ne $target -or
            $registry.GetValue('ProxyEnable') -ne 1) {
            throw 'Run Restore-Proxy.cmd before taking over again.'
        }
    } else {
        $state = @{}
        foreach ($name in $names) {
            if ($registry.GetValueNames() -contains $name) {
                $state[$name] = @{ Value = $registry.GetValue($name); Kind = $registry.GetValueKind($name).ToString() }
            }
        }
        $saved = @{ Values = $state; Target = $target }
    }
    if ($saved.GitKey -and $saved.GitKey -ne $gitKey) { throw 'Restore the previous proxy before changing GitProxyHost.' }
    if ($gitKey -and !$saved.ContainsKey('GitProxy')) {
        $previous = @(git config --global --get-all $gitKey)
        if ($LASTEXITCODE -notin 0,1) { throw 'Could not read the global Git proxy.' }
        $saved.GitProxy = $previous
    }
    if ($gitKey) { $saved.GitKey = $gitKey }
    $saved | Export-Clixml -LiteralPath $backup
    Set-ItemProperty -LiteralPath $key -Name ProxyServer -Value $target
    Set-ItemProperty -LiteralPath $key -Name ProxyOverride -Value 'localhost;127.*;[::1]'
    Remove-ItemProperty -LiteralPath $key -Name AutoConfigURL -ErrorAction SilentlyContinue
    Set-ItemProperty -LiteralPath $key -Name ProxyEnable -Value 1
    if ($gitKey) {
        git config --global --replace-all $gitKey "http://$target"
        if ($LASTEXITCODE -ne 0) { throw 'Could not set the global Git proxy.' }
    }
} elseif (Test-Path -LiteralPath $backup) {
    $saved = Import-Clixml -LiteralPath $backup
    if ($saved.GitKey) { $gitKey = $saved.GitKey }
    $registry = Get-Item -LiteralPath $key
    if ($registry.GetValue('ProxyServer') -eq $saved.Target) {
        foreach ($name in $names) {
            if ($saved.Values.ContainsKey($name)) {
                $entry = $saved.Values[$name]
                New-ItemProperty -LiteralPath $key -Name $name -Value $entry.Value -PropertyType $entry.Kind -Force | Out-Null
            } else {
                if ($registry.GetValueNames() -contains $name) {
                    Remove-ItemProperty -LiteralPath $key -Name $name
                }
            }
        }
    }
    if ($saved.ContainsKey('GitProxy')) {
        if (!$gitKey) { throw 'Set the original GitProxyHost in proxy-rules.json before restoring Git.' }
        $current = @(git config --global --get-all $gitKey)
        if ($LASTEXITCODE -notin 0,1) { throw 'Could not read the global Git proxy.' }
        if ($current.Count -eq 1 -and $current[0] -eq "http://$($saved.Target)") {
            $previous = @($saved.GitProxy)
            if ($previous.Count -eq 1 -and $previous[0] -eq '') {
                # An empty native argument is dropped by Windows PowerShell.
                Restore-EmptyGitProxy '--replace-all'
            } else {
                git config --global --unset-all $gitKey
                if ($LASTEXITCODE -ne 0) { throw 'Could not remove the temporary Git proxy.' }
                foreach ($value in $previous) {
                    if ($value -eq '') {
                        Restore-EmptyGitProxy '--add'
                    } else {
                        git config --global --add $gitKey $value
                        if ($LASTEXITCODE -ne 0) { throw 'Could not restore the global Git proxy.' }
                    }
                }
            }
        }
    }
    Remove-Item -LiteralPath $backup
}
[void][SplitProxy.WinInet]::InternetSetOption([IntPtr]::Zero, 39, [IntPtr]::Zero, 0)
[void][SplitProxy.WinInet]::InternetSetOption([IntPtr]::Zero, 37, [IntPtr]::Zero, 0)
