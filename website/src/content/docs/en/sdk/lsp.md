---
title: Language servers
description: The language.lsp capability, the optional discovery hook, readiness and document lifecycle.
section: sdk
order: 5
alternate: /zh-cn/sdk/lsp/
---

# Language services and snapshot supplements

The manifest uses `protocol: 7` with the required capabilities `language.lsp: ^1` and
`process: ^1`. `language_servers` is independent of recognition and highlighting
contributions; a purely declarative package needs no empty lifecycle WASM component.

```json
{
  "services": {"analysis": {"program": "analysis-server", "args": ["--stdio"]}},
  "permissions": ["process.service.analysis"],
  "language_servers": [{
    "id": "analysis", "language": "novel", "service": "analysis",
    "initialization_options": {"index": true},
    "configuration": {"analysis.features": {"completion": true}},
    "completion_triggers": ["."], "hook": false
  }]
}
```

The host chooses one LSP per language and never overrides an existing choice because of
package installation order. Recognition, highlighting and LSP use separate `recognition:`,
`highlight:` and `lsp:` selection keys that share the user/project precedence and
candidate-removal rules. All services belong to a trusted workspace, and an application-level
plugin cannot declare an LSP.

## Optional WASM hook

With `hook: true` the host sends `Notification::LanguageService(language::Context)` to an
active instance of that package. The context contains the provider ID, the workspace root,
the effective settings with their source, and the fixed service candidates of that provider.
`language.lsp >=1.2` also supplies `package_root` and `data_root`: canonical native paths
owned by this package version and the current isolated private-data scope. They are refreshed
for preparation candidates and after cutover. Use them to configure native resource or cache
locations, never as saved identities. These strings grant no WASI filesystem access, and the
WASM sandbox does not restrict an authorized native service's filesystem access.
The plugin returns `Output.language_service: Some(language::Proposal)`:

- `service`: choose the declared default service or another service from `alternatives`; each
  candidate is authorized separately.
- `project_root`: a workspace-relative directory; once normalized it cannot escape through
  `..`, an absolute path or a link.
- `program`, `args`: discover the service location dynamically and compute start arguments.
  Either field additionally requires **`process.exec`**; holding only the fixed service
  permission cannot override the command. The generic toolchain resolution applies, and no
  concatenated shell string is executed.
- `initialization_options`: written verbatim into the LSP
  `initialize.initializationOptions`.
- `configuration`: a mapping from configuration section names to JSON values;
  `workspace/configuration` is queried by that original name, an unknown section returns
  null, and the host appends no concrete server name.

An omitted field keeps the declared value. `executable_setting` may point at a declared
string setting; an explicit user or project value must be a valid absolute executable path
and takes precedence over a discovered value. An explicit error is reported as a preparation
failure and does not fall back to another program.

With the required capability `dependencies >=1.1`, a provider may explicitly declare
`optional_installation: true`. On first installation, failed preparation of that provider's
dependency allows its resource features to activate; its service is reported as unavailable
and cannot start without a valid prepared cache. Cancellation still aborts installation.
Updates retain mandatory preparation and preserve the old version on failure. Reinstalling
the package retries preparation; an explicit local executable can avoid the download.

The hook uses the ordinary WASM call budget and memory limits and may read authorized package
resources, workspace files or private files; it cannot write files, start processes, operate
the editor, subscribe to events or publish UI or snapshots. Temporary file handles are
released when the call ends. Hook output is checked before publication in every scenario, and
initialization plus configuration together are bounded by the 256 KiB declaration budget.

## Pure completion supplements

