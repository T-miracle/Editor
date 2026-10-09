# Plugin services `plugin.services` 1.1

[简体中文](../../zh-cn/sdk/services.md)

A consumer declares a contract ID and a SemVer range; it does not reference a particular
provider plugin. A protocol 7 WASM package publishes contracts with
`plugin_services.provides` and declares dependencies with `plugin_services.requires`; both
need the required `plugin.services: ^1` capability. A caller additionally needs the
`services.call` permission approved at install time. This is independent of the `process`
service declaration that starts native programs.

Each contract contains named methods, and each method explicitly declares its `parameters`,
`result` and `permissions`. The schema supports null, boolean, bounded integers, strings with
a byte limit, arrays with an item limit and closed records. A record's `optional` lists only
the fields that may be omitted; unknown fields are rejected. A provider version must match
the dependency range, and the method structure and permissions a consumer requires must
match exactly; a provider may add further methods.

A consumer declares, for example:

```json
{
  "plugin_services": {
    "requires": {
      "example.echo": {
        "version": "^1",
        "optional": false,
        "methods": {
          "echo": {
            "parameters": {"type": "string", "max_bytes": 2048},
            "result": {"type": "string", "max_bytes": 4096},
            "permissions": []
          }
        }
      }
    }
  }
}
```

A provider places the same method under `provides.example.echo.methods` with a concrete
version such as `1.0.0`. Required dependencies are checked before activation: a missing one,
or several providers with no selection yet, rejects activation. An optional dependency lets
the plugin handle an unavailable result after it is enabled. The host retries startup as
dependencies become ready; a missing dependency with no progress, or a dependency cycle, ends
retrying and shows the reason.

## Discovery, selection and requests

When exactly one compatible provider exists in the same logical scope it is adopted
automatically; several providers must be chosen in the host's plugin settings. The user's
default choice and explicit project overrides are stored in host-managed configuration.
Application-level and workspace-level services are chosen separately and cannot implicitly
cross workspaces or borrow the currently active window; neither project files nor plugin
requests can choose a provider on the user's behalf.

A consumer calls `service::guest::open(contract)` for a non-persistable reference and then
`service::guest::Task::start(&reference, method, arguments, timeout_ms)` to issue a request.
An accepted request returns a task handle; the final result is delivered as
`Notification::Service(Request { handle, update })`, and `Task::update` correlates and
filters out stale results. `Task::cancel` reuses the shared cancellation interface. Release a
reference or task with `api::guest::close_resource` when it is no longer needed.

A provider receives `Notification::Service(Invoke(Invocation))` and returns
`Result<serde_json::Value, Failure>` through `Output.service_reply` inside a current, budgeted
WASM callback. The host validates arguments, results, request ownership and version, and
transfers only bounded values, never shared memory or internal host objects. Switching,
exiting or rebuilding a provider invalidates old references; switching back to the original
provider does not revive them.

Requests use the shared Accepted, Progress, Completed and Cancelled states. Deadlines range
from 1 to 300,000 ms. A cancellation before entering the provider callback reports
`NotExecuted`; once execution has begun only `WaitingStopped` can be reported, with no
promise of rollback. WASM callbacks additionally respect the host's existing fuel and memory
budgets, and a request that times out while queued never executes. Each instance holds at
most 32 in-flight service requests, the host queue holds at most 256, and each poll routes at
most 32. Arguments and results are each at most 64 KiB; declarations are at most 256 KiB with
at most 32 contracts, 32 methods per contract, schema depth 8, 32 record fields and 1024
array items.

## Origin permissions and lifecycle

The plugin, instance, scope and permissions in `Invocation.caller` are produced by the host.
A consumer and a provider must both hold the permissions listed by the method, and each hop
can only narrow them further. Version 1.1 supports delegating `workspace.read`,
`editor.read`, `editor.write`, `ui.panels` and `process.exec`; a delegated permission that was
not declared is always rejected.

A provider may read its own immutable package resources, but it cannot use a service call to
read the caller's private data or to operate a private process the caller created earlier.
Asynchronous editor requests, process events and nested service results inherit the original
origin and the narrowed authority; the provider's full permissions are not restored when the
callback runs. Resources created by this delegation may be released or cancelled explicitly.
When any instance in the origin chain exits, the host seals its unfinished operations,
reclaims delegated resources and stops delivering late callbacks; external effects that
already happened are not treated as rolled back.

A process reclaimed because its origin was revoked receives one additional
`Process/Terminated` notification so the provider can update its local session UI. That final
cleanup notification still carries the revoked origin permissions: it cannot start new
operations or read provider private data, and it is not delivered once the provider itself is
disabled. A repeated instance in the chain is rejected immediately, and a chain holds at most
8 instances; nested calls enter the next queue round instead of recursing into the WASM store.

## Deferred provider replies

With plugin.services 1.1, an invocation supplies an optional provider-owned `reply` handle. A provider can return its immediate result through `Output.service_reply`, or retain that handle and complete it later through `service::guest::reply(&handle, result)` from an authorized native event or nested-service callback. No immediate reply means the invocation is deferred, not completed.

At most 32 deferred invocations may remain per provider instance. Each retains the original caller, narrowed permissions, exact result schema and original deadline; deferral does not renew a timeout. The reply handle belongs only to that provider invocation and is single-use. Another origin cannot finish it, a duplicate or retired reply is rejected, and invalid result shape or size is rejected before completion. Closing the handle abandons the reply. Cancellation, timeout, caller/provider retirement and scope changes release the slot and reject late replies. Providers must revoke delegated work; completing a reply does not transfer its native resources to a new origin.

Current service callbacks require plugin.services 1.1. Consumers and providers use the same SDK contract; installation of an old callback implementation does not add a legacy execution branch.
