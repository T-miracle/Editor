---
title: Run sessions
description: Run sessions contract and lifecycle.
section: sdk
order: 9
alternate: /zh-cn/sdk/sessions/
---

# Run sessions — interactive.execute 2.0 and session.host 2.0

Nanobug includes a native `interactive.execute` 2.2 provider. No terminal package is required.
The built-in provider is the default when no explicit user or workspace selection exists; an
explicit compatible choice still wins and does not retarget sessions already created. Plugins
use `session.host` to create visible task tabs and source-owned subscriptions in the unified
terminal panel. Direct provider calls do not create a host session record.

Version 2.1 additionally offers `resize(session, columns, rows)` with `process.exec` authority.
Columns are 2–1000 and rows are 1–500; the result is `session, state`. Consumers require
`execution::resize_method()` only when they use geometry. The six required 2.0 methods retain
their exact schemas, so existing 2.0 consumers remain compatible. Native execution retains
at most 64 session records, evicting only ended histories.

Version 2.2 also offers `execute_terminal`, using the execute fields plus a required
`inherit_cursor` boolean and the same creation result. Declare
`execution::terminal_execute_method()` to use it. A Windows VT view passes true and answers
ConPTY's initial cursor query through input so later preparation steps continue after prior output.
Headless consumers use execute; Unix views pass false. Resource permissions and ownership are identical.

Run services use [plugin.services](/en/sdk/services/). A provider publishes the exact method schemas, permissions and versions; consumers declare the methods they require. The host chooses by compatible contract and logical scope. A provider's name never supplies authority.

## Interactive execution

The public SDK's `execution::observation_methods()` declares the canonical input, locate and events methods;
`execution::resize_method()` and `execution::terminal_execute_method()` declare the optional extensions.

| Method | Parameters | Result | Permissions |
| --- | --- | --- | --- |
| execute | program, args; optional cwd, name, env | session, state | process.exec, ui.panels |
| stop | session; optional mode | session, state | process.exec |
| status | session | session, state; optional code | process.exec |
| input | session, bytes | session, state | process.exec |
| locate | session | session, state | ui.panels |
| events | session, after, limit | session, events, cursor, gap | process.exec |

`program` is an executable and `args` is a literal array: at most 128 arguments of 4096 UTF-8 bytes each. The host does not assemble a shell command. Environment overrides hold at most 64 names of 128 bytes and values of 32768 bytes. Values are passed to the child and are not logged. Input holds at most 1024 integer bytes in 0–255.

A request accepted into a queue, a process created by the provider and a process observed to exit are separate states. Unknown provider states are errors. A normal exit may report the full native unsigned 32-bit exit code; an absent code does not mean zero. Forced termination reports terminated without a sentinel exit code.

Providers validate the authenticated caller instance for every session operation. `locate` restores the provider's presentation of that same session. Hiding a managed view keeps its program, input and output alive; closing an ordinary provider-created shell may have different semantics.

## Incremental events

Output events contain sequence, kind=output, stream (stdout, stderr or pty) and at most 512 raw bytes. State events contain sequence, kind=state, state and an optional code. Consumers decode UTF-8 and ANSI across chunks.

`execution::EventBuffer` retains at most 128 events. A request reads 1–16 events after a cursor; the returned cursor is the last delivered sequence and stays unchanged for an empty batch. `gap=true` explicitly reports evicted history. Future cursors and unordered events cannot be presented as complete output. The native provider allows 32 active processes and retains up to 64 session records, reclaiming completed history rather than evicting active programs. Reclaimed output or presentation returns InvalidHandle.

## Host-owned sessions

The SDK's `execution::host_dependency()` declares `session.host ^2`. Calls create the same records used by the editor's run controls.

| Method | Parameters | Result | Origin permissions |
| --- | --- | --- | --- |
| start | interactive execute fields; optional configuration | session, state, located | process.exec, ui.panels |
| list | empty record | sessions, each with session and state | none additional |
| status | session | session, state | none additional |
| stop | session; optional mode, grace_ms | session, state | process.exec |
| input | session, bytes | session, state | process.exec |
| locate | session | session, state | ui.panels |
| subscribe | session | subscription, session, state | none additional |
| next | subscription, limit (1–16) | subscription, session, state, events, cursor, gap | none additional |
| unsubscribe | subscription | subscription | none additional |

All consumers still negotiate plugin.services and hold services.call. Only the originating live instance in the original workspace can access its records and subscriptions; other plugins cannot list, read, stop or borrow them. The editor window can manage all sessions belonging to its workspace. Delegation retains original permissions and lifetimes.

States are starting, running, stopping, terminating, exited and failed. Stop defaults to graceful with a 3000 ms grace period; grace_ms may be 1–60000. Unsupported graceful exit or expiry escalates to force. Explicit force acts immediately on the owned process tree. A stop acknowledgement is not final exit: actual native completion is required. Failure to confirm forced cleanup is reported as failure, not success. Repeated stop of completed sessions is idempotent. InvalidHandle from a provider status read seals the record as failed; transient errors never imply successful exit.

An optional configuration identity holds 1–256 printable bytes. An active identical configuration from the same origin and workspace is located even if its command was edited; different configurations with identical commands remain independent. Without this identity, literal commands are deduplicated. At most 64 records are retained and only completed or failed history is evicted. Provider selection changes do not reroute existing sessions.

Subscriptions pull retained provider history. Each origin may own 32 subscriptions and the host 128; one pull may be pending per subscription. `next` completes only after the actual provider reply. Cancelling a pull does not advance its cursor. Unsubscribe frees the wait and cursor without stopping the program. Provider/history retirement, origin retirement, workspace change and window close revoke subscriptions. IDs never regain authority on reuse.

Version 2.0 requires public input, locating and observation, explicit origin permissions, normal/forced stop and unsigned exit codes. Old ^1 consumers and providers must update; there is no compatibility path that silently drops these requirements.
