---
title: Typed commands and native menus
description: Checked command parameters, results, cancellation and native target contributions.
section: sdk
order: 17
alternate: /zh-cn/sdk/commands/
---

# Typed commands and native menus

Negotiate `plugin.commands ^1` to discover or provide typed commands and native menu contributions. Calling another plugin also requires approved `commands.call`. A command without `signature` keeps existing one-way `Notification::Command` semantics and is not discoverable as a result-bearing command.

`plugin.commands` **1.0.0 is stable**, not experimental. Its minimum host supports the current
`protocol = 7`, `api.base = ^1` and `plugin.commands = ^1` negotiation. Packages providing typed
commands or native menus must declare `plugin.commands` in `api.required`. Consumer-only plugins
may use `api.optional` for discovery/invocation and disable those calls or fall back when the
capability is absent. The shared [stability, compatibility and deprecation policy](/en/sdk/#stability-and-capability-compatibility)
defines same-major additions and major-version migrations without promising a future host
release or wire protocol.

## Registration and calls

Manifest commands may declare a `service::Method` signature with `parameters`, `result` and `permissions`. Both installed caller and provider must authorize the signature; the provider declares those permissions in its manifest. Supported schemas are null, boolean, bounded integer, bounded UTF-8 string, bounded array and closed record. Unknown fields and malformed arguments are rejected before execution; malformed results return an explicit failure.

```json
{
  "id": "echo", "title": "Echo",
  "signature": {
    "parameters": { "type": "string", "max_bytes": 64 },
    "result": { "type": "string", "max_bytes": 64 },
    "permissions": []
  },
  "menus": [{ "location": "editor", "group": "tools", "order": 10, "arguments": "hello" }]
}
```

`commands::discover()` returns live `Descriptor { plugin, command, signature }` values in the caller's scope without executing a provider or granting permission. `commands::invoke(plugin, command, arguments, timeout_ms)` returns an accepted request handle. `commands::Task` correlates `Notification::CommandRequest` updates and cancels through the ordinary host cancellation API.

The provider receives `Notification::CommandInvocation(Invocation { id, arguments, context, caller, reply })`. The host authenticates the original caller and shrinking permission intersection; it never lends the provider's stronger private grants. Optional native `context` is descriptive and separate from the parameter schema. Return immediately through `Output.service_reply`, or retain the provider-owned `reply` handle and later call `commands::reply(reply, result)`. Empty output means deferred work, not successful null. `Notification::CommandCancelled` retires local bookkeeping; cancelled, expired, repeated and foreign replies are rejected.

Nested calls retain the original source. Cycles and depth above eight are refused. Replaced, retired or cross-workspace instances cannot receive old results. Selected-resource authority cannot be transferred through commands or services, including when both plugins have `files.select`.

Parameters and replies hold at most 64 KiB. Each instance has at most 32 pending requests and 32 deferred invocations. Deadlines are 1–300000 milliseconds. The first terminal result wins. Cancellation stops waiting and closes pending native interactions under that wait; it does not roll back an entered side effect or imply termination of an already owned program.

## Native contributions

Each command may declare at most 16 `menus`. `location` is `editor`, `selection`, `explorer` or `tab`; selection entries require an actual selection. `group` holds at most 64 bytes; `order` is a signed integer. Entries sort by group, order, plugin ID, command ID and contribution index. Groups are separated and labels identify the source plugin.

`when` controls visibility and `enabled_when` controls enabled state. These closed conditions optionally contain `has_selection`, `writable`, `directory`, `language` and `extension`; every specified field must match. An omitted condition is true. Language identifiers hold at most 128 bytes and extensions 32 bytes. The host reevaluates conditions on activation and retains the original target and instance incarnation.

Static `arguments` default to null and must fit the schema and 64 KiB budget. Separate native `Context` contains `has_selection`, `writable`, `directory`, `language`, `extension` and optional workspace-relative `path`. It describes the clicked target, which may differ from the active editor. Virtual resources do not become disk paths. Context grants no file access and replaces no version, selection grant or resource handle.

Disable, uninstall, fault, trust revocation and replacement remove unavailable contributions. Stale callbacks cannot execute against a new instance or a changed/closed document. Menus use the same admitted route as ordinary command entry points.

Use [native interactions and selected resources](/en/sdk/interaction/) for confirmed input, messages, progress and external-file selection.
