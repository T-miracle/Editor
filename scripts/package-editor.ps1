# Build a distributable editor and independent local plugin installation packages.
param([string]$Output = "$PSScriptRoot/../dist/editor")
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path "$PSScriptRoot/..").Path
Push-Location $projectRoot
try {
    cargo build -p editor-app --release
    if ($LASTEXITCODE -ne 0) { throw 'Editor build failed' }
    New-Item -ItemType Directory -Force $Output | Out-Null
    Copy-Item -LiteralPath "$projectRoot/target/release/editor-app.exe" -Destination $Output
    & "$PSScriptRoot/build-plugins.ps1" -HostExe (Join-Path $Output 'editor-app.exe') -Output (Join-Path $Output 'plugins') -SdkOutput (Join-Path $Output 'sdk')
} finally { Pop-Location }
