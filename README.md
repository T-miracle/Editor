# Nanobug

<p align="center">
  <img src="crates/editor-app/assets/branding/nanobug.png" alt="Nanobug logo" width="128" height="128" />
</p>

**English** · [简体中文](README.zh-CN.md)

Nanobug is a native desktop code editor built with Rust and GPUI, with Windows as its primary platform.

Independent plugins provide language support, a terminal, Markdown and image previews, and run, debug, and build tools. Bundled and third-party plugins use the same public interfaces.

[Documentation](https://t-miracle.github.io/Editor/en/) · [Getting started](https://t-miracle.github.io/Editor/en/guide/getting-started/) · [Plugin SDK](https://t-miracle.github.io/Editor/en/sdk/) · [Report an issue](https://github.com/T-miracle/Editor/issues)

## Installation

Nanobug is under active development. With the development prerequisites below installed, run these commands from the repository root to build a Windows distribution:

```powershell
# Build the Release executable, plugin packages, and first-use catalog.
.\scripts\package-editor.ps1

# Launch the packaged editor.
.\dist\editor\Nanobug.exe
```

The output is in `dist/editor/`. Keep the adjacent `plugins/` directory when distributing or copying the application. Using the compiled editor and plugins does not require Rust or Cargo.

Windows is the primary development and acceptance platform. macOS and Linux have not completed native or cross-build verification.

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

The six workspace crates cover the application, document core, platform integration, plugin schema, public protocol, and plugin runtime. Independent plugins live in `plugins/`; build and verification scripts live in `scripts/`. See the [maintainer documentation index](docs/README.md) for details.

The Cargo package remains `editor-app`. Existing `MeEditor` data directories, `ME_EDITOR_*` environment variables, and persisted command identifiers are retained to preserve settings, history, and installed plugins.

## Plugin development

Plugins build against the SDK embedded in the editor through its `--plugin-cargo` entry point. See the [Plugin SDK](https://t-miracle.github.io/Editor/en/sdk/) for capabilities and packaging, and the [maintainer documentation index](docs/README.md) for host integration and verification.

```powershell
# Build a host that provides the current SDK.
cargo build -p editor-app

# Build independent packages used for running and debugging.
.\scripts\build-plugins.ps1 -HostExe .\target\debug\editor-app.exe -Packages terminal,rust,rust-debugger,run-target-example

# Verify SDK export and independent plugin builds.
.\scripts\verify-plugin-sdk.ps1
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
