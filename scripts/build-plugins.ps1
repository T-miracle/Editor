# Build independent component packages; copy these beside the packaged editor executable.
param([string]$Output = "$PSScriptRoot/../dist/plugins")
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path "$PSScriptRoot/..").Path
Push-Location $projectRoot
try {
    cargo build -p terminal-guest -p example-guest --target wasm32-wasip2 --release
    if ($LASTEXITCODE -ne 0) { throw 'WASM plugin build failed' }
    New-Item -ItemType Directory -Force $Output | Out-Null
    Add-Type -AssemblyName System.IO.Compression
    # Remove the five old package filenames so the bundled-plugin list shows one ZIP per plugin.
    foreach ($name in @('terminal', 'example', 'rust', 'toml', 'default-light-theme')) {
        Remove-Item -LiteralPath (Join-Path $Output "me.$name.zip") -Force -ErrorAction SilentlyContinue
    }
    foreach ($plugin in @(@('terminal', 'terminal_guest'), @('example', 'example_guest'))) {
    # Plugin packages are ordinary ZIP archives with the standard .zip extension.
    $destination = [IO.Path]::GetFullPath((Join-Path $Output "$($plugin[0]).zip"))
    $stream = [IO.File]::Create($destination)
    $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
    try {
        $packageFiles = @(@('manifest.json', "plugins/$($plugin[0])/manifest.json"), @("$($plugin[0]).wasm", "target/wasm32-wasip2/release/$($plugin[1]).wasm"))
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
    foreach ($name in @('rust', 'toml', 'default-light-theme')) {
        # Declarative language and theme packages contain only resources; the host owns their lifecycle.
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
