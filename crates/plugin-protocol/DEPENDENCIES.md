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
Nothing modifies global PATH or installs into a system package directory.

Preparation is part of explicit installation/reinstallation. Enable and ordinary
configuration changes read caches only; a changed hook plan not already cached
reports that installation must be retried. Matching verified cache entries are
reused offline. The cache key covers version, platform, checksum and extraction
format. OS shared locks pin active service/process files. Immutable receipts
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
