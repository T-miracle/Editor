---
title: Plugin contract
description: How to build a plugin package — capability negotiation, native UI, services, processes and packaging.
section: sdk
order: 0
alternate: /zh-cn/sdk/
---

# Plugin contract

This section is for people building plugin packages. A plugin is a declarative package,
optionally carrying a WebAssembly component for behaviour that cannot be declared.

The contract is versioned and negotiated: a manifest states which capabilities it needs and
which are optional, and the editor checks those declarations before an instance runs. A
plugin that requires a capability the editor cannot provide is rejected with a reason rather
than started in a degraded state.

## Stability and capability compatibility

Each capability page states its version and whether it is stable or experimental. A stable
capability keeps existing operation shapes and meaning compatible within one major version;
minor releases may add features. A plugin using a later addition must declare that feature's
minimum capability version and check the negotiated result. Experimental capabilities are
explicitly labelled and do not carry the stable compatibility promise; permission, ownership
and resource cleanup rules still apply.

An unknown, unavailable or mismatched `api.required` capability rejects the package before
activation. An unavailable `api.optional` capability is omitted from the prepare negotiation,
including an unknown optional ID; the plugin must disable that feature or provide its own
fallback. Optional negotiation does not grant permission or make an unsupported operation
callable.

The minimum host is determined by its supported wire/base protocol and required capability
versions, rather than a guessed Nanobug product version. The current SDK uses `protocol = 7`
and `api.base = ^1`; a capability page supplies the additional minimum range. Package, SDK,
wire protocol and capability versions are managed separately. This policy introduces no
support for future or historical wire protocols.

A breaking change requires a new capability major version. Before removing a stable API,
publish its deprecation, affected version range, replacement, minimum replacement capability
and migration steps; it remains compatible through its current major.

Cross-plugin collaboration, versioned contracts, provider selection and origin permissions
are described in [Plugin services](/en/sdk/services/). Native services, interactive
processes, permissions and reclamation are described in
[Native processes](/en/sdk/processes/). Purely declarative language packages, and
independently contributed recognition and highlighting providers, are described in
[Language packages](/en/sdk/languages/); they do not need an empty lifecycle component, and
install, enable, update and uninstall stay in sync with documents that are already open.
Private storage, data-format migration and the transactional cutover that replaces a running
instance are described in [Private data and migration](/en/sdk/migration/). The budgets
applied to guest execution, and what recovery does and does not promise, are described in
[Execution limits and recovery](/en/sdk/faults/).

## Service discovery and language.lsp 1.1

A native service may declare `search_paths` and `check_args`. The former holds at most 32
absolute path globs and the only supported variable is a leading `${HOME}/`; it must not
contain `..`, control characters or a recursive `**`. Candidates are searched in the declared
order, entries matching one pattern are ordered by reverse path, and absolute `PATH`
directories are searched afterwards. A search visits at most 20,000 directory entries, keeps
at most 256 intermediate candidates and 256 discovered results, and the whole discovery is
limited to five seconds; a guest request is additionally bounded by the shorter deadline of
that call. `check_args` runs as a literal argument array without a shell, and probing is
limited to two seconds sharing the remaining discovery deadline; a candidate that exits with
a failure is skipped. Every probe uses the host's process ownership and cleanup rules.

An explicit absolute path in a service's `program` or `executable_setting` has exactly one
candidate, and an error is reported directly instead of falling back to a search result.
Approving a service permission approves the declared lookup and probing behaviour; these
fields grant no permission to run an arbitrary executable.

An LSP provider may declare `client_experimental`, which is passed to
`initialize.capabilities.experimental`; standard transport capabilities remain owned by the
host. It holds at most 64 non-empty keys of at most 256 bytes each, with a JSON depth of at
most 16, and counts against the provider's 256 KiB declaration limit. The host recognises no
specific service name, extension field or Rust project structure.

## The current capability protocol

The manifest's `protocol` marker selects the typed messages of the `api` module. It is not
one version number shared by every feature: `api.base` carries the base-protocol SemVer
range, while `api.required` and `api.optional` declare capability IDs with their own version
ranges. The current base API is 1.0.0 and provides `package.assets` 1.0.0 and `ui.native`
1.0.0; the plugin package version is managed separately.

A missing required interface or a version mismatch is rejected during package checks and
instance recovery. An unavailable optional interface simply does not appear in the
negotiation result of the prepare phase.

A plugin receives typed lifecycle messages through `api::guest::dispatch` and reads its
resources through `api::guest::read_asset`. The SDK generates non-zero request IDs, encodes
messages and verifies the request ID in responses. Missing methods, invalid arguments,
unknown operations, unnegotiated capabilities, insufficient permissions, illegal paths and
exceeded quotas are returned as a `Failure`; genuine transport errors that prevent decoding
or calling the component still use the WIT error channel. Synchronous resource reads return
their final result directly instead of fabricating a "processing" stage.

Availability of the `package.assets` capability is not authorization: the manifest must also
declare `assets.read`, and the user approves it at install time. The operation reads only
resources inside the current package version; the prepare phase may read them too, because
they are immutable. It cannot read the workspace or another plugin's directory.

