# Independent configuration provider example

This standalone package consumes the editor's public `run.configurations` contract. It provides two templates with different native layouts, complete literal argument arrays, a readonly program and plugin-owned validation. Host configuration identity and Apply/Save/Cancel remain outside the package.

For isolated verification the default program is `probe.exe` (Windows) or `probe` under the supplied workspace. The verification fixture prepares that executable explicitly; adding a configuration never starts it. This package is a verification example, not a bundled Shell or language provider.

Fault verification uses only the public workspace file capability. `.configuration-form-offline` rejects form acknowledgements, while `.configuration-validation` selects business rejection, service error, malformed success or a genuinely unanswered invocation. These markers belong to isolated verification workspaces; cancelling an unanswered invocation releases its reply resource. Missing files mean normal operation, and other file errors remain visible.
