---
title: Configuration templates
description: Plugin-owned native forms and validation for local run configurations.
section: sdk
order: 12
alternate: /zh-cn/sdk/configurations/
---

# Configuration templates — run.configurations 1.0

The editor also owns two **Nanobug** templates, **Plugin Packaging** and **Plugin Development**, which remain available without plugin providers. They reuse native forms and local draft storage but execute the host's shared project builder. Their project/output/workspace fields follow the [packaging format](/en/sdk/packaging/); they do not change the public provider contract below.

A compatible plugin can contribute command templates and its own native form through `configurations::declaration()`. Publish that exact contract in the manifest's services, negotiate `plugin.services >=1.1,<2`, and declare `ui.native ^1` and the native capabilities used by the form. Identity is authenticated by service routing; a payload cannot choose a different provider. The host supplies configuration identity, the tree, local storage and request lifecycle. The plugin supplies executable policy, editable defaults, layout and business validation.

## Methods and envelopes

All calls contain `workspace`, `locale` and `os` strings (4096, 64 and 64 bytes). Replies are records containing a JSON `payload` string, at most 64 KiB, decoded into the public Rust types. `catalog` and `validate` require `workspace.read` and `process.exec`, so providers can resolve available tools through permission-checked host calls. `form` requires `workspace.read` and `ui.panels`. A manifest declaration is not a permission grant. Restricted workspaces cannot invoke these services or execute configurations.

`catalog` returns `Catalog { templates }`, at most 128 templates per provider and in the combined catalog. Templates contain `id`, `group`, `label`, `icon`, `defaults` and optional `unavailable`. Id, group and label are bounded to 256 bytes; id and label must be nonempty, and ids must be unique within a provider. Defaults are provider-owned JSON, at most 16 KiB. Icons are stock `code`, `terminal`, `play`, `build`, `debug`, or self-contained SVG up to 16 KiB; external assets and executable foreign content are rejected. Unavailable ordinary commands remain visible with a reason. Providers can omit environment-specific commands, such as unavailable Shell interpreters. Selecting a template instantiates an independent draft, without executing a program or saving it.

`form` also receives `template`, `values` and `event`. Template is bounded to 256 bytes; values to 16 KiB; event to 64 KiB. An empty event initializes the form. Otherwise it is a serialized native `ui::UiEvent`. Return `Form { values, name, program, document }` with canonical provider-owned JSON values and a complete native `ui::Document`. Name is at most 256 bytes; program is nonempty and at most 4096 bytes. The provider renders the name, read-only executable and editable full argument list using native nodes; it may render additional fields or a different layout. The host does not infer tool-specific values from field ids. Configuration forms cannot claim editor-source, toolbar, viewport or image-input capabilities.

Events are ordered per configuration. Decode `event` as `configurations::FormEvent`: native events retain the native UI wire shape; `{ "configuration_name": "..." }` requests the provider to rename copied values. The host never guesses a JSON field or input node. Retain stable node ids and input `value_revision` for ordinary echoes: increment an input's value revision only for an intentional reset or replacement, so native input, focus and IME composition survive form updates. Document revisions follow the public [native UI contract](/en/sdk/ui/).

`validate` receives `template`, `values` and `intent` (`save`, `run`, `build`, `debug`, at most 64 bytes), and returns `Validation { valid, message, launch }`. The provider can prepare a build action or debuggable artifact for the same editable command without host tool-specific branches. Valid receipts require structured launch data. Invalid receipts carry an actionable reason and no launch. Service failures, malformed receipts, deadline expiry or retired providers are unavailable results, never permission to reuse an old launch. A save waits for validation and may preserve invalid configurations for later editing; execution requires a fresh receipt for unchanged values in the same live workspace/provider incarnation. Stop retains independent ownership of existing sessions.

Catalog, form and validation requests expire after 30 seconds. The host accepts at most four concurrent catalogs and 128 concurrent form/validation calls. Native documents also pass the negotiated capability checks and drawing/event budgets. Replacing or disabling a provider, closing a form or retiring a workspace cancels its requests; late replies cannot change replacement drafts or authorize execution.

## Structured launch

The optional guest helper `configurations::command_form::Fields` composes a name field,
read-only executable, one native input per literal argument and collapsed working-directory
and environment fields. An optional script uses `ui.native >=1.1` multiline input. Plugins
own its serialized values and validation and may replace the helper with another native
layout. `command_form::resolve` uses public `process >=1.6` tool lookup; the helper never
starts a process while adding or editing a form.

In `Launch.target` and action targets inside `build`/`prelaunch`, `provider: "$self"` refers to the
authenticated configuration provider. The host replaces this token with that provider's
identity before validating the projection. It does not select an execution provider or
grant target, process or debug permissions. This allows the same guest package to retain
its own target preparation after a package identity change.

An unsuccessful form call does not acknowledge its event. The host retains pending events with the local draft (at most 512 events and 64 KiB combined), stops automatic retries after failure, and may save them as unavailable. Reopening replays them in order before validation. Canonical values alone cannot authorize execution while input remains unacknowledged. Input exceeding the storage budget keeps the window open with an explicit error. Deferred native events retain their window and provider incarnation; replacement surfaces cannot adopt old callbacks.

`Launch` contains `target`, optional `directory`, `env`, `tool_paths`, ordered `build` and `prelaunch` actions, and optional execution `provider`. A program target is `{ "mode": "program", "program": "cargo", "args": ["run", "--release"] }`. The subcommand is part of the editable full argument array. Spaces, quotes, Chinese text and metacharacters stay inside their original argument elements; the host never joins them into a Shell command.

A script target is `{ "mode": "script", "interpreter": "...", "args": [], "script": "..." }` and uses the interpreter supplied by the plugin. A provided target is `{ "mode": "provided", "provider": "...", "binding": "...", "label": "...", "args": [] }`, prepared through [run targets](/en/sdk/targets/). Each preparation action is `{ "name": "...", "target": { "kind": "action", "target": PROGRAM_TARGET } }`; an explicit build reference uses `{ "kind": "build", "config": "CONFIGURATION_ID" }` instead. These values pass the existing configuration, path, tool and session boundaries; they cannot grant permissions.

Build-only executes build actions and never the final target. Run and Debug consume the same validated projection through the public [session](/en/sdk/sessions/) and [debug](/en/sdk/debug/) contracts. Never hardcode a host branch for a package, language, executable or command template.

Configurations and their tree are stored locally per workspace. Provider-owned JSON is preserved without field migration by the host. Package installation, private plugin data, permissions and running process memory are separate from configuration storage.
