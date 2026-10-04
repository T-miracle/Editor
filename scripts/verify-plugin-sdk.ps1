# Verify the distributed host SDK from an independent project outside the repository.
param([string]$HostExe = "$PSScriptRoot/../target/debug/editor-app.exe")

function Invoke-Native([string]$Program, [string[]]$Arguments) {
    # Cargo writes progress and warnings to stderr, which Windows PowerShell 5.1
    # turns into a terminating error under $ErrorActionPreference = 'Stop'. The exit
    # code is the contract, so stderr is passed through instead of raised.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $Program @Arguments
    } finally {
        $ErrorActionPreference = $previous
    }
    if ($LASTEXITCODE -ne 0) {
        throw "$Program exited with code $LASTEXITCODE."
    }
}

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath "$PSScriptRoot/..").Path
$hostPath = (Resolve-Path -LiteralPath $HostExe).Path
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
$workRoot = Join-Path $tempRoot ("me-editor-sdk-" + [guid]::NewGuid().ToString('N'))
$projectPrefix = $projectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
if ($workRoot.StartsWith($projectPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The system temporary directory must be outside the repository for this verification.'
}

function Get-SdkHashes([string]$Directory) {
    # Hash every exported file so repair checks cover the whole contract, not just its manifest.
    $hashes = @{}
    foreach ($file in Get-ChildItem -LiteralPath $Directory -Recurse -File) {
        $relative = $file.FullName.Substring($Directory.Length + 1)
        $hashes[$relative] = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
    }
    return $hashes
}

$previousTarget = $env:CARGO_TARGET_DIR
New-Item -ItemType Directory -Path $workRoot | Out-Null
Push-Location $workRoot
try {
    $source = Join-Path $projectRoot 'plugins/capability-example'
    $guest = Join-Path $workRoot 'capability-example'
    New-Item -ItemType Directory -Path $guest | Out-Null
    # Copy source inputs only: neither a parent Cargo workspace nor repository config can provide the SDK.
    foreach ($name in @('src', 'Cargo.toml', 'Cargo.lock', 'manifest.json', 'README.md', 'welcome.txt', 'composed-ui.json')) {
        Copy-Item -LiteralPath (Join-Path $source $name) -Destination $guest -Recurse
    }
    # The fixture manifest carries non-ASCII titles, so the encoding must be explicit:
    # Windows PowerShell 5.1 otherwise decodes it as ANSI and the JSON parse fails.
    $manifest = Get-Content -LiteralPath (Join-Path $guest 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($manifest.protocol -ne 7 -or $manifest.component -ne 'capability-example.wasm') {
        throw 'The SDK fixture must declare the current protocol and its packaged component.'
    }

    $sdk = Join-Path $workRoot 'exported-sdk'
    Invoke-Native $hostPath @('--export-plugin-sdk', $sdk)
    foreach ($name in @('Cargo.toml', 'README.md', 'src/lib.rs', 'src/api.rs', 'src/api/guest.rs', 'wit/plugin.wit')) {
        if (-not (Test-Path -LiteralPath (Join-Path $sdk $name) -PathType Leaf)) {
            throw "The exported SDK is missing $name."
        }
    }
    $before = Get-SdkHashes $sdk
    # Repair uses the same public export entry point and touches only this disposable export.
    [IO.File]::WriteAllText((Join-Path $sdk 'src/api.rs'), 'damaged SDK source')
    Remove-Item -LiteralPath (Join-Path $sdk 'wit/plugin.wit')
    Invoke-Native $hostPath @('--export-plugin-sdk', $sdk)
    $after = Get-SdkHashes $sdk
    if ($before.Count -ne $after.Count) { throw 'SDK repair changed the exported file set.' }
    foreach ($name in $before.Keys) {
        if ($before[$name] -ne $after[$name]) { throw "SDK repair did not restore $name." }
    }

    # Both Cargo's current directory and all build outputs stay outside the host checkout.
    $env:CARGO_TARGET_DIR = Join-Path $workRoot 'target'
    Push-Location $guest
    try {
        Invoke-Native $hostPath @('--plugin-cargo', 'Cargo.toml', 'build', '--locked', '--target', 'wasm32-wasip2', '--release')
    } finally { Pop-Location }
    $component = Join-Path $env:CARGO_TARGET_DIR 'wasm32-wasip2/release/capability_example_guest.wasm'
    $bytes = [IO.File]::ReadAllBytes($component)
    # wasm32-wasip2 already emits a component; the Manager test performs full instantiation afterward.
    if ($bytes.Length -lt 8 -or [Convert]::ToBase64String($bytes, 0, 8) -ne 'AGFzbQ0AAQA=') {
        throw 'The public SDK build did not produce a WebAssembly component.'
    }

    $files = [ordered]@{
        'manifest.json' = Join-Path $guest 'manifest.json'
        'README.md' = Join-Path $guest 'README.md'
        'welcome.txt' = Join-Path $guest 'welcome.txt'
        'composed-ui.json' = Join-Path $guest 'composed-ui.json'
        'capability-example.wasm' = $component
    }
    $package = Join-Path $workRoot 'capability-example.zip'
    Add-Type -AssemblyName System.IO.Compression
    $stream = [IO.File]::Create($package)
    $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($entry in $files.GetEnumerator()) {
            $entryStream = $archive.CreateEntry($entry.Key).Open()
            try {
                $content = [IO.File]::ReadAllBytes($entry.Value)
                $entryStream.Write($content, 0, $content.Length)
            } finally { $entryStream.Dispose() }
        }
    } finally { $archive.Dispose(); $stream.Dispose() }
    # Existing public Manager regressions consume the identical artifact, never a second in-repo build.
    foreach ($relative in @('target/plugin-sdk-test', 'target/plugin-api-test')) {
        $destination = Join-Path $projectRoot $relative
        New-Item -ItemType Directory -Force -Path $destination | Out-Null
        $zipPath = Join-Path $destination 'capability-example.zip'
        Copy-Item -LiteralPath $package -Destination $zipPath -Force
        Write-Output $zipPath
    }
    Write-Output 'Verified public SDK export, repair and independent component build.'
} finally {
    $env:CARGO_TARGET_DIR = $previousTarget
    Pop-Location
    # Recheck the resolved deletion target before recursively removing this exact generated directory.
    $cleanup = (Resolve-Path -LiteralPath $workRoot).Path
    if (-not [String]::Equals([IO.Path]::GetDirectoryName($cleanup), $tempRoot, [StringComparison]::OrdinalIgnoreCase) -or
        -not [String]::Equals([IO.Path]::GetFileName($cleanup), [IO.Path]::GetFileName($workRoot), [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove a verification directory outside the expected temporary root.'
    }
    Remove-Item -LiteralPath $cleanup -Recurse -Force
}
