# Packaging and development

[简体中文](../../zh-cn/sdk/packaging.md)

A plugin project has a `nanobug-plugin.json` beside its source. This versioned description is shared by the editor's **Plugin Packaging** and **Plugin Development** configurations and command-line entry points. It is separate from the installed `manifest.json` and can be tracked in Git. Local configurations override project and output selection without rewriting it. CLI usage is documented in the repository README.

Select the checkout containing that file. If it is missing, the editor names the missing description and asks you to select a configured project or add its build description. It does not infer an archive policy from `manifest.json` or silently redirect to another checkout.

## Description format 1

```json
{
  "version": 1,
  "manifest": "manifest.json",
  "output": ".",
  "wasm": { "manifest": "Cargo.toml" },
  "assets": [
    { "source": "README.md", "destination": "README.md" },
    { "source": "queries", "destination": "queries" }
  ]
}
```

`version` must be `1`. `manifest` defaults to `manifest.json`; `output` defaults to `.` (the plugin project root). Shared output paths must be relative to the project. A GUI directory selection or CLI override takes precedence and may select a directory outside the source project. Package, protocol, capability and description versions are independent. Packaging never automatically changes the manifest version.

Omit `wasm` for resource-only packages. A Rust guest uses the host's managed embedded SDK and `wasm32-wasip2`; its Cargo manifest is project-relative. Cargo must report exactly one matching `cdylib` WASM artifact; optional `wasm.library` selects its target name. The artifact is copied to the installed manifest's `component` path. No SDK source copy or repository-relative SDK dependency is required. Missing build tools and targets are reported without automatic installation.

`assets` explicitly lists files or directories to copy recursively. Sources are project-relative; destinations are package-relative. The installed manifest is generated automatically and must not be repeated in assets. Include every referenced contribution, grammar, query, icon, license and other resource. Unlisted source files are excluded. Symlinks, escaping paths, duplicate destinations and missing references fail admission. ZIPs contain package files directly at their root. File-count and expanded-size quotas are identical to installation admission; development directories use the same checks.

## Native artifacts

Optional `native` entries invoke an executable and literal argument array in the project directory. They require an exact platform such as `windows-x86_64`, matching the selected and current host platform. Undeclared native cross-compilation is rejected. These entries describe build tools, not runtime native services; the installed manifest still declares service installation and permissions.

```json
{
  "version": 1,
  "native": [
    {
      "program": "rustc",
      "args": ["native/bridge.rs", "-o", "{build}/bridge.exe"],
      "platform": "windows-x86_64"
    }
  ],
  "assets": [
    { "source": "{build}/bridge.exe", "destination": "native/bridge.exe" }
  ]
}
```

Arguments support `{project}`, `{build}` and `{profile}` (`debug` or `release`). `{build}` is a private compiler cache; it is not a distributable. Asset sources may start with `{build}/` for generated native files. Packaged service artifact hashes are calculated from the collected bytes in the distribution manifest, leaving the source manifest unchanged. Native tools are launched directly; shells, script interpreters and archived packaging helpers are rejected. Project build tools and Cargo build scripts execute with the developer's OS authority.

## Publishing a ZIP

Packaging builds all declared components, gathers assets, validates the complete candidate and creates `<id>-<version>.zip`. A validated ZIP atomically replaces an existing archive in the same output directory. Failed builds preserve the old archive. Choosing an asset directory as output does not embed the previous ZIP in its replacement. Multiple projects produce separate ZIPs, each at its own default location unless overridden; a batch reports every project and fails overall if any member fails or destinations collide.

## Development without ZIP

Development builds and validates an immutable directory candidate, then starts an independent editor process. The first run initializes shipped plugins and substitutes the development version when IDs match. Settings, plugin installations, private data, history and run configurations use a separate profile. Data is retained for the same configuration; **Reset development environment** starts a fresh profile and retains the previous profile as a backup. Stop the instance before resetting it.

The test workspace defaults to the plugin source directory and can be selected independently. GUI startup confirms workspace trust and the plugin's permissions; CLI startup requires explicit permission grants. New permissions require stopping and authorizing another run. Changing the development plugin ID also requires restarting the configuration so the shipped-plugin substitution can be rebuilt. These grants do not change the main editor's trust or installation records.

Manual **Reload plugin** rebuilds, validates and transactionally activates a candidate while retaining private data. Optional automatic reload watches declared source/resources, debounces changes and uses compiler caches. Preparation or activation failure retains the previous usable version and reports the error in output. Stale requests from a previous controller cannot switch a restarted instance. Stop owns the development process tree; Force terminates it immediately.

**Run** starts the isolated editor. **Build** prepares artifacts without a ZIP or development window. The development template provides logs and effect inspection; WASM source breakpoints are unavailable and **Debug** is disabled with a reason. These host templates remain available when installed plugins are disabled. See the [user guide](../guide/run-debug-build.md).

Windows has native acceptance coverage. macOS/Linux use portable filesystem and process interfaces but have not been validated on those systems; native build declarations must match the actual OS and architecture.
