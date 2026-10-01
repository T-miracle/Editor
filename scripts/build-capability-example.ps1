# Build the migration fixture through the public host SDK; never publish it into dist.
param([string]$HostExe = "$PSScriptRoot/../target/debug/editor-app.exe")
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path "$PSScriptRoot/..").Path
$hostPath = (Resolve-Path -LiteralPath $HostExe).Path
$previousTarget = $env:CARGO_TARGET_DIR
Push-Location $projectRoot
try {
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target'
    & $hostPath --plugin-cargo 'plugins/capability-example/Cargo.toml' build --target wasm32-wasip2 --release
    if ($LASTEXITCODE -ne 0) { throw 'Capability example build failed' }
    $output = Join-Path $projectRoot 'target/plugin-api-test'
    New-Item -ItemType Directory -Force -Path $output | Out-Null
    $destination = Join-Path $output 'capability-example.zip'
    # Only declared fixture resources enter the package, not local SDK caches or build outputs.
    $files = @{
        'manifest.json' = 'plugins/capability-example/manifest.json'
        'README.md' = 'plugins/capability-example/README.md'
        'welcome.txt' = 'plugins/capability-example/welcome.txt'
        'capability-example.wasm' = 'target/wasm32-wasip2/release/capability_example_guest.wasm'
    }
    $stream = [IO.File]::Create($destination)
    $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($entry in $files.GetEnumerator()) {
            $entryStream = $archive.CreateEntry($entry.Key).Open()
            try {
                $bytes = [IO.File]::ReadAllBytes((Join-Path $projectRoot $entry.Value))
                $entryStream.Write($bytes, 0, $bytes.Length)
            } finally { $entryStream.Dispose() }
        }
    } finally { $archive.Dispose(); $stream.Dispose() }
    Write-Output $destination
} finally {
    $env:CARGO_TARGET_DIR = $previousTarget
    Pop-Location
}
