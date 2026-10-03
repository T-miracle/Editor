# Private data formats

An executable capability package may declare:

```json
{
  "data_format": {"version": 2, "migration_hook": true},
  "api": {"base": "^1", "required": {"storage.private": "^1", "storage.migration": "^1"}},
  "permissions": ["storage"]
}
```

Versions are positive integers owned by the plugin, independent of package semver.
Zero means a new private scope. Existing unversioned private scopes begin at version
one. The host stores the committed version per workspace/application scope, so a
dormant workspace is migrated before its next activation. Changing or removing a
declared format cannot silently bypass migration; declare a hook for version changes.

The host sends `Notification::MigrateData { from, to, snapshot }` when a scope changes
format. The snapshot is opaque plugin data, not a process/connection image. Only
package asset reads and private-copy file operations are permitted in this hook;
workspace, editor, process and cross-plugin service access is denied. File handles
opened by the hook expire when it returns. Return an optional converted snapshot;
omitting it retains the supplied snapshot. No UI or other proposals may be returned.
The new instance receives the converted snapshot during its final `Prepare`, followed
by configuration and activation. It never receives an old-format snapshot as its
initialization state before the hook. Ordinary execution budgets apply.

Preparation compiles and validates the candidate while retaining the old instance.
The runtime's `prepare_installation` token may be held while dispatching old commands;
`commit_installation` takes a fresh copy and final snapshot under the owning worker's
exclusive turn. Thus the initial preparation copy cannot overwrite newer writes.
Dropping a preparation token discards its unpublished copy. Tokens are bound to the
manager root, workspace and previous package digest.

The candidate activates against its isolated directory. After successful activation,
the host flushes a journal containing the prior registry, swaps the data scope while
retaining a backup, atomically writes the new registry, then flushes a committed marker.
Only after the marker can it remove the backup and publish the candidate. Startup
rolls back journals lacking that marker and finishes cleanup for committed journals.
If restoration cannot complete, startup/activation fails with the retained recovery
location. Backups are not discarded, and no guest resumes against uncertain data.
The journal covers private files, opaque snapshots, format versions and the package
record; user documents, external effects and native process identity are excluded.

Executable private-data writers have one runtime owner for an installation root.
Metadata-only and declarative dependency managers remain independent. Taking ownership
refreshes the registry after locking; transaction readers reject a busy commit rather
than activating a mixed package/data version. Existing logical workspaces remain
isolated within their owning manager. The copy accepts bounded regular files only:
128 MiB total, 16,384 entries, depth 16; links are rejected. The recovery guarantees are
validated with process interruption, not a claim that every filesystem/controller
provides power-loss durability.