With the required capability `language.completion: ^1`, `editor.read` permission and
`completion_hook: true`, a workspace language provider can supplement its native suggestions.
The host sends `Notification::LanguageCompletion(language::CompletionRequest)` to a separate
pure guest executor. `source` contains a `DocumentVersion` (open editor ID, workspace-relative
path and revision) and at most 1 MiB of UTF-8 `text`; `cursor` is a UTF-8 byte offset.
`provider`, the monotonic `request` ID and effective `settings` complete the input. Request IDs
are distinct from document revisions. There are no editable document handles.
`uri` is the logical document's standard URI (at most 8192 bytes), before native snapshot aliases.
`diagnostics` optionally carries standard code/message summaries proven for the exact same source;
None means not yet known. There are at most 128 summaries, with a string/integer `code` (string
codes at most 128 bytes) and a UTF-8 `message` preview of at most 1024 bytes. The host applies
no language-specific interpretation; a plugin may distinguish unavailable rules from content errors.

Return `Output.language_completion` containing the same request and document metadata, with
at most 256 items: `label` (256 bytes), `replace` (UTF-8 range containing the caret), and
`new_text` (4096 bytes). The complete encoded proposal is limited to 256 KiB. The host validates
every character boundary, converts ranges to UTF-16, and keeps native entries before supplements;
same-label or identical-edit entries are deduplicated. Returning no items preserves native output.

The separate executor never restores private state or activates resources. All host operations
are denied from its first preparation instruction, including assets, files, processes, editor
operations and subscriptions. It cannot publish UI, configuration or a private snapshot.
Calls use the ordinary fuel, 1 second and 256 MiB limits. Failed supplements retain native
results and report their actual error; provider retirement revokes the executor and rejects late
results. The editor rechecks the current source, open entity and revision before publication.
Language policy such as avoiding unconstrained history suggestions for a Schema-bound file,
while retaining them after an observed rule-loading failure, belongs in the plugin.

## Readiness, documents and lifecycle

By default, completing the `initialize` response and the `initialized` notification means
ready. A provider may declare `readiness` with `notification`, an RFC 6901 `pointer` into the
params, an expected JSON `expected` value and a `timeout_ms` of 1–300000; the host reads real
notifications until they match or the timeout expires. Ready does not mean every index has
finished.

The host provides stdio JSON-RPC, UTF-16 positions, didOpen/didChange/didClose, optional
didSave, diagnostics, definition, completion and hover. After installation it rebinds open
documents using the unsaved text held in `EditorState`. Document versions increase
monotonically within a connection, and reopening the same URI does not reuse a version.
Pushed diagnostics from a newer interface must carry a version; otherwise they are ignored in
favour of pull diagnostics when the service supports them, so an unattributable late result
cannot overwrite newer text.
Providers opting into `diagnostic_snapshots: true` require `language.lsp >=1.3`. The host
gives each immutable document revision a distinct wire URI, and accepts unversioned pushes
only for its currently live mapping. Replaced/closed snapshots are revoked; no URI is reused
within a connection. Physical local paths and normal navigation targets retain their identity.
For file URIs, `language::SNAPSHOT_URI_SEGMENTS` (64) redundant dot segments are inserted after
the volume root using a unique percent-encoding case pattern. Plugins whose native server
matches absolute path globs can add an equivalent companion pattern with 64 `./` segments;
basename and suffix globs remain unchanged. This preserves path boundaries, without granting
access or broadening globs. A server must support this opt-in URI strategy.
Navigation calls only the advertised standard `definition`/`typeDefinition` methods; an empty
definition result may fall back to type definition. Schema/type targets retain the same document
lease and revision checks.

An ordinary request waits 30 seconds, then sends `$/cancelRequest` and stops waiting;
cancelling does not mean the server rolled back its side effects. Stopping or switching a
provider terminates its process tree and rejects new requests and late results from the old
adapter. The host keeps bounded JSON-RPC frames and message queues; stdout is the protocol
stream and stderr is drained continuously.

A configuration change recomputes the plan and replaces only the services whose final root,
program, argv, initialization or configuration changed. Disabling, uninstalling, replacing a
package, closing or revoking a workspace revokes the start lease, so a new start is refused
even if an older background task still holds a reference. Re-selecting a provider that is
still installed uses a new adapter lifecycle.

A native program still runs with the current user's authority, and the WASM sandbox cannot
constrain its internal operations. Service installation and dependency download are managed
by the separate dependency capability; this capability never downloads a tool implicitly.
