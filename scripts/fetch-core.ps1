param(
    [string]$Version = 'v1.19.29'
)

$ErrorActionPreference = 'Stop'

$rootDir = Split-Path -Parent $PSScriptRoot
$targetPath = Join-Path $rootDir 'ApexCore.exe'

if (Test-Path $targetPath) {
    Write-Host "已存在内核文件: $targetPath"
    $confirm = Read-Host '是否重新下载并覆盖? (y/N)'
    if ($confirm -notin @('y', 'Y')) {
        exit 0
    }
}

$assetName = "mihomo-windows-amd64-compatible-$Version.zip"
$downloadUrl = "https://github.com/MetaCubeX/mihomo/releases/download/$Version/$assetName"

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("routeisle-core-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

try {
    $zipPath = Join-Path $tmpDir $assetName
    Write-Host "正在下载固定兼容内核 $assetName ..."
    Invoke-WebRequest -Uri $downloadUrl -OutFile $zipPath -Headers @{ 'User-Agent' = 'RouteIsle-Setup' }

    $extractDir = Join-Path $tmpDir 'extracted'
    Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force

    $exe = Get-ChildItem -Path $extractDir -Filter '*.exe' -Recurse | Select-Object -First 1
    if (-not $exe) {
        throw '压缩包中未找到 .exe 可执行文件。'
    }

    Copy-Item -Path $exe.FullName -Destination $targetPath -Force
    Write-Host "内核已就绪: $targetPath ($Version)"
} finally {
    Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
