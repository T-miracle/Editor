---
title: Installing and managing plugins
description: How plugins are installed, what the permission prompt means, and what happens on enable, update and removal.
section: guide
order: 6
alternate: /zh-cn/guide/plugins/
---

# Installing and managing plugins

Plugins are what add languages, syntax highlighting, panels and tools to the editor. This page
covers the host behaviour: installing a package, approving what it may do, and controlling
where it runs. Each plugin documents its own features in the README that ships inside its
package, which the plugin manager shows on the overview tab.

## Installing a package

Open **Plugin management** and use **Install local ZIP…** to pick a plugin package from your
machine. Packages are the packaged form of a plugin: a manifest plus its resources, and a
WebAssembly component when the plugin needs behaviour that cannot be declared.

Nothing is installed before you approve it. The confirmation dialog shows where the package
came from, what it may do once installed, and the exact programs and arguments it declares.

## What the permission prompt means

A package declares the capabilities it needs, and you approve them as a set. Declarative
resource packages state that they need no runtime permissions at all. For an executable
plugin, the dialog lists what each declared permission grants:

| Permission | What approving it allows |
| --- | --- |
| Read package resources | Read resource files inside this plugin's own package |
| Read workspace files | Read files in the current workspace |
| Clipboard | Read and write the system clipboard |
| Private storage | Keep the plugin's own settings and session data |
| Run a declared service | Start the fixed native service the package declares, which then runs with your user account and can reach local files and the network |
| Run any program | Run arbitrary local programs, including an interactive terminal, with your user account |
| Prepare dependencies | Download, verify and unpack the service dependencies the package declares, into the editor's private directory. No installer script runs and the global `PATH` is not modified |
| Native installation steps | Show a second prompt with the actual program, arguments, target and purpose, and run it only after you confirm that specific plan |

Native programs run with your account's authority; the sandbox that constrains a plugin's own
WebAssembly does not constrain them. Approving a permission is a decision about a program, not
a formality.

## Enabling, disabling and scope

Each plugin has a global switch and a per-project switch:

- **Global On / Global Off** decides whether the plugin may run at all.
- **Enable in This Project** decides whether it runs for the workspace you have open.

A plugin you disabled globally stays disabled until you turn it back on; installing an update
does not silently re-enable it. The per-project switch is how you keep a heavy plugin — a
language server, a terminal — out of projects that do not need it.

## Restricted workspaces

A workspace the editor does not trust is restricted. In one, you can still edit files, but
plugins and language tools do not start and project-provided tool paths are not used. That
protects you from a repository that ships its own configuration. Trust is a property of the
workspace, and project configuration cannot grant it.

## Updating

Updating replaces the running version with a new one. If the new version declares a permission
the old one did not, you are asked again; if you refuse, the previous version keeps working.

The editor prepares the new version first. A preparation that fails leaves the old instance
running. Only when the new version is ready does it stop the old one, and if activation fails
it restores the old version and its private data. An update does not re-run the commands the
old instance had already been given.

## Removing a plugin

Uninstalling removes the plugin's panels, stops the processes it started, and releases the
resources it held. It asks for confirmation first, because removing a plugin can also remove
the language or panel you were relying on.

Settings and private data that belong to the plugin are removed with it. Data the plugin wrote
into your documents is not touched.

## When something goes wrong

The detail page has a **Runtime log** tab with this session's messages, and a **Restart plugin**
button at the top.

- A restart replaces the plugin's instance and reconnects it. It keeps the plugin's private
  files and its last safe checkpoint, so restarting is the first thing to try after a failure.
- The log tab shows what the plugin and its services reported, with severity, source and time.
  A coloured icon on the tab means there are unread warnings or errors.
- A plugin that exceeds its execution budget or crashes is retired and reported. The editor and
  your other plugins keep running.
- A language server that fails repeatedly stops being retried: after three failures the plugin
  needs an explicit restart, and the log says so.

Restarting or removing a plugin never modifies your documents, and never rolls back anything a
plugin already did outside the editor.
