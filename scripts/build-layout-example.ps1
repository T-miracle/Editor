# Build the independent layout fixture using only the public SDK exported by this host.
param([string]$HostExe = "$PSScriptRoot/../target/debug/editor-app.exe")
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path "$PSScriptRoot/..").Path
$taskHost = (Resolve-Path -LiteralPath $HostExe).Path
$taskPreviousTarget = $env:CARGO_TARGET_DIR
Push-Location $taskRoot
try {
    $env:CARGO_TARGET_DIR = Join-Path $taskRoot 'target'
    & $taskHost --plugin-cargo 'plugins/layout-example/Cargo.toml' build --target wasm32-wasip2 --release
    if ($LASTEXITCODE -ne 0) { throw 'Layout example build failed' }
    $taskOutput = Join-Path $taskRoot 'target/plugin-layout-test'
    New-Item -ItemType Directory -Force -Path $taskOutput | Out-Null
    $taskArchivePath = Join-Path $taskOutput 'layout-example.zip'
    $taskStream = [IO.File]::Create($taskArchivePath)
    $taskArchive = [IO.Compression.ZipArchive]::new($taskStream,[IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($taskEntry in @{ 'manifest.json'='plugins/layout-example/manifest.json'; 'README.md'='plugins/layout-example/README.md'; 'layout-example.wasm'='target/wasm32-wasip2/release/layout_example_guest.wasm' }.GetEnumerator()) {
            $taskEntryStream = $taskArchive.CreateEntry($taskEntry.Key).Open()
            try { $taskBytes = [IO.File]::ReadAllBytes((Join-Path $taskRoot $taskEntry.Value)); $taskEntryStream.Write($taskBytes,0,$taskBytes.Length) }
            finally { $taskEntryStream.Dispose() }
        }
    } finally { $taskArchive.Dispose(); $taskStream.Dispose() }
    Write-Output $taskArchivePath
} finally { $env:CARGO_TARGET_DIR=$taskPreviousTarget; Pop-Location }
