param([switch]$Preview)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
[Windows.Forms.Application]::EnableVisualStyles()
$rulesPath = Join-Path $PSScriptRoot 'proxy-rules.json'
$rules = @{
    SystemRoute = @('intranet.example.com')
    PhysicalDirect = @('direct.example.com', 'ippure.com', 'ipv6.icanhazip.com', 'api.ipify.org')
}
if (Test-Path -LiteralPath $rulesPath) {
    $rules = Get-Content -Encoding UTF8 -Raw -LiteralPath $rulesPath | ConvertFrom-Json
}
$form = New-Object Windows.Forms.Form
$form.Text = 'Split Proxy'
if ($Preview) { $form.Text += ' - Preview (simulation only)' }
$form.ClientSize = '620,395'
$form.StartPosition = 'CenterScreen'

function Add-Control($type, $caption, $x, $y, $width, $height) {
    $control = New-Object "Windows.Forms.$type"
    $control.Text = $caption
    $control.SetBounds($x, $y, $width, $height)
    $form.Controls.Add($control)
    $control
}
[void](Add-Control Label 'Direct via Windows routing (includes WireGuard routes)' 12 12 590 22)
$systemBox = Add-Control TextBox ($rules.SystemRoute -join "`r`n") 12 35 596 105
[void](Add-Control Label 'Direct via WLAN' 12 150 590 22)
$physicalBox = Add-Control TextBox ($rules.PhysicalDirect -join "`r`n") 12 173 596 105
foreach ($box in $systemBox, $physicalBox) {
    $box.Multiline = $true
    $box.ScrollBars = 'Vertical'
}
[void](Add-Control Label 'Paste domain suffixes separated by newlines, spaces or commas. Other domains use FlyingBird.' 12 284 596 24)
$save = Add-Control Button 'Save' 12 315 95 30
$start = Add-Control Button 'Save and Start' 115 315 125 30
$stop = Add-Control Button 'Stop' 248 315 95 30
$restore = Add-Control Button 'Restore proxy' 351 315 125 30
$status = Add-Control Label '' 12 360 596 25
$session = @{ Process = $null; StopFile = $null; PreviewRunning = $false }

function Save-Rules {
    $system = @($systemBox.Text -split '[\s,]+' | Where-Object { $_ } | Sort-Object -Unique)
    $physical = @($physicalBox.Text -split '[\s,]+' | Where-Object { $_ } | Sort-Object -Unique)
    foreach ($domain in $system + $physical) {
        if ($domain -notmatch '^[a-zA-Z0-9-]+(\.[a-zA-Z0-9-]+)+$') { throw "Invalid domain: $domain" }
    }
    foreach ($domain in $physical) {
        if ($system -contains $domain) { throw "Domain is in both lists: $domain" }
    }
    if (!$Preview) {
        $rules.SystemRoute = $system
        $rules.PhysicalDirect = $physical
        $rules | ConvertTo-Json |
            Set-Content -LiteralPath $rulesPath -Encoding UTF8
    }
}
function Request-Stop {
    Set-Content -LiteralPath $session.StopFile -Value 'stop' -Encoding ASCII
}
$save.Add_Click({
    try {
        Save-Rules
        $message = if ($Preview) { 'Preview: rules validated. Nothing was written.' }
            else { 'Saved. Restart the split proxy to apply changes.' }
        [void][Windows.Forms.MessageBox]::Show($message)
    }
    catch { [void][Windows.Forms.MessageBox]::Show($_.Exception.Message) }
})
$start.Add_Click({
    try {
        Save-Rules
        if ($Preview) { $session.PreviewRunning = $true; return }
        $session.StopFile = Join-Path $PSScriptRoot ('.proxy-stop-' + [guid]::NewGuid())
        $script = Join-Path $PSScriptRoot 'Start-SplitProxy.ps1'
        $command = '-NoProfile -File "{0}" -SetSystemProxy -StopFile "{1}"' -f $script, $session.StopFile
        $session.Process = Start-Process powershell.exe -ArgumentList $command -PassThru -WindowStyle Normal
        $start.Enabled = $false
        $stop.Enabled = $true
        $restore.Enabled = $false
    } catch { [void][Windows.Forms.MessageBox]::Show($_.Exception.Message) }
})
$stop.Add_Click({
    if ($Preview) { $session.PreviewRunning = $false; return }
    Request-Stop
    $stop.Enabled = $false
})
$restore.Add_Click({
    if ($Preview) { $session.PreviewRunning = $false; return }
    & (Join-Path $PSScriptRoot 'Restore-Proxy.cmd')
})
$timer = New-Object Windows.Forms.Timer
$timer.Interval = 750
$refresh = {
    if ($Preview) {
        $status.Text = if ($session.PreviewRunning) {
            'Simulation: FlyingBird ready | Split proxy running | System proxy applied'
        } else { 'Preview idle. No network settings are changed.' }
        $start.Enabled = !$session.PreviewRunning
        $stop.Enabled = $session.PreviewRunning
        $restore.Enabled = $session.PreviewRunning
        return
    }
    if ($session.Process -and $session.Process.HasExited) {
        $session.Process.Dispose()
        $session.Process = $null
        if (Test-Path -LiteralPath $session.StopFile) { Remove-Item -LiteralPath $session.StopFile }
    }
    $ports = [Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners().Port
    $registry = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
    $status.Text = "FlyingBird 7890: $($ports -contains 7890)   Split proxy 17890: $($ports -contains 17890)   System proxy: $($registry.ProxyEnable -eq 1 -and $registry.ProxyServer -eq '127.0.0.1:17890')"
    $start.Enabled = !$session.Process -and $ports -notcontains 17890
    $stop.Enabled = $null -ne $session.Process -and !(Test-Path -LiteralPath $session.StopFile)
    $restore.Enabled = !$session.Process -and $ports -notcontains 17890
}
$timer.Add_Tick($refresh)
$form.Add_FormClosing({
    param($sender, $eventArgs)
    if ($session.Process -and !$session.Process.HasExited) {
        Request-Stop
        $eventArgs.Cancel = $true
        [void][Windows.Forms.MessageBox]::Show('Stopping the split proxy and restoring settings. Close again after it stops.')
    }
})
& $refresh
$timer.Start()
try { [void]$form.ShowDialog() } finally { $timer.Dispose(); $form.Dispose() }
