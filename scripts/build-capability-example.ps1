# Build the migration fixture through the public host SDK; never publish it into dist.
param([string]$HostExe = "$PSScriptRoot/../target/debug/editor-app.exe")
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path "$PSScriptRoot/..").Path
$hostPath = (Resolve-Path -LiteralPath $HostExe).Path
# A native program writes its progress to stderr, and with $ErrorActionPreference = 'Stop' that
# output becomes a terminating error before the exit code below can be read. Run it as a native
# command so the same failure is reported by that check instead of by PowerShell's own handling.
function Invoke-HostTool {
    param([Parameter(Mandatory)][string]$Exe, [Parameter(ValueFromRemainingArguments)][string[]]$Arguments)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $Exe @Arguments
    } finally {
        $ErrorActionPreference = $previous
    }
}
$previousTarget = $env:CARGO_TARGET_DIR
Push-Location $projectRoot
try {
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target'
    Invoke-HostTool -Exe $hostPath --plugin-cargo 'plugins/capability-example/Cargo.toml' build --target wasm32-wasip2 --release
    if ($LASTEXITCODE -ne 0) { throw 'Capability example build failed' }
    $output = Join-Path $projectRoot 'target/plugin-api-test'
    New-Item -ItemType Directory -Force -Path $output | Out-Null
    $destination = Join-Path $output 'capability-example.zip'
    # Only declared fixture resources enter the package, not local SDK caches or build outputs.
    $files = @{
        'manifest.json' = 'plugins/capability-example/manifest.json'
        'README.md' = 'plugins/capability-example/README.md'
        'welcome.txt' = 'plugins/capability-example/welcome.txt'
        'composed-ui.json' = 'plugins/capability-example/composed-ui.json'
        'capability-example.wasm' = 'target/wasm32-wasip2/release/capability_example_guest.wasm'
    }
    $stream = [IO.File]::Create($destination)
    # The compression types are not loaded into a fresh Windows PowerShell session, and reading a
    # type that is absent fails before anything is packed.
    Add-Type -AssemblyName System.IO.Compression -ErrorAction Stop
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
