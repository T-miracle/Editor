# Build independent component packages; copy these beside the packaged editor executable.
param(
    [string]$Output = "$PSScriptRoot/../dist/plugins",
    [string]$HostExe = '',
    # Restrict verification to named packages without rebuilding unrelated components.
    [ValidateSet('terminal', 'example', 'svg', 'rust', 'toml', 'html', 'javascript', 'markdown')]
    [string[]]$Packages = @('terminal', 'example', 'svg', 'rust', 'toml', 'html', 'javascript', 'markdown')
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
        if ($Packages -contains 'terminal') {
        & $hostPath --plugin-cargo 'plugins/terminal/Cargo.toml' build --target wasm32-wasip2 --release
        if ($LASTEXITCODE -ne 0) { throw 'Terminal WASM build failed' }
        }
        if ($Packages -contains 'example') {
        & $hostPath --plugin-cargo 'plugins/example/Cargo.toml' build --target wasm32-wasip2 --release
        if ($LASTEXITCODE -ne 0) { throw 'Example WASM build failed' }
        }
        # File-scoped previews compile against the same host-managed interface as dock plugins.
        if ($Packages -contains 'svg') {
        & $hostPath --plugin-cargo 'plugins/svg/Cargo.toml' build --target wasm32-wasip2 --release
        if ($LASTEXITCODE -ne 0) { throw 'SVG preview WASM build failed' }
        }
        # Rust analysis policy is an independent guest built solely against the exported public SDK.
        if ($Packages -contains 'rust') {
        & $hostPath --plugin-cargo 'plugins/rust/Cargo.toml' build --target wasm32-wasip2 --release
        if ($LASTEXITCODE -ne 0) { throw 'Rust language WASM build failed' }
        }
        # Markdown owns parsing inside an ordinary file-scoped guest built against the public SDK.
        if ($Packages -contains 'markdown') {
        & $hostPath --plugin-cargo 'plugins/markdown/Cargo.toml' build --target wasm32-wasip2 --release
        if ($LASTEXITCODE -ne 0) { throw 'Markdown preview WASM build failed' }
        }
    } finally { $env:CARGO_TARGET_DIR = $previousTargetDir }
    New-Item -ItemType Directory -Force $Output | Out-Null
    Add-Type -AssemblyName System.IO.Compression
    # Remove legacy package filenames, including the theme now built into the editor.
    foreach ($name in @('terminal', 'example', 'svg', 'rust', 'toml', 'html', 'javascript', 'markdown')) {
        Remove-Item -LiteralPath (Join-Path $Output "me.$name.zip") -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath (Join-Path $Output 'me.default-light-theme.zip') -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $Output 'default-light-theme.zip') -Force -ErrorAction SilentlyContinue
    # Retire the old SVG filename so the market never offers two identities for the same plugin.
    foreach ($legacySvgPackage in @('svg-preview.zip', 'me.svg-preview.zip')) {
        Remove-Item -LiteralPath (Join-Path $Output $legacySvgPackage) -Force -ErrorAction SilentlyContinue
    }
    foreach ($plugin in @(@('terminal', 'terminal_guest'), @('example', 'example_guest'), @('svg', 'svg_guest'))) {
    if ($Packages -notcontains $plugin[0]) { continue }
    # Plugin packages are ordinary ZIP archives with the standard .zip extension.
    $destination = [IO.Path]::GetFullPath((Join-Path $Output "$($plugin[0]).zip"))
    $stream = [IO.File]::Create($destination)
    $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
    try {
        # The manager reads README.md from the installed package version.
        $packageFiles = @(@('manifest.json', "plugins/$($plugin[0])/manifest.json"), @('README.md', "plugins/$($plugin[0])/README.md"), @("$($plugin[0]).wasm", "target/wasm32-wasip2/release/$($plugin[1]).wasm"))
        if ($plugin[0] -eq 'terminal') {
            # Bundle the upstream WASM-compatible core's license alongside its VTE dependency.
            $packageFiles += ,@('licenses/term-wm-vt100-LICENSE', 'THIRD_PARTY_LICENSES/term-wm-vt100-LICENSE')
            $packageFiles += ,@('licenses/vte-LICENSE-APACHE', 'THIRD_PARTY_LICENSES/vte-LICENSE-APACHE')
            # The panel manifest selects the matching SVG when the editor theme changes.
            $packageFiles += ,@('icons/terminal_light.svg', 'plugins/terminal/icons/terminal_light.svg')
            $packageFiles += ,@('icons/terminal_dark.svg', 'plugins/terminal/icons/terminal_dark.svg')
        }
        if ($plugin[0] -eq 'svg') {
            # Keep editable vector sources beside the component that embeds the same toolbar assets.
            foreach ($icon in @('zoom-in', 'zoom-out', 'actual-size', 'fit-window')) {
                $packageFiles += ,@("icons/$icon.svg", "plugins/svg/icons/$icon.svg")
            }
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
    foreach ($name in @('rust', 'toml', 'html', 'javascript', 'markdown')) {
        if ($Packages -notcontains $name) { continue }
        # Policy and document-preview guests may accompany the same declaration-only language resources.
        $destination = [IO.Path]::GetFullPath((Join-Path $Output "$name.zip"))
        $stream = [IO.File]::Create($destination)
        $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
        try {
            $pluginRoot = Join-Path $projectRoot "plugins/$name"
            # Explicit distribution roots prevent Cargo/source/cache files from leaking into the Rust ZIP.
            $packageFiles = @('manifest.json', 'README.md', 'plugin.toml') | ForEach-Object {
                ,@($_, (Join-Path $pluginRoot $_))
            }
            # Resource-only languages may reuse the host's generic file icon without an icon catalog.
            if (Test-Path -LiteralPath (Join-Path $pluginRoot 'icons.json')) {
                $packageFiles += ,@('icons.json', (Join-Path $pluginRoot 'icons.json'))
            }
            foreach ($directory in @('grammar', 'queries', 'icons')) {
                if (-not (Test-Path -LiteralPath (Join-Path $pluginRoot $directory))) { continue }
                foreach ($file in Get-ChildItem -LiteralPath (Join-Path $pluginRoot $directory) -Recurse -File | Sort-Object FullName) {
                    $relative = [IO.Path]::GetRelativePath($pluginRoot, $file.FullName).Replace('\', '/')
                    $packageFiles += ,@($relative, $file.FullName)
                }
            }
            if ($name -eq 'rust') {
                $packageFiles += ,@('rust.wasm', (Join-Path $projectRoot 'target/wasm32-wasip2/release/rust_language_guest.wasm'))
            }
            if ($name -eq 'markdown') {
                $packageFiles += ,@('markdown.wasm', (Join-Path $projectRoot 'target/wasm32-wasip2/release/markdown_guest.wasm'))
                $packageFiles += ,@('licenses/pulldown-cmark-LICENSE', (Join-Path $pluginRoot 'src/pulldown-cmark-LICENSE'))
            }
            foreach ($item in $packageFiles) {
                $entryStream = $archive.CreateEntry($item[0]).Open()
                try {
                    $bytes = [IO.File]::ReadAllBytes($item[1])
                    $entryStream.Write($bytes, 0, $bytes.Length)
                } finally { $entryStream.Dispose() }
            }
        } finally { $archive.Dispose(); $stream.Dispose() }
        Write-Output $destination
    }
    # This release policy is data consumed by the generic first-use host flow, never a host ID branch.
    # Hash only ZIPs present in this output: partial builds must not point to missing packages.
    $bundlePackages = @()
    foreach ($defaultPackage in @(@{ file = 'markdown.zip'; file_extensions = @('md', 'markdown') })) {
        $packagePath = Join-Path $Output $defaultPackage.file
        if (Test-Path -LiteralPath $packagePath -PathType Leaf) {
            $bundlePackages += @{
                file = $defaultPackage.file
                sha256 = (Get-FileHash -LiteralPath $packagePath -Algorithm SHA256).Hash.ToLowerInvariant()
                file_extensions = $defaultPackage.file_extensions
            }
        }
    }
    $catalog = @{ version = 1; packages = @($bundlePackages) } | ConvertTo-Json -Depth 5
    $catalogPath = [IO.Path]::GetFullPath((Join-Path $Output 'bundle-defaults.json'))
    $catalogTemporary = Join-Path (Split-Path -Parent $catalogPath) ('.bundle-defaults-' + [guid]::NewGuid().ToString('N') + '.tmp')
    try {
        # Atomic replacement prevents a reader from observing a half-written catalog after a package build.
        [IO.File]::WriteAllText($catalogTemporary, $catalog, [Text.UTF8Encoding]::new($false))
        [IO.File]::Move($catalogTemporary, $catalogPath, $true)
    } finally {
        if (Test-Path -LiteralPath $catalogTemporary -PathType Leaf) {
            Remove-Item -LiteralPath $catalogTemporary
        }
    }
    Write-Output $catalogPath
} finally { Pop-Location }