## Native UI

New UI output uses `api::View { panel, document }`, and an ordinary form does not require a
canvas or character-grid field. The host checks that `ui.native` was negotiated and that the
panel was declared. The `ui.canvas ^1` capability allows a canvas anywhere in the same tree,
while `ui.grid ^1` separately provides optional character measurement. Native notifications
keep their panel and node scope. Editor preview uses `editor.documents` with `editor.read`,
driven by versioned in-memory text notifications. Legacy canvas and PTY messages do not
become part of the new base protocol automatically; the composition protocol is documented
in [Native UI](/en/sdk/ui/).

## Installation and protocol compatibility

Package installation and instance recovery currently require `protocol = 7` and a negotiable
base API. Installation records from protocols 1–6 keep their settings, permissions,
enablement scope and private data, and are shown as needing an update; their old components
are never activated. Rebuilding and installing a new package of the same plugin with the
current SDK restores use.

Old runtime protocols and their converters have been removed. Legacy installation records
only take part in management-view display and limited data import; they never resume
execution.

## Run, debug and target services

[Run sessions](/en/sdk/sessions/) specify public input, presentation, output subscriptions and normal/forced stopping. [Debug sessions](/en/sdk/debug/) specify pause generations, real inspection and breakpoint verification. [Run target providers](/en/sdk/targets/) contribute portable bindings and controlled artifact preparation.

[Configuration templates](/en/sdk/configurations/) provide plugin-owned defaults, native forms and validation for local configuration drafts.

## workspace.files 1.1 and host.sdk 1.0

`api::guest::find_files(&workspace, FileQuery { include, exclude, max_results })` uses the
workspace root handle returned by `open_workspace`, and re-checks `workspace.files >= 1.1`
together with `workspace.read` on every call. Handles for private data, other instances,
released handles and retired handles cannot be used for discovery. Application-scoped
plugins do not own a workspace root, and a call never follows whichever other workspace is
currently selected.

Queries use root-relative globs separated by `/`. `**/` may match zero directory levels and
`*` does not cross a directory boundary. Absolute paths, drive prefixes, backslashes, empty
segments and `.` or `..` segments are invalid. `include` needs at least one entry; include
and exclude together may hold at most 32 entries of at most 1024 bytes each, and
`max_results` ranges from 1 to 4096. Excludes prune directory entries, so `**/generated/**`
never enters a `generated` directory. The host ships no built-in language or build-directory
names; the plugin declares its own selection rules.

Discovery honours nested and negated rules from `.gitignore` and `.ignore` inside the root,
with `.ignore` taking precedence. It does not read parent or global ignore files outside the
workspace, and it does not follow symbolic links, Windows junctions or other reparse points.
`FileMatches.paths` are deduplicated, sorted UTF-8 workspace-relative paths; `skipped` holds
relative paths that could not be read or parsed, and the caller decides whether to accept an
incomplete discovery. The bounded budget covers 50,000 directory entries including ignored
ones, 64 levels of directory depth, 512 KiB of encoded results and at most 64 skipped
entries; a single ignore file may hold 64 KiB and all of them 512 KiB or 2048 lines.
Exceeding a limit returns `LimitExceeded` instead of a truncated success. Traversal checks
the current plugin call deadline between filesystem calls and returns `TimedOut`.

`api::guest::describe_sdk()` requires the `host.sdk` capability and returns
`SdkDescriptor { digest, root, cargo_config }` without needing workspace read permission. It
describes the same interface cache the host itself uses: the digest is a content identity,
and `root` plus `cargo_config` are absolute paths for native language tooling. It creates no
file handles, no WASI preopen and no extra file-read permission; handing those paths to the
workspace `read_file` operation is still rejected. The host returns `NotFound` when it has no
SDK, `OperationFailed` when export fails, and `CapabilityUnavailable` when the capability was
not negotiated.

Both operations may be called by an active instance and from a read-only `LanguageService`
prepare hook. The hook may close the file handles it opened during the call; remaining
temporary handles are revoked when it returns. It cannot start writes, processes or editor
operations through discovery. Migration hooks still have private-copy permission only. The
host supplies an immutable `HostResources` through `Manager::open_with_resources`, and the
same resource snapshot reaches background installation, settings replacement, workspace
switching and instances restored after a failed rollback.

## ui.clipboard 1.0 and storage.editor 1.0

`EditorOperation::ReadClipboard` and `WriteClipboard { text }` return
`EditorValue::Clipboard { text }` and `Unit` respectively. They require the negotiated
`ui.clipboard` capability and the approved `clipboard` permission, with at most 1 MiB of text
per call. Operations run through the editor request queue, so `Accepted` only means the
request was queued; a plugin must wait for the completion notification of that request and
must not apply a delayed result to a target that has since been switched or closed.

`EditorOperation::OpenDataFile { path }` requires the negotiated `storage.editor` capability
and the `storage` permission. The path is relative, uses `/`, and lives inside the plugin's
own private directory; `..`, absolute paths and links that escape are rejected. Success
returns `Unit` and the file enters the host's normal document lifecycle. It cannot open
another plugin's file or an arbitrary file on the machine.

