# Build independent configuration providers through the public SDK, without repository crate access.
param([string]$HostExe = "$PSScriptRoot/../target/debug/editor-app.exe")
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath "$PSScriptRoot/..").Path
$hostPath = (Resolve-Path -LiteralPath $HostExe).Path
$tempPrefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
$fixtureRoot = [IO.Path]::GetFullPath((Join-Path $tempPrefix ('editor-configuration-' + [guid]::NewGuid().ToString('N'))))
$previousTarget = $env:CARGO_TARGET_DIR
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
try {
    # Only source inputs are copied. SDK source is resolved by --plugin-cargo outside the workspace.
    foreach ($name in @('src', 'Cargo.toml', 'manifest.json', 'README.md')) {
        Copy-Item -LiteralPath (Join-Path $projectRoot "plugins/configuration-example/$name") -Destination $fixtureRoot -Recurse
    }
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target'
    $previousError = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $hostPath --plugin-cargo (Join-Path $fixtureRoot 'Cargo.toml') build --target wasm32-wasip2 --release }
    finally { $ErrorActionPreference = $previousError }
    if ($LASTEXITCODE -ne 0) { throw 'Independent configuration provider build failed.' }
    $output = Join-Path $projectRoot 'target/run-config-plugin-tree'
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    Add-Type -AssemblyName System.IO.Compression
    # Distinct package identities are installed together. Their selected templates render different layouts.
    foreach ($identity in @('configuration-alpha', 'configuration-beta')) {
        $manifest = Get-Content -LiteralPath (Join-Path $fixtureRoot 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
        $manifest.id = $identity
        $manifest.name = $identity
        $stream = [IO.File]::Create((Join-Path $output "$identity.zip"))
        $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($entry in @('manifest.json', 'README.md', 'configuration-example.wasm')) {
                $bytes = switch ($entry) {
                    'manifest.json' { [Text.Encoding]::UTF8.GetBytes(($manifest | ConvertTo-Json -Depth 80)) }
                    'README.md' { [IO.File]::ReadAllBytes((Join-Path $fixtureRoot 'README.md')) }
                    default { [IO.File]::ReadAllBytes((Join-Path $projectRoot 'target/wasm32-wasip2/release/configuration_example_guest.wasm')) }
                }
                $entryStream = $archive.CreateEntry($entry).Open()
                try { $entryStream.Write($bytes, 0, $bytes.Length) } finally { $entryStream.Dispose() }
            }
        } finally { $archive.Dispose(); $stream.Dispose() }
    }
    Write-Output 'Built configuration-alpha.zip and configuration-beta.zip through the public SDK.'
} finally {
    $env:CARGO_TARGET_DIR = $previousTarget
    # Resolve and check this generated path before recursive cleanup; no project or private plugin data is removed.
    if (!$fixtureRoot.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase) -or $fixtureRoot -eq $tempPrefix.TrimEnd('\')) {
        throw 'Fixture cleanup path is outside its temporary parent.'
    }
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
}
