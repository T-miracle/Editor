---
title: Run, debug and build
description: Configure, prepare, run and inspect native program sessions.
section: guide
order: 7
alternate: /zh-cn/guide/run-debug-build/
---

# Run, debug and build

The title bar shows a configuration dropdown and four icon buttons—Build, Run, Debug and Stop—beside plugin management. The dropdown opens directly below its button: saved configurations appear above a separator, and **Edit configurations** opens the configuration window below it. An empty list shows a muted **No configurations** placeholder. Selecting a configuration does not launch it. Hover an icon for its action or disabled reason. While an ordinary execution is stopping, the Stop slot offers immediate termination.

## Choose a configuration

Open **Edit configurations** to create or edit a configuration. Like plugin management, this is a separate native dialog above its parent window. Reopening activates the same dialog. It follows the externally selected configuration; without a selection the right pane shows **Please add a configuration**.

The plus button opens a drawer over the left list. Enabled compatible plugins supply grouped command templates with icons, names and defaults. Choose a template to create a separate unsaved configuration and open its plugin-owned native form. The program is read-only; its complete argument vector is editable, including a default subcommand. Argument boundaries preserve spaces, quotes and punctuation. The plugin supplies additional fields and business validation.

**Apply** validates and stores the selected configuration, retaining the dialog and other drafts. **Save** waits for edits and validates every configuration, stores the drafts and closes the dialog. Invalid or unavailable results remain editable and cannot run; a disk write error keeps the dialog open. **Cancel** discards unapplied changes and retains earlier successful Apply operations. Editing or saving never starts a program.

Configurations stay on this machine, scoped to the workspace. They do not create a project configuration file or grant plugin permissions. A provider that disappears produces an unavailable result; execution never silently switches to another provider or reuses an old validation receipt.

## Build and launch

The **Nanobug** group always offers **Plugin Packaging** and **Plugin Development**. Select either to create a draft. Packaging accepts one project directory per line and a manually entered or selected ZIP output directory. Leave output empty to use each project's shared default, normally its own root. Run builds and produces one ZIP per project; Build prepares artifacts without ZIPs.

Plugin Development accepts one project and an optional test workspace. Run confirms trust/permissions and opens an independent editor with isolated settings, plugins, private data and history. Shipped plugins remain available; the development version replaces a shipped plugin with the same ID. Data is retained for this configuration. Stop it before choosing **Reset development environment**. The output panel offers **Reload plugin**; automatic reload is optional. Failed reload retains the old version. Build prepares artifacts without opening a window; Debug is disabled because WASM source breakpoints are unavailable. See [packaging and development](/en/sdk/packaging/) for project declarations.

The Rust plugin offers a **Cargo** group with run, build and debug templates. Run defaults
to `run --release`, Build to `build --release`, and Debug to `run`; all these arguments remain
editable. Missing Cargo or a root Cargo project leaves these ordinary templates disabled
with a reason. Debug uses a real Cargo artifact, supports dev/debug and release profiles,
and asks for explicit `--package`/`--bin` when more than one binary applies.

The terminal plugin offers Shell templates for the current operating system and installed
interpreters only. Its read-only interpreter, editable arguments and multiline script belong
to the plugin. Working directory and environment overrides live under **More settings**.
Unsupported debugging is reported by the Shell provider. The simplified configuration
window has no local/shared selector. Older fixed-form configurations are not imported;
after an explicitly authorized cutover, create new configurations from plugin templates.

The left toolbar offers Add, Delete, Copy and Add folder as icons with tooltips. Folders are virtual groups. Add creates a child of the selected folder, a sibling of the selected configuration, or a root entry when nothing is selected. Click empty tree space to select the root. Rename a folder with F2 or a double click; click its disclosure arrow to expand or collapse it.

Drag rows into folders or onto empty tree space to move them to the root. The gap before a row changes order within its category; folders always precede configurations. Moving into a descendant is rejected. Copy creates an independent sibling with a copy name. Delete shows the recursive folder and configuration counts and changes only the draft. Save commits the deletion, while Cancel retains earlier applied data. X or Escape offers Save, Discard changes and Continue editing when unapplied changes remain.

Build executes ordered build steps and stops before running the final program. Run first saves modified documents through the ordinary save path, then executes build and prelaunch steps and starts the final target once. A disk conflict or failed save prevents launch. A failed preparation identifies its step and output and does not start the program.

A provider may prepare an exact build artifact for the selected target and profile. For example, debug and release configurations remain separate; Build does not substitute an unrelated executable. The attempt snapshots the effective configuration, so later edits do not change a running attempt.

Independent configurations can run together. Selecting a session only changes which session a control addresses. Hiding its output preserves the program; Locate restores the same session. Stop asks for normal cleanup and escalates after a bounded grace period; Force stops the selected owned tree immediately. The stopping display lasts until actual native completion. Rerun waits for the previous attempt to finish cleanup before preparing again.

## Debug inspection

Debug requires a compatible enabled provider, a usable configuration and applicable workspace authority. The native debug panel shows session selection, bound/unbound breakpoints, source location, call stack and local variables. Controls unsupported by the provider remain disabled with a reason.

A real pause opens the reported source position. Selecting another reported frame requests that frame's locals. Continue and stepping invalidate the previous pause's inspection; late results cannot overwrite a new pause or a different session. In the focused debug panel, F5 continues, Shift+F5 stops, F6 pauses, F10 steps over, F11 steps into, Shift+F11 steps out, and Up/Down selects a frame.

Hiding the panel keeps debugging active. Stop reclaims the target and adapter; Force applies only to that session. Before disabling, uninstalling or updating an affected plugin, or leaving the workspace/window, the editor lists affected work. Cancel preserves the plugin and sessions. Confirmed retirement cleans them up and never automatically reruns a user command.

Plugin authors can implement [run sessions](/en/sdk/sessions/), [debug sessions](/en/sdk/debug/) or [target discovery and preparation](/en/sdk/targets/) with the public SDK.
