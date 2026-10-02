# Execution limits and recovery

The host applies the same limits to every plugin identity. Normal WASM calls have
100 million fuel units and a 1 second epoch deadline (10 ms clock ticks); snapshot
calls have 1 billion units and 10 seconds. These bound guest execution, not blocking
host I/O or native process CPU time. A component has an aggregate 256 MiB linear
memory budget, at most 32 memories/tables/instances and 100,000 elements per table.

Existing queues remain bounded: each instance may hold 32 editor requests and eight
document subscriptions; document notifications coalesce by version and close on
capacity exhaustion. The service broker accepts at most 256 queued invocations.
Quota failures return typed errors. Request deadlines remain between 1 and 300,000 ms;
completed results are retained until delivery or owner retirement, not silently dropped.

A WASM trap or invalid guest output retires that instance immediately. Its resources,
pending requests and delegated authority are revoked. Ordinary typed API failures
are reported without treating them as a crashed guest. No failed command is replayed.
Diagnostics retain at most 32 records per plugin, with plugin identity, scope,
operation and a bounded message. The management view shows the latest eight.

LSP startup permits three failed attempts, separated by 1 then 4 seconds. Later
transport errors share the budget. A successful connection alone does not clear the
budget; a successful operation after 60 seconds on that connection does. Time spent
offline/backing off is excluded. Retired document leases are not transport failures.
After the budget is exhausted the user must explicitly restart the plugin.

The installed plugin detail view provides **重启插件** and diagnostic status. Restart
replaces the selected scope's instance and language service transport, preserving
private files and the last safe checkpoint. Failed recovery is reported honestly;
it does not delete persisted data or restart peer plugins/the editor. Native process
trees are owned and terminated on retirement, but this is not a sandbox guaranteeing
limits on every operating-system resource. External effects are not rolled back.
