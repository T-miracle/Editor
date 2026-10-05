---
title: Run, debug and build
description: Configure, prepare, run and inspect native program sessions.
section: guide
order: 7
alternate: /zh-cn/guide/run-debug-build/
---

# Run, debug and build

The title bar groups configuration selection, Build, Run, Debug, Stop, Locate and Rerun beside plugin management. A disabled control explains the missing configuration, unavailable capability, restricted workspace or current lifecycle state.

## Choose a configuration

Open the configuration menu to create or edit a configuration. Its Basic, Build, Debug and Environment pages share one draft. Saving stores the draft without executing anything; Cancel discards changes.

Program mode takes an executable and one literal argument per line. Spaces, quotes and punctuation inside an argument remain part of that argument. Shell mode takes an explicitly selected interpreter, its arguments and multiline script text. The editor passes the script as the final interpreter argument and does not guess a shell or command flags. A missing interpreter or invalid environment entry keeps the form open with a reason.

The Environment page accepts NAME=value rows and tool-path overrides. Values apply only to the launched program/preparation. Tool paths are machine-local; shared configuration cannot grant trust or plugin permissions. Native fields support Chinese composition, selection and multiline input.

## Discover and share

Discover targets asks enabled providers for candidates. It does not run a candidate or save it automatically. Confirm a candidate to create a configuration; repeated confirmation reuses it. Your edited names, arguments, environment and ordered preparation steps remain intact. If its provider or target disappears, repair the configuration explicitly instead of running a different target.

Configurations can stay local or be stored in the project's shared configuration file. Shared data is portable and omits machine paths, permissions, credentials and running sessions. Opening a copy in another workspace gives it local identities and resolves its provider binding there. External invalid/deleted shared data is reported; the editor does not silently launch a stale fallback.

## Build and launch

Build executes ordered build steps and stops before running the final program. Run first saves modified documents through the ordinary save path, then executes build and prelaunch steps and starts the final target once. A disk conflict or failed save prevents launch. A failed preparation identifies its step and output and does not start the program.

A provider may prepare an exact build artifact for the selected target and profile. For example, debug and release configurations remain separate; Build does not substitute an unrelated executable. The attempt snapshots the effective configuration, so later edits do not change a running attempt.

Independent configurations can run together. Selecting a session only changes which session a control addresses. Hiding its output preserves the program; Locate restores the same session. Stop asks for normal cleanup and escalates after a bounded grace period; Force stops the selected owned tree immediately. The stopping display lasts until actual native completion. Rerun waits for the previous attempt to finish cleanup before preparing again.

## Debug inspection

Debug requires a compatible enabled provider, a usable configuration and applicable workspace authority. The native debug panel shows session selection, bound/unbound breakpoints, source location, call stack and local variables. Controls unsupported by the provider remain disabled with a reason.

A real pause opens the reported source position. Selecting another reported frame requests that frame's locals. Continue and stepping invalidate the previous pause's inspection; late results cannot overwrite a new pause or a different session. In the focused debug panel, F5 continues, Shift+F5 stops, F6 pauses, F10 steps over, F11 steps into, Shift+F11 steps out, and Up/Down selects a frame.

Hiding the panel keeps debugging active. Stop reclaims the target and adapter; Force applies only to that session. Before disabling, uninstalling or updating an affected plugin, or leaving the workspace/window, the editor lists affected work. Cancel preserves the plugin and sessions. Confirmed retirement cleans them up and never automatically reruns a user command.

Plugin authors can implement [run sessions](/en/sdk/sessions/), [debug sessions](/en/sdk/debug/) or [target discovery and preparation](/en/sdk/targets/) with the public SDK.
