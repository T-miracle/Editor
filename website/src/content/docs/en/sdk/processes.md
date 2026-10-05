---
title: Native processes
description: Declared services, authorized execution, transports, process ownership and resource limits.
section: sdk
order: 3
alternate: /zh-cn/sdk/processes/
---

# Native process capability 1.5

The `process: ^1` capability is negotiated independently. Operations are sent through the
ordinary SDK request as `api::Operation::Process { operation }`; a capability is never
decided by the plugin's name.

## Declaration and authorization

```json
{
  "services": { "analysis": { "program": "analysis-server", "args": ["--stdio"] } },
  "permissions": ["process.service.analysis"]
}
```

`StartService { service: "analysis" }` may only pass a declared ID. It cannot override the
program, append arguments, change the environment or working directory, or insert an
installation step. Each service is authorized separately, and the install dialog shows the
program and its argument array. An update that adds a permission requires confirmation; if
the user refuses, the old package, its existing authorization and its running instances stay
usable.

`Execute { program, args, transport, cwd, env }` separately requires **`process.exec`** and allows
an explicit program choice, including an interpreter. It cannot be obtained from an ordinary
service permission. No native installer operation exists today.

`cwd` defaults to empty; setting it explicitly requires negotiating `process >= 1.2` and must
be an existing absolute directory of at most 4096 bytes without NUL. It only chooses the
start directory of an already-authorized arbitrary process: it grants no WASM filesystem
access and cannot be used to override a `StartService` declaration. Omitting it keeps the
instance's default directory.

The generic toolchain resolver accepts an absolute executable path, or looks up a bare tool
name in the absolute directories of `PATH`. It never searches the current project implicitly
and never assembles a shell command. On Windows only `.exe` is started. A workspace instance
uses its trusted project as the working directory; an application instance or an instance
without a project uses its own private data directory.

A native program accesses files and the network with the current user's authority; the WASM
sandbox does not constrain its internal behaviour. Revoking workspace trust stops the
workspace instance and its process trees. Project configuration cannot grant these
permissions.

## Transports, results and resource ownership

- A declared service always uses **Stdio**. `stdout` and `stderr` are independent byte
  streams suited to LSP and JSON-RPC, with no terminal escapes and no line conversion.
- An explicit execution chooses between Stdio and
  **Pty { columns, rows, inherit_cursor }**. A PTY merges output, which may contain terminal
  escapes; the host does not interpret the terminal picture.
- `inherit_cursor` defaults to `false` and is omitted when serialized, preserving the
  behaviour of older capability consumers. Setting it to `true` requires `process >= 1.3` and
  is supported on Windows only; other platforms return `UnsupportedOperation` before the
  process is created. Windows uses the standard `PSEUDOCONSOLE_INHERIT_CURSOR`, handing the
  cursor query to the byte-stream consumer unchanged so the plugin can answer it
  asynchronously through `Write`. The host does not parse prompts, snapshots or cursor
  replies, and never enables this based on the plugin ID. The protocol basis is
  [CreatePseudoConsole](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole).
- Starting returns a `Value::Resource` whose handle is bound to the instance and its scope.
  Other plugins, destroyed instances and handles held after the process exits cannot use it.
- `Write` returning `Unit` means the input entered a bounded queue; it does not mean the
  program consumed or executed it.
- `Resize` applies to PTYs only. It bounds the size and coalesces consecutive adjustments
  within 150 ms; a standard pipe cannot be resized. On Windows the native resize runs on a
  dedicated worker that keeps at most one pending latest size, and the return value means the
  request was accepted. A plugin that never answers a cursor query still cannot block the
  host's input, polling or termination; a later poll reports the native resize error. On
  retirement or a failed start the host first terminates asynchronously and confirms that
  every process in the job has exited, and only then sends a fixed neutral cursor reply to an
  OS pipe that requested a handshake, so the wait can end; that cleanup reply is never handed
  to a live program and never reads the plugin's terminal state. After the pending native
  resize finishes, the console is closed exactly once, reclaiming its pipes and job.
- `Notification::Process { handle, update }` delivers output tagged with its stream and the
  numeric exit code. Exited is sent only after EOF is read and output is drained, so a
  temporarily empty queue cannot lose the last large block of output.
- `Terminate` and `CloseResource` release the handle and ask the OS to terminate the process
  tree, returning `Value::Process(Update::Terminated)` and the generic `Unit`. That result
  means the resource is closed and termination was requested; it does not mean a program's
  file or network changes were rolled back. A handle the plugin closed itself receives no
  further notifications.
- When the origin of a service call exits, the host reclaims delegated processes and sends
  one cleanup `Terminated` notification to the still-active provider. That notification keeps
  the revoked origin permissions: it allows updating local UI and does not allow starting new
  operations or reading private data from the callback. If the provider itself is also
  exiting, no callback is made. The detailed origin rules are in
  [Plugin services](/en/sdk/services/).
- Disabling, uninstalling, replacing or destroying the owner also cleans up resources.
  Windows uses a kill-on-close job: a PTY is added to the job atomically at creation, and a
  pipe process starts suspended and is resumed only after joining the job, so descendants
  cannot be spawned early. macOS and Linux remain compile targets; the process-tree guarantee
  and native acceptance for this round target Windows.

Each instance holds at most 32 processes and 128 total resource handles. A single input is at
most 1 MiB and each process queues at most 8 of them; output blocks are 8 KiB with a queue
capacity of 64, applying backpressure to bound the backlog with a bounded read per turn.
Insufficient permission, an unnegotiated capability, an invalid handle, an inactive instance,
an exceeded request limit and native I/O failure are all returned as unified typed errors.
## Environment and normal exit

process 1.4 adds `Execute.env`, a caller-supplied environment overlay: at most 64 entries, names of 128 bytes and values of 32768 bytes. Names must be nonempty and contain neither NUL nor `=`; values must not contain NUL. The child inherits the native environment and receives these explicit overrides. The host neither logs values nor grants access to another instance's environment or process. Declared StartService settings cannot be replaced by this overlay.

process 1.5 adds `RequestExit { handle, mode }`. Graceful PTY exit sends an interrupt byte; a universal graceful protocol for Stdio is unavailable and returns UnsupportedOperation. Force requests termination of only the handle's owned process tree. Both retain observation until an actual terminal native event; accepting a request does not mean exit. Callers choose a bounded grace period and may escalate explicitly. The host session service uses 3000 ms by default.

Normal `Update::Exited { code }` preserves the full unsigned 32-bit native exit code, including Windows interrupt codes. Output is drained through EOF before exit is published. Forced termination is a distinct `Terminated` event, without a fake exit code. A closed resource cannot be reused by the guest; the host's cleanup observer independently waits for the original owned tree and EOF. Cleanup failures are reported, never converted into an invented successful exit.
