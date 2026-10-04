---
title: Plugin contract
description: Capability negotiation, native UI, services, processes and packaging for plugin authors.
section: sdk
order: 0
alternate: /zh-cn/sdk/
---

# Plugin contract

This section is for people building plugin packages. A plugin is a declarative package,
optionally carrying a WebAssembly component for behaviour that cannot be declared.

The contract is versioned and negotiated: a manifest states which capabilities it needs
and which are optional, and the editor checks those declarations before an instance runs.
A plugin that requires a capability the editor cannot provide is rejected with a reason
rather than started in a degraded state.

## What the contract covers

- The package manifest and the capability declarations it carries.
- Typed requests and notifications between the guest component and the editor.
- The native UI tree a plugin publishes for its panel, including the canvas escape hatch.
- Cross-plugin services with an explicit version and provider selection.
- Native processes, including interactive ones, behind their own permissions.
- Language packages: grammar and query resources, and language servers.
- Private storage, configuration, data-format migration and fault recovery.
- Independent builds through the editor's own Cargo entry point, and SDK export.

## How the documentation is delivered

The contract documentation is also part of the SDK the editor hands to plugin projects:
the editor embeds it, caches it by content digest, and serves it to `cargo` when you build
a plugin. The pages in this section are the same text, published for reading here.

## Where to start

If you have never written a plugin for this editor, start from the package manifest and
capability declarations, then move to the UI or service chapter that matches what your
plugin needs. Every chapter states the version it applies to.
