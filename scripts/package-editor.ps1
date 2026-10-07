# Build the branded Nanobug executable and independent plugin installation packages.
param([string]$Output = "$PSScriptRoot/../dist/editor")
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path "$PSScriptRoot/..").Path
Push-Location $projectRoot
try {
    cargo build -p editor-app --release
    if ($LASTEXITCODE -ne 0) { throw 'Editor build failed' }
    New-Item -ItemType Directory -Force $Output | Out-Null
    # The Cargo package keeps its stable developer name; the distributed program uses the product name.
    $packagedHost = Join-Path $Output 'Nanobug.exe'
    Copy-Item -LiteralPath "$projectRoot/target/release/editor-app.exe" -Destination $packagedHost
    # A reused output directory must not retain an older executable under the previous product filename.
    $legacyHost = Join-Path $Output 'editor-app.exe'
    if (Test-Path -LiteralPath $legacyHost) {
        Remove-Item -LiteralPath $legacyHost -Force
    }
    & "$PSScriptRoot/build-plugins.ps1" -HostExe $packagedHost -Output (Join-Path $Output 'plugins')
} finally { Pop-Location }
