# Nanobug

<p align="center">
  <img src="crates/editor-app/assets/branding/nanobug.png" alt="Nanobug logo" width="128" height="128" />
</p>

**English** · [简体中文](README.zh-CN.md)

Nanobug is a native desktop code editor built with Rust and GPUI, with Windows as its primary platform.

Independent plugins provide language support, a terminal, Markdown and image previews, and run, debug, and build tools. Bundled and third-party plugins use the same public interfaces.

[Documentation](https://t-miracle.github.io/Editor/en/) · [Getting started](https://t-miracle.github.io/Editor/en/guide/getting-started/) · [Plugin SDK](https://t-miracle.github.io/Editor/en/sdk/) · [Report an issue](https://github.com/T-miracle/Editor/issues)

## Installation

Nanobug is under active development. On Windows, run `Nanobug-Setup-<version>-x64.exe` to install for your user account. The installer creates a Start menu entry, offers an optional desktop shortcut, and registers an uninstaller. It installs separate program and plugin files under `%LOCALAPPDATA%\Programs\Nanobug`; Rust and Cargo are not required to use them.

Exit Nanobug before installing an update or uninstalling. Settings, history, and installed plugin data in the existing `MeEditor` directories are preserved. Windows installers include English and Simplified Chinese.

With the development prerequisites below and [Inno Setup](https://jrsoftware.org/isdl.php) prepared, build the host directly:

```powershell
# Build the native Release host; direct development launches also remain available.
cargo build -p editor-app --release
cargo run --release
```

Prepare the executable, plugin ZIPs, first-use catalog, icon, and licenses as described in [native packaging](installer/README.md). Then compile the prepared Windows payload directly from the repository root:

```powershell
# Read the actual host version and invoke the native installer compiler directly.
$version = ((cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages |
    Where-Object name -eq 'editor-app').version
$payload = (Resolve-Path .\dist\editor).Path
$output = Join-Path $PWD 'dist/installers'
ISCC.exe "/DPayloadDir=$payload" "/DInstallerDir=$output" "/DAppVersion=$version" .\installer\windows\nanobug.iss
```

Installers are written to `dist/installers/`; prepared runtime files remain in `dist/editor/`. Specify the compiler's full path if ISCC is not on PATH. Keep adjacent `plugins/` and `licenses/` directories for a portable copy.

The repository contains no helper-script directory. Packaging must call Cargo, the host's public CLI, archive tools, and native installer tools directly; it must not invoke archived Codex helper scripts, including through wrappers.

Phase one validated Windows installation, in-place repair, running-app protection, and uninstall. macOS application bundles, DMG/PKG, and Linux DEB/RPM use direct native tools; preparation and commands are documented in [native packaging](installer/README.md). Those platforms are **not built or installation-tested in this phase**. Signing/notarization and platform-specific plugin dependencies require separate release work.

## Developing Nanobug

On Windows, development requires the Rust MSVC toolchain (Rust 1.95 or newer), C++ build tools, and the Windows SDK. Building plugins and the complete distribution also requires the `wasm32-wasip2` target. See [Cargo.toml](Cargo.toml), [rust-toolchain.toml](rust-toolchain.toml), and [Cargo.lock](Cargo.lock) for version and dependency constraints.

Run from the repository root:

```powershell
# Start the Debug development build.
cargo run

# Start an optimized Release build.
cargo run --release

# Open a workspace; a file path is also accepted.
cargo run --release -- C:\path\to\project
```

Debug builds are unoptimized; window movement and editing may be slower. Use Release for everyday evaluation.

The six workspace crates cover the application, document core, platform integration, plugin schema, public protocol, and plugin runtime. Independent plugins live in `plugins/`; native packaging configuration and instructions live in `installer/`. See the [maintainer documentation index](docs/README.md) for details.

The Cargo package remains `editor-app`. Existing `MeEditor` data directories, `ME_EDITOR_*` environment variables, and persisted command identifiers are retained to preserve settings, history, and installed plugins.

## Plugin development

Plugins build against the SDK embedded in the editor through its `--plugin-cargo` entry point. See the [Plugin SDK](https://t-miracle.github.io/Editor/en/sdk/) for capabilities and packaging, and the [maintainer documentation index](docs/README.md) for host integration and verification.

```powershell
# Build a host that provides the current SDK.
cargo build -p editor-app

# Build an independent component through the actual host's public SDK entry point.
.\target\debug\editor-app.exe --plugin-cargo plugins/terminal/Cargo.toml build --target wasm32-wasip2 --release

# Export the embedded SDK directly for inspection or an external toolchain.
.\target\debug\editor-app.exe --export-plugin-sdk .\target\sdk-export
```

Install or update the generated packages in the plugin manager and approve their declared permissions. See the [run, debug, and build guide](https://t-miracle.github.io/Editor/en/guide/run-debug-build/) for usage and the [Rust debugger README](plugins/rust-debugger/README.md) for dependency preparation and authorization.

## Contributing

Use [GitHub Issues](https://github.com/T-miracle/Editor/issues) to report bugs or propose changes. Before editing, find the relevant specification and acceptance requirements in the [maintainer documentation index](docs/README.md).

Basic checks for Rust changes:

```powershell
# Check Rust formatting.
cargo fmt --check

# Run workspace tests excluding the UI application.
cargo test --workspace --exclude editor-app

# Check compilation of the entire workspace.
cargo check --workspace
```

Changes to `editor-app` also require relevant UI tests and native interaction checks. Tests marked ignored that use actual WASM packages require their fixtures to be built before explicitly running them. Documentation-only changes require link and path checks plus `git diff --check`.

Reader-facing guides and SDK documentation live in `website/`; see [website/README.md](website/README.md) for the site build. Specifications, tickets, and verification records live in `docs/`. Keep this English README and [its Chinese translation](README.zh-CN.md) in sync.

## Licensing

Workspace packages declare the MIT license in [Cargo.toml](Cargo.toml). Third-party dependencies and plugin grammars, icons, and other assets retain their respective licenses; see the corresponding plugin directories and distribution packages for attribution and license texts.
