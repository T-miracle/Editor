# Build independent component packages; copy these beside the packaged editor executable.
param(
    [string]$Output = "$PSScriptRoot/../dist/plugins",
    [string]$HostExe = ''
)
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path "$PSScriptRoot/..").Path
Push-Location $projectRoot
try {
    if (-not $HostExe) {
        cargo build -p editor-app --release
        if ($LASTEXITCODE -ne 0) { throw 'Editor host build failed' }
        $HostExe = Join-Path $projectRoot 'target/release/editor-app.exe'
    }
    $hostPath = (Resolve-Path -LiteralPath $HostExe).Path
    $previousTargetDir = $env:CARGO_TARGET_DIR
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target'
    try {
        & $hostPath --plugin-cargo 'plugins/terminal/Cargo.toml' build --target wasm32-wasip2 --release
        if ($LASTEXITCODE -ne 0) { throw 'Terminal WASM build failed' }
        & $hostPath --plugin-cargo 'plugins/example/Cargo.toml' build --target wasm32-wasip2 --release
        if ($LASTEXITCODE -ne 0) { throw 'Example WASM build failed' }
    } finally { $env:CARGO_TARGET_DIR = $previousTargetDir }
    New-Item -ItemType Directory -Force $Output | Out-Null
    Add-Type -AssemblyName System.IO.Compression
    # Remove legacy package filenames, including the theme now built into the editor.
    foreach ($name in @('terminal', 'example', 'rust', 'toml')) {
        Remove-Item -LiteralPath (Join-Path $Output "me.$name.zip") -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath (Join-Path $Output 'me.default-light-theme.zip') -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $Output 'default-light-theme.zip') -Force -ErrorAction SilentlyContinue
    foreach ($plugin in @(@('terminal', 'terminal_guest'), @('example', 'example_guest'))) {
    # Plugin packages are ordinary ZIP archives with the standard .zip extension.
    $destination = [IO.Path]::GetFullPath((Join-Path $Output "$($plugin[0]).zip"))
    $stream = [IO.File]::Create($destination)
    $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
    try {
        # The manager reads README.md from the installed package version.
        $packageFiles = @(@('manifest.json', "plugins/$($plugin[0])/manifest.json"), @('README.md', "plugins/$($plugin[0])/README.md"), @("$($plugin[0]).wasm", "target/wasm32-wasip2/release/$($plugin[1]).wasm"))
        if ($plugin[0] -eq 'terminal') {
            # Bundle license notices for the adapted Alacritty core and its VTE dependency.
            $packageFiles += ,@('licenses/alacritty-LICENSE-APACHE', 'plugins/terminal/vendor/alacritty_terminal/LICENSE-APACHE')
            $packageFiles += ,@('licenses/vte-LICENSE-APACHE', 'THIRD_PARTY_LICENSES/vte-LICENSE-APACHE')
            # The panel manifest selects the matching SVG when the editor theme changes.
            $packageFiles += ,@('icons/terminal_light.svg', 'plugins/terminal/icons/terminal_light.svg')
            $packageFiles += ,@('icons/terminal_dark.svg', 'plugins/terminal/icons/terminal_dark.svg')
        }
        foreach ($item in $packageFiles) {
            $entry = $archive.CreateEntry($item[0])
            $entryStream = $entry.Open()
            try { $bytes = [IO.File]::ReadAllBytes((Join-Path $projectRoot $item[1])); $entryStream.Write($bytes, 0, $bytes.Length) }
            finally { $entryStream.Dispose() }
        }
    } finally { $archive.Dispose(); $stream.Dispose() }
    Write-Output $destination
    }
    foreach ($name in @('rust', 'toml')) {
        # Declarative language packages contain only resources; the host owns their lifecycle.
        $destination = [IO.Path]::GetFullPath((Join-Path $Output "$name.zip"))
        $stream = [IO.File]::Create($destination)
        $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
        try {
            $pluginRoot = Join-Path $projectRoot "plugins/$name"
            foreach ($file in Get-ChildItem -LiteralPath $pluginRoot -Recurse -File | Sort-Object FullName) {
                $relative = [IO.Path]::GetRelativePath($pluginRoot, $file.FullName).Replace('\', '/')
                $entry = $archive.CreateEntry($relative)
                $entryStream = $entry.Open()
                try {
                    $bytes = [IO.File]::ReadAllBytes($file.FullName)
                    $entryStream.Write($bytes, 0, $bytes.Length)
                } finally { $entryStream.Dispose() }
            }
        } finally { $archive.Dispose(); $stream.Dispose() }
        Write-Output $destination
    }
} finally { Pop-Location }
