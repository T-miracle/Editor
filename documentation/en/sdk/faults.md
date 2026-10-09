# Execution limits and recovery

[简体中文](../../zh-cn/sdk/faults.md)

The host applies the same limits to every plugin identity. A normal WASM call has 100 million
fuel units and a one-second epoch deadline with 10 ms clock ticks; a snapshot call has one
billion units and ten seconds. These bound guest execution, not blocking host I/O or native
process CPU time. A component has an aggregate 256 MiB linear memory budget, at most 32
memories, tables or instances, and at most 100,000 elements per table.

Existing queues remain bounded: each instance may hold 32 editor requests and eight document
subscriptions; document notifications coalesce by version and close when capacity is
exhausted. The service broker accepts at most 256 queued invocations. Quota failures return
typed errors. Request deadlines remain between 1 and 300,000 ms, and completed results are
retained until delivery or owner retirement rather than silently dropped.

A WASM trap or invalid guest output retires that instance immediately, revoking its resources,
pending requests and delegated authority. Ordinary typed API failures are reported without
being treated as a crashed guest, and no failed command is replayed. Diagnostics retain at
most 32 records per plugin with plugin identity, scope, operation and a bounded message; the
management view shows the latest eight.

LSP startup permits three failed attempts separated by one then four seconds, and later
transport errors share that budget. A successful connection alone does not clear the budget;
a successful operation after 60 seconds on that connection does. Time spent offline or backing
off is excluded, and retired document leases are not transport failures. Once the budget is
exhausted the user must restart the plugin explicitly.

The installed plugin detail view provides **Restart plugin** and a diagnostic status. A restart
replaces the selected scope's instance and language-service transport while preserving private
files and the last safe checkpoint. A failed recovery is reported honestly: it does not delete
persisted data and does not restart peer plugins or the editor. Native process trees are owned
and terminated on retirement, but this is not a sandbox that guarantees limits on every
operating-system resource, and external effects are not rolled back.
