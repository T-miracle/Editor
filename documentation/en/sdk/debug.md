# Debug sessions — debug.session 1.1

[简体中文](../../zh-cn/sdk/debug.md)

A debug provider publishes the canonical `debug::contract()` through [plugin services](services.md). The host selects a compatible provider in the current scope. Debug adapters and their transport belong to the provider; the host does not select by plugin, language or adapter name.

## Methods and permissions

All methods require process.exec; start additionally requires ui.panels.

| Method | Parameters | Result |
| --- | --- | --- |
| start | program, args; optional cwd, name, env, breakpoints, stop_on_entry | session, state, pause |
| set_breakpoints | session, breakpoints | breakpoints with source, line, verified |
| resume | session, pause | session, state, pause |
| pause | session | session, state, pause |
| status | session | session, state, pause; optional reason, source, line |
| stop | session | session, state, pause |
| step (optional) | session, pause, kind (into/over/out) | session, state, pause |
| frames (optional) | session, pause | frames with id, name, source, line |
| variables (optional) | session, pause, frame | variables with name, value |

Start uses a literal executable and argument array with the same 128 × 4096-byte argument and 64-entry environment limits as [run sessions](sessions.md). A session ID holds at most 128 UTF-8 bytes. States are connecting, running, paused, exited and failed. A creation or control acknowledgement is not evidence of a stopped target or completed cleanup.

Resume, pause, status, stop and breakpoint setting are required. Step is optional. Inspection is available only when both frames and variables are declared. Missing optional methods disable only their corresponding controls with a reason.

## Pause identity and inspection

Version 1.1 adds the required nonnegative pause generation to status, start and control results, and to resume, step, frames and variables arguments. It is bounded by signed 64-bit integer range. Each observed new pause advances the generation; operations must use the exact currently paused session and generation. A resumed, stopped or newly paused target invalidates old frames, variables, selected frames and pending inspection. Stale requests return an explicit failure and cannot resume or step a different pause. A refused control keeps the existing paused inspection valid.

The provider reports real target state and inspection. Printed text is not a call stack or a variable value. Frame selection requests that frame's variables under the same generation. Frame IDs are 0–2147483647, at most 256 frames are returned, and each frame returns at most 512 variables. Names hold 512 bytes and values 4096 bytes; the complete service reply remains limited to 64 KiB. Providers must fail an oversized result instead of presenting a partial inspection as complete.

## Breakpoints and presentation

At most 512 breakpoints are accepted. Source paths hold 4096 bytes and lines are 1–2147483647. Results report verified per breakpoint; false or missing verification does not mean a bound breakpoint. Binding receipts belong to the configuration/session that requested them even if the user selects another session while waiting.

The native host panel presents session selection, breakpoint verification, current location, controls, frames and variables. It uses the provider's actual source location to open the editor and follows a new pause once. Hiding the panel preserves the session. Provider selection changes affect future starts and do not reroute old controls. The host never treats elapsed time, program output or its own guessed location as a pause.

## Cleanup and recovery

The provider owns its target, adapter and delegated native resources. Stopping a debug session requests provider cleanup; the host has a bounded wait and can force only that session's owned tree. Provider disable, uninstall, replacement, failure and workspace/window retirement revoke the corresponding roots and inspection without affecting unrelated sessions. Late replies do not revive an old session or repeat user commands.

An update is prepared transactionally: preparation failure keeps the old instance; failed activation restores its logical state and managed private data. Already ended process memory and external side effects cannot be restored. Switching back or recovery never silently launches a target.

1.0 providers must update their declaration and pause validation to 1.1. A method-name match without the exact compatible parameter, result and permission shapes does not count as availability.
