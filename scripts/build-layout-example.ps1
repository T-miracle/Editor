# Build independent layout/tool examples using only the public SDK exported by this host.
param(
    [string]$HostExe = "$PSScriptRoot/../target/debug/editor-app.exe",
    [ValidateSet('layout-example','tools-example')]
    [string[]]$Packages = @('layout-example','tools-example')
)
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path "$PSScriptRoot/..").Path
$taskHost = (Resolve-Path -LiteralPath $HostExe).Path
$taskPreviousTarget = $env:CARGO_TARGET_DIR
Push-Location $taskRoot
try {
    $env:CARGO_TARGET_DIR = Join-Path $taskRoot 'target'
    $taskOutput = Join-Path $taskRoot 'target/plugin-layout-test'
    New-Item -ItemType Directory -Force -Path $taskOutput | Out-Null
    foreach ($taskPackage in $Packages) {
        & $taskHost --plugin-cargo "plugins/$taskPackage/Cargo.toml" build --target wasm32-wasip2 --release
        if ($LASTEXITCODE -ne 0) { throw "$taskPackage build failed" }
        $taskFiles = @{
            'manifest.json'="plugins/$taskPackage/manifest.json"
            'README.md'="plugins/$taskPackage/README.md"
            "$taskPackage.wasm"="target/wasm32-wasip2/release/$($taskPackage.Replace('-','_'))_guest.wasm"
        }
        # Own icons are part of the archive; guests never depend on host-resident artwork.
        foreach ($taskIcon in Get-ChildItem -LiteralPath "$taskRoot/plugins/$taskPackage/icons" -File) {
            $taskFiles["icons/$($taskIcon.Name)"] = "plugins/$taskPackage/icons/$($taskIcon.Name)"
        }
        $taskArchivePath = Join-Path $taskOutput "$taskPackage.zip"
        $taskStream = [IO.File]::Create($taskArchivePath)
        $taskArchive = [IO.Compression.ZipArchive]::new($taskStream,[IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($taskEntry in $taskFiles.GetEnumerator()) {
                $taskEntryStream = $taskArchive.CreateEntry($taskEntry.Key).Open()
                try { $taskBytes = [IO.File]::ReadAllBytes((Join-Path $taskRoot $taskEntry.Value)); $taskEntryStream.Write($taskBytes,0,$taskBytes.Length) }
                finally { $taskEntryStream.Dispose() }
            }
        } finally { $taskArchive.Dispose(); $taskStream.Dispose() }
        Write-Output $taskArchivePath
    }
} finally { $env:CARGO_TARGET_DIR=$taskPreviousTarget; Pop-Location }
