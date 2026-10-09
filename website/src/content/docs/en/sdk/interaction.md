---
title: Native interactions and selected resources
description: Confirmed native input, cancellation, attributed messages and restricted file selection.
section: sdk
order: 18
alternate: /zh-cn/sdk/interaction/
---

# Native interactions

Negotiate `ui.interaction ^1` and approve `ui.interaction` for quick pick, input, confirmation, notices and progress. File, directory and save-location selection instead require `files.selection ^1` and the approved `files.select` permission. These operations use `api::EditorOperation::Interaction`, in an active trusted workspace instance.

`ui.interaction` and `files.selection` **1.0.0 are stable**, not experimental. The minimum host
supports the current `protocol = 7`, `api.base = ^1` and the relevant capability's `^1` range.
Required capabilities that cannot be negotiated reject the package; absent optional capabilities
must disable the corresponding feature or use a plugin fallback. The shared
[stability, compatibility and deprecation policy](/en/sdk/#stability-and-capability-compatibility)
applies to both interfaces.

Native file, directory and save-location pickers currently support Windows. On other platforms,
`Select` returns `UnsupportedOperation`, even when `files.selection` was negotiated; handle this
failure separately from a user's cancellation.

`interaction::start(operation, timeout_ms)` returns an owned accepted handle. The result arrives in `Notification::Request` as `RequestUpdate<EditorValue>`; successful values are wrapped in `EditorValue::Interaction`. `api::guest::EditorTask` correlates that handle and ignores other tasks and repeated terminal updates. Acceptance means queued work. It does not mean that the user confirmed the operation.

## Confirmed values and messages

| Operation | Successful value | User action |
| --- | --- | --- |
| `QuickPick` | `Value::Picked(id)` | Filter labels/descriptions, choose a stable item ID |
| `Input` | `Value::Input(text)` | Confirm a bounded UTF-8 string; password mode masks presentation |
| `Confirm` | `Value::Confirmed` | Explicitly confirm the displayed message |
| `Notify` | `Value::Dismissed` | Dismiss a notice attributed to its plugin |
| `Progress` | `Value::Finished` | The owner calls `interaction::finish(handle)` |
| `Select` | `Value::Selected(resources)` | Confirm the system file, directory or save picker |

Escape, a cancel button, native dialog closure, timeout and owner retirement produce a terminal cancellation or failure. They never return an empty successful selection, false confirmation or a fabricated input string. A late result cannot replace that terminal state. Cancelling a typed command also closes native interactions still waiting under that command; it does not promise to undo an effect already performed.

Input and quick pick use the host's native input behavior, including IME composition. Modal prompts restore their previous focus when they own it. Notices and progress appear in the host message area with a plugin source and retain editor focus. They do not enter the host's persistent error history. At most one modal interaction is shown at a time in a window; accepted requests wait in order.

`interaction::update(handle, message, percent)` replaces a progress task's presentation; it allocates no second task. Percent is optional and ranges from 0 to 100. A cancellable task exposes a user cancel action. The owner must consume cancellation before continuing its own work and must handle a rejected late update or finish.

Titles are 1–256 UTF-8 bytes; messages are at most 4096 bytes. Quick pick accepts 1–512 items, unique IDs of 1–128 bytes, labels of 1–256 bytes and optional descriptions of at most 1024 bytes. Input budgets are 1–65536 bytes, and an initial value must fit that budget; placeholders hold at most 256 bytes. Each instance has at most 32 pending editor requests. Deadlines are 1–300000 milliseconds. Windows native pickers have a separate bound of four workers and 64 native paths; a selection result can grant at most 32 resources and validates the entire batch before issuing any handle.

## Restricted selection authority

`Select` includes a title, `SelectionMode::{File,Directory,Save}`, multiple-selection flag and optional suggested basename. Save mode is single-selection. A suggestion is at most 255 bytes and contains no slash, backslash, colon, NUL, `.` or `..`; it creates no access authority.

The host alone receives the native path. Each `SelectedResource` returned to the guest contains an opaque `handle`, informational `name` and `kind`. The selecting instance owns the exact target and permitted operation. A handle is temporary and must not be persisted or transferred.

| Selected kind | Current operation |
| --- | --- |
| File | `api::guest::read_file(&handle, "")` reads only that file |
| Directory | `read_file(&handle, "relative/path")` reads only a regular file inside that selected directory |
| Save | Retains an exact-target save intent; `ReadFile` is denied and `WriteFile` returns `UnsupportedOperation` until a safe host file transaction consumes the intent |

Selection does not approve arbitrary workspace or external writes. Directory traversal, absolute/drive/device paths, alternate data streams and links or junctions escaping the selected boundary are rejected. Idle grants do not lock the file against normal rename or atomic replacement; replacing the selected object invalidates the old handle rather than granting access to the new object.

`api::guest::close_resource(handle)` consumes the handle and releases the grant. Disable, uninstall, trust revocation and instance replacement revoke all remaining grants. A cancelled, expired or late picker callback creates no grant. Selection and selected-resource reads are forbidden in delegated service and cross-plugin command contexts, even if both plugins separately have `files.select`; authority cannot be transferred through a JSON value or reply.

A typed command invoked directly by the host may select, read and release its own resources if its signature requires `files.select` and installation approved that permission. The runtime records this host origin internally and admits only the target provider's first hop; a guest-supplied caller name such as `@host` does not establish it. Nested or cross-plugin calls remain denied, and a service method cannot declare `files.select` as a delegatable grant.

Use [typed commands and menus](/en/sdk/commands/) for reusable entry points. Treat menu paths as descriptive context and acquire file authority through the ordinary workspace or selection APIs.
