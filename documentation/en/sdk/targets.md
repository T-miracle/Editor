# Run target providers — run.targets 1.0

[简体中文](../../zh-cn/sdk/targets.md)

Discovery is an explicit user action and never starts a discovered program. Language and tool packages contribute candidates through the canonical `targets::declaration()` service contract. The host consumes portable bindings without interpreting a language's project format, package names, build output or adapter protocol.

## Discovery and identity

`discover { workspace }` returns `{ targets }` and requires workspace.read and process.exec. A provider may run controlled project-discovery tools within its authenticated trusted workspace; it must not execute the candidate program. Each candidate contains identity, label, source, target_type, type_version and binding.

Identity and label are at most 256 bytes, source and binding 4096 bytes, and target_type 128 bytes. type_version is 1–2147483647. One reply and the combined host catalog each hold at most 128 targets; at most 16 dynamic providers are queried. The host namespaces identity with the authenticated provider; a raw candidate cannot impersonate another provider.

Binding is versioned portable JSON owned by the provider. It contains project-relative identity fields, not credentials, grants, machine-specific executable paths or process handles. Discovery candidates are temporary. Only explicit confirmation stores a run configuration, and confirming the same stored binding reuses that configuration. Discovery never overwrites edited names, arguments, environment or preparation steps.

A failing source does not discard healthy sources. Existing candidates from a failed source remain visible with a launch/build blocker until healthy rediscovery or explicit repair. A removed or incompatible target is not silently replaced with another executable.

## Preparation and artifact selection

`prepare { workspace, binding, env }` returns `{ program }` and requires workspace.read, process.exec and ui.panels. env has at most 64 names of 128 bytes and values of 32768 bytes; program is at most 4096 bytes. The provider validates the binding and produces the exact executable for that target. Build-only never runs it.

The host runs ordered build and prelaunch steps, then launches the prepared artifact once through [run sessions](sessions.md) or [debug sessions](debug.md). A configuration snapshot fixes the target, arguments, environment, working directory and effective provider for that attempt; changes do not mutate a running attempt. Failed preparation prevents launch and identifies the failed step with output.

Each preparation owns an authenticated resource root. Output decoding preserves UTF-8 across native chunks, and the host keeps at most 64 KiB of preparation output. Stop applies to that preparation and its delegated native tree; force escalation waits for actual process and output completion. Cancellation, retirement, deadline expiry and provider replacement revoke the root and never publish a usable stale artifact. Cleanup cannot replay a preparation or the final program.

## Independent providers and portability

A declarative target rule can provide ordinary programs where no dynamic build behavior is needed. Dynamic providers negotiate plugin.services and publish the exact run.targets method schemas through their own WASM component. Both paths use the same confirmation, configuration, preparation and validation rules.

Portable shared configurations preserve bindings and project-relative data. Machine-specific interpreter/tool paths, credentials, permissions and runtime session identities stay local. Import into another workspace establishes a new local configuration identity and requires live discovery or repair of its provider binding. There is no cross-workspace process, permission or session migration.

Use the editor's public SDK build entry point for independent packages. Do not depend on the editor repository's implementation crates or distribute a private SDK source copy.
