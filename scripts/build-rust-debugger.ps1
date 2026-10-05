# Build one independent public-SDK component and its fixed native byte bridge.
param([string]$HostExe = '', [string]$Output = "$PSScriptRoot/../dist/plugins")
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath "$PSScriptRoot/..").Path
Push-Location $projectRoot
try {
    if (-not $HostExe) { throw 'Provide a built host with -HostExe; this script does not install a compiler or SDK' }
    $hostPath = (Resolve-Path -LiteralPath $HostExe).Path
    $buildRoot = Join-Path $projectRoot 'target/rust-debugger-package'
    New-Item -ItemType Directory -Force $buildRoot, $Output | Out-Null
    $bridgePath = Join-Path $buildRoot 'me-debug-bridge.exe'
    # The existing native compiler emits the bridge; its descendants remain in the host job.
    rustc --edition 2024 -O plugins/rust-debugger/native/bridge.rs -o $bridgePath
    if ($LASTEXITCODE -ne 0) { throw 'Native debug bridge build failed' }
    $previousTarget = $env:CARGO_TARGET_DIR
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target'
    try {
        & $hostPath --plugin-cargo plugins/rust-debugger/Cargo.toml build --target wasm32-wasip2 --release
        if ($LASTEXITCODE -ne 0) { throw 'Debug guest build failed' }
    } finally { $env:CARGO_TARGET_DIR = $previousTarget }
    $manifest = Get-Content -LiteralPath plugins/rust-debugger/manifest.json -Raw | ConvertFrom-Json
    # Only the distribution manifest changes; the source placeholder cannot bless arbitrary bytes.
    $manifest.services.adapter.installation.artifacts[1].sha256 = (Get-FileHash -LiteralPath $bridgePath -Algorithm SHA256).Hash.ToLowerInvariant()
    $manifestBytes = [Text.UTF8Encoding]::new($false).GetBytes(($manifest | ConvertTo-Json -Depth 100))
    Add-Type -AssemblyName System.IO.Compression
    $destination = [IO.Path]::GetFullPath((Join-Path $Output 'rust-debugger.zip'))
    $stream = [IO.File]::Create($destination)
    $archive = [IO.Compression.ZipArchive]::new($stream,[IO.Compression.ZipArchiveMode]::Create)
    try {
        $entryStream = $archive.CreateEntry('manifest.json').Open()
        try { $entryStream.Write($manifestBytes,0,$manifestBytes.Length) } finally { $entryStream.Dispose() }
        foreach ($item in @(@('README.md','plugins/rust-debugger/README.md'), @('rust-debugger.wasm','target/wasm32-wasip2/release/rust_debugger_guest.wasm'), @('native/me-debug-bridge.exe',$bridgePath))) {
            $entryStream = $archive.CreateEntry($item[0]).Open()
            try { $bytes = [IO.File]::ReadAllBytes($item[1]); $entryStream.Write($bytes,0,$bytes.Length) } finally { $entryStream.Dispose() }
        }
    } finally { $archive.Dispose(); $stream.Dispose() }
    Write-Output $destination
} finally { Pop-Location }