All three operations are currently available only in an active workspace instance and follow
the shared timeout, cancellation and instance-revocation rules. Native host code checks the
request state before performing a side effect; cancelling does not mean rolling back a
clipboard write or a document open that already happened.

## configuration 1.0

A protocol 7 manifest declares `settings` with `title`, `value_type`, `default`, `scope` and
`apply` for each plugin-internal key. Types are boolean, string with `max_length`, integer
with `min` and `max`, and enum with `choices`; scopes are `user` or `project`, and the latter
allows an explicitly confirmed project override. In this version the effect of an applied
value is `restart_instance`. A manifest may declare at most 64 fields, strings are at most
4096 bytes, and an enum holds at most 32 distinct choices. Invalid defaults are rejected
during package checks, and an executable package must declare the required
`configuration: ^1` capability.

The optional `settings_hook: true` receives `Notification::Configuration { phase: Validate,
values }` and returns discovered values and errors through `Output.configuration: Proposal`.
The hook runs while the candidate instance is being prepared, inherits bounded instructions
and a memory budget, and cannot acquire active-instance resources or authority; reading
package resources it already has permission for still works. Discovered values only fill
entries that were not set explicitly, and any error fails the application. The apply phase
then receives the final values together with their source, for `Activate` to use; a failure
in that phase also leaves the old instance in place. Configuration is confined to the plugin
namespace: a guest cannot obtain permissions or modify host user settings through it.

User settings are independent of guest private files, and confirmed project values live in
the workspace record managed by the host rather than being trusted from repository files.
Hot application, initialization and reopening a workspace all use the same resolution path.
A failed application keeps the previous configuration and the running instance; a successful
one replaces only the affected instances and revokes their earlier resources.

## Independent builds and SDK distribution

The protocol crate is the versioned plugin interface of the main program. `wit/plugin.wit`
defines the imports and exports of the WebAssembly Component Model, while the
`plugin-protocol` Rust crate defines the JSON messages, documents and permission names that
travel across it. The main program compiles these interface files into its executable and
manages the interface cache that plugin compilation needs.

`host.request` is the import the main program provides at runtime. Plugins use typed
capability requests through `api::guest`; the main program checks the negotiated capability,
the plugin permission, the instance scope and the handle on every call. WIT and the Rust
types are used only while compiling a plugin: an installed `.wasm` never reads SDK files.

A plugin declares `plugin-protocol = { version = "=0.2.0", features = ["guest"] }` in its
`Cargo.toml` and uses `plugin_protocol::bindings::{Guest, editor, export}` for host calls and
component exports, without generating WIT bindings itself. Compile an independent plugin by
invoking the packaged editor:

```powershell
# Invoke Nanobug from its distribution directory.
.\Nanobug.exe --plugin-cargo capability-example/Cargo.toml build --target wasm32-wasip2 --release
# The same entry point supports native unit tests and compile checks.
.\Nanobug.exe --plugin-cargo capability-example/Cargo.toml test --lib
.\Nanobug.exe --plugin-cargo capability-example/Cargo.toml check --target wasm32-wasip2
```

The editor caches its embedded interface by content digest under the system user cache
directory at `MeEditor/plugin-sdk/<digest>/` (`%LOCALAPPDATA%/MeEditor/plugin-sdk/<digest>/`
on Windows) and selects that cache through a dependency override for the duration of the
Cargo command. Different interface versions never overwrite each other, and a missing or
damaged cache is repaired automatically. A plugin project needs no `sdk/` directory and does
not reference main-program sources; a distribution directory only needs the main program and
the plugin packages. When the interface changes, check both the WIT package version and the
`protocol` version in plugin manifests.

A development machine needs Rust and Cargo with the `wasm32-wasip2` target; users of compiled
plugins need neither. `--plugin-cargo` executes a developer-provided Cargo project and is a
local development tool, not a runtime sandbox. `--export-plugin-sdk <directory>` remains
available for other language toolchains and for interface inspection; ordinary builds and
distribution do not call it.

Developing a plugin inside this editor, a Rust plugin obtains the same host SDK cache as
`--plugin-cargo` through `host.sdk` and discovers independent plugin projects through
`workspace.files`. The plugin's WASM hook produces Rust Analyzer's `cargo.configPath`,
`linkedProjects` and configuration sections, and the host passes that data through the
generic LSP protocol. Project discovery follows ignore rules, and the Rust plugin declares
which build outputs and vendor directories to exclude. Typing `plugin_protocol::api::`
provides type completion, hover documentation and go-to-definition. A plugin directory needs
no SDK, no Cargo configuration and no path into host sources; go-to-definition opens the
protocol sources in the host cache. This needs the Rust language plugin enabled and Rust
Analyzer installed.
[Semantic viewports](/en/sdk/viewport/), [links and navigation](/en/sdk/navigation/) and [read-only code highlighting](/en/sdk/code-highlighting/) add versioned native preview interactions without creating another mutable document.

[Documents and readonly resources](/en/sdk/documents/) cover exact unsaved snapshots, ordered metadata subscriptions, instance-owned virtual Tabs and native comparison.
