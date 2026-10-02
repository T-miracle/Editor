# Private native service dependencies

Negotiate `dependencies: ^1` and request `dependencies.prepare` in addition to the
service's `process.service.<id>` grant. Consent permits data-only preparation of
declared or WASM-resolved plans, not execution of an installer or a shell script.

`process::Service.installation` optionally supplies a `dependencies::Plan`:

```json
{
  "program": "analysis-server",
  "args": ["--stdio"],
  "installation": {
    "executable": "server/bin/server.exe",
    "artifacts": [{
      "id": "server",
      "version": "1.2.3",
      "platform": "windows-x86_64",
      "sha256": "<64 hexadecimal digits of the exact archive>",
      "source": {"kind": "url", "url": "https://example.org/server-1.2.3.zip"},
      "format": {"kind": "zip"},
      "requires": []
    }]
  }
}
```

Other sources: `{"kind":"package","path":"tools/server.zip"}` and
`{"kind":"local","path":"C:/offline/server.zip"}`. Package paths are relative;
local source paths must be absolute. A plain executable uses
`{"kind":"file","path":"bin/server.exe"}` instead of ZIP. Checksums apply to
the downloaded/source file before unpacking. HTTPS is required except literal
loopback HTTP addresses, which support local repositories and test fixtures.
Credentials in URLs are rejected. HTTPS redirects remain HTTPS; loopback HTTP
does not redirect. Requests use TLS verification, finite timeouts and size quotas.

Each artifact declares OS-architecture (`windows-x86_64`, for example). A plan
must match the current host. Use alternative service declarations and a hook
to select platform-specific plans. `requires` contains artifact IDs in the same
plan; missing references, duplicates and cycles fail before downloads. Limits:
32 artifacts, 64 KiB plan, 128 MiB source file, 256 MiB expanded archive,
10,000 ZIP entries. Symlinks and escaping/duplicate file entries are rejected.

A language hook can return `language::Proposal.installation`. The same bounded,
read-only hook runs on the prepared candidate, before retiring the old plugin.
It may read consented package/workspace/private files, but cannot start programs,
write files, publish UI or request another installation. The host downloads,
hashes, unpacks and resolves the executable through its toolchain boundary.

Resolution order: explicit user/project executable setting; hook native program
(requires `process.exec`); hook installation; declared installation; declared
local/PATH program. An explicit invalid path fails without fallback. A native
program override skips unused downloads. Hooks can set initialization and
configuration independently of dependency resolution.

Additional runtime artifacts remain in private version directories. An argument
starting `${dependency:runtime}/bin/entry` resolves to that artifact's validated
path; ordinary arguments remain literal. Pass argv arrays, never shell templates.
Data-only preparation never modifies global PATH or installs into a system package directory.

Preparation is part of explicit installation/reinstallation. Enable and ordinary
configuration changes read caches only; a changed hook plan not already cached
reports that installation must be retried. Matching verified cache entries are
reused offline. The cache key covers version, platform, checksum and extraction
format and any installer definition. Download-only cache identities remain compatible.
OS shared locks pin active service/process files. Immutable receipts
accumulate resolved plans for retained package versions, including alternate
workspace/configuration plans, until uninstall removes installation pins.
Garbage collection skips receipt pins and active leases from other windows.

Cancellation stops preparation and prevents candidate publication; it does not
roll back arbitrary native side effects. Waiting for another preparation lock
is cancellable. HTTP producers never write cache files and are bounded to four
workers; a cancelled stalled network read expires within 120 seconds without
delaying the caller. Installation progress distinguishes download/read, SHA-256
verification, unpacking and plugin installation. LSP startup and readiness are
reported separately by the actual protocol client; installed does not mean ready.

HTTP behavior follows the [ureq configuration contract](https://docs.rs/ureq/3.4.2/ureq/config/struct.ConfigBuilder.html).

## Authorized native installation

An artifact may add an `installer` and the package must separately declare and
receive `dependencies.install`. Neither `dependencies.prepare`, fixed service
startup, nor `process.exec` grants this authority. WASM-returned plans face the
same checks as static plans. A new permission on update requires fresh consent;
refusal or failed preparation leaves the previous version available.

```json
{
  "program": "setup.exe",
  "args": ["--output", "${target}", "--source", "${source}"],
  "target": "installed",
  "purpose": "Prepare the private analysis service",
  "kind": "service"
}
```

`program` and `target` are non-escaping paths relative to this artifact. The native
program comes from its verified bytes; no implicit shell or ambient program lookup
is used. Only whole `${target}` and `${source}` arguments expand. The executable
in the enclosing plan points at the produced service, for example
`server/installed/server.exe`. The installer must produce relocatable output:
staging is renamed into the immutable version directory after completion.

The host displays the actual program, argv, private target and purpose, and awaits
one-use authorization before execution. Set `kind` to `project_sdk` for compilers
or large project SDKs: their source is not read or downloaded until the user
actively selects preparation. After verification a second prompt authorizes the
concrete native execution. Existing complete cache entries need no re-execution.
SDK classification is a package declaration, not an inferred security sandbox.

Native programs retain the current user's OS file and network authority. A private
target is **not** an OS sandbox. Cancellation terminates managed processes and
cleans staging, but cannot undo external effects already performed by a program.
Windows execution waits for the entire owned job to become empty before cleaning
or publishing; Windows is the currently verified native platform.

The headless `Manager::install` path fails closed when concrete authorization is
needed. Interactive hosts create `InstallControl::with_installer_prompts`, poll
`installer_prompt`, and call `approve_installer(id, include_project_sdk)` only in
response to the user's displayed choice. Permission grants still pass through
`install_with_control`; a prompt is not a substitute for them. Stale or duplicate
approvals are rejected, and SDK opt-in is scoped to that request. Cancel the token
to refuse or close the prompt. Consent expires after 15 minutes; native execution
has a 10-minute deadline. Reinstallation is the explicit retry operation.

Installers are noninteractive (stdin is closed); stdout/stderr are drained without
unbounded buffering. Nonzero exit, cancellation, timeout, or missing/escaping
service output prevents the completion marker and cache publication. Successful
installation still uses the ordinary LSP readiness protocol, not installer exit,
to report a ready language service.
