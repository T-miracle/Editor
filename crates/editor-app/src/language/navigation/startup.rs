//! Create an owned stdio transport and perform the standard LSP initialization handshake.
use super::*;
use plugin_runtime::plugin_protocol::language::{
    SEMANTIC_LINKED_EDITING_CAPABILITY, SEMANTIC_LINKED_EDITING_VERSION,
    SemanticLinkedEditingCapabilities,
};
impl LanguageServerConnection {
    /// Starts the plugin's server and completes the LSP handshake.
    pub(super) fn start(
        root_uri: &Uri,
        service: &Arc<plugin_runtime::LanguageService>,
        retired: &Arc<std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<Self> {
        // Every server is launched by its permission-checked runtime owner.
        let configuration = service.provider.initialization_options.clone();
        let (child, input, stdout) = service.spawn_for_owner(retired)?;
        service.append_runtime_log(
            retired,
            plugin_runtime::LogLevel::Info,
            &format!("lsp/{}", service.provider.id),
            rust_i18n::t!("plugins.logs.lsp_started").to_string(),
        );
        // Unsolicited server messages must reach the same log sink even with no open document.
        let output = transport::reader(stdout, service.clone(), retired.clone())?;
        let mut connection = Self {
            documents: Default::default(),
            child,
            service: service.clone(),
            input: transport::Writer::new(input),
            output,
            root_uri: root_uri.as_str().to_owned(),
            language_id: service.provider.language.clone(),
            next_id: 1,
            diagnostics: diagnostics::DiagnosticsStore::default(),
            snapshot_uris: service
                .provider
                .diagnostic_snapshots
                .then(snapshots::SnapshotUris::new)
                .transpose()?,
            save_notifications: None,
            pull_diagnostics: false,
            definition_provider: false,
            type_definition_provider: false,
            formatting_provider: false,
            rename_provider: false,
            prepare_rename_provider: false,
            linked_editing_provider: false,
            semantic_linked_editing_provider: false,
            configuration,
            failed: false,
        };

        if let Err(error) = connection.initialize(root_uri) {
            // Failed initialization still owns the server's final stderr diagnostic until bounded teardown.
            connection.failed = true;
            return Err(error);
        }
        Ok(connection)
    }

    /// Server capabilities control optional notifications; initialization options stay opaque plugin data.
    fn initialize(&mut self, root_uri: &Uri) -> anyhow::Result<()> {
        let root_uri = root_uri.as_str();
        // Ordinary experimental names remain plugin data. This reserved marker must originate
        // from the negotiated runtime grant, never a plugin-supplied initialization override.
        let mut experimental = self.service.provider.client_experimental.clone();
        experimental.remove(SEMANTIC_LINKED_EDITING_CAPABILITY);
        let semantic_linked_authorized = self.service.provider.editing
            && self
                .service
                .capability("language.editing")
                .is_some_and(|version| version.major == 1 && version.minor >= 1);
        if semantic_linked_authorized {
            experimental.insert(
                SEMANTIC_LINKED_EDITING_CAPABILITY.into(),
                serde_json::to_value(SemanticLinkedEditingCapabilities {
                    version: SEMANTIC_LINKED_EDITING_VERSION,
                })?,
            );
        }
        let initialized = self.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                // Match rootUri even for servers that still consult deprecated rootPath.
                "rootPath": url::Url::parse(root_uri)?.to_file_path().ok(),
                "workspaceFolders": [{ "uri": root_uri, "name": "workspace" }],
                "initializationOptions": self.configuration,
                "capabilities": {
                    "general": { "positionEncodings": ["utf-16"] },
                    // Single-document versioned text edits are supported; resource and annotation
                    // capabilities remain unadvertised because this native command cannot apply them.
                    "workspace": { "workspaceFolders": true, "workspaceEdit": {"documentChanges":true} },
                    "textDocument": {
                        "diagnostic": { "dynamicRegistration": false, "relatedDocumentSupport": false },
                        "synchronization": { "didSave": true },
                        "publishDiagnostics": {
                            "versionSupport": true,
                            "relatedInformation": true,
                            "tagSupport": { "valueSet": [1, 2] },
                            "codeDescriptionSupport": true
                        },
                        "definition": { "linkSupport": true },
                        "typeDefinition": { "linkSupport": true },
                        "formatting": { "dynamicRegistration": false },
                        "rename": { "dynamicRegistration": false, "prepareSupport": true },
                        "linkedEditingRange": { "dynamicRegistration": false },
                        "completion": {
                            "completionItem": { "snippetSupport": false }
                        },
                        "hover": { "contentFormat": ["markdown", "plaintext"] }
                    },
                    "experimental": experimental
                },
                "clientInfo": { "name": "Me Editor", "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        // Only send optional save notifications when the server requests them.
        self.pull_diagnostics = initialized["capabilities"]["diagnosticProvider"].is_object()
            || initialized["capabilities"]["diagnosticProvider"] == Value::Bool(true);
        // Objects express options/registration data; false or absent means the method is unavailable.
        self.definition_provider = initialized["capabilities"]["definitionProvider"].is_object()
            || initialized["capabilities"]["definitionProvider"] == Value::Bool(true);
        self.type_definition_provider = initialized["capabilities"]["typeDefinitionProvider"]
            .is_object()
            || initialized["capabilities"]["typeDefinitionProvider"] == Value::Bool(true);
        let capabilities = &initialized["capabilities"];
        let supported =
            |key: &str| capabilities[key].is_object() || capabilities[key] == Value::Bool(true);
        self.formatting_provider = supported("documentFormattingProvider");
        self.rename_provider = supported("renameProvider");
        self.prepare_rename_provider =
            capabilities["renameProvider"]["prepareProvider"] == Value::Bool(true);
        self.linked_editing_provider = supported("linkedEditingRangeProvider");
        self.semantic_linked_editing_provider = semantic_linked_supported(
            semantic_linked_authorized,
            capabilities["experimental"][SEMANTIC_LINKED_EDITING_CAPABILITY].clone(),
        );
        self.save_notifications = match &initialized["capabilities"]["textDocumentSync"]["save"] {
            Value::Bool(true) => Some(false),
            Value::Object(options) => Some(
                options
                    .get("includeText")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
            _ => None,
        };
        ensure!(
            initialized["capabilities"]["positionEncoding"]
                .as_str()
                .is_none_or(|encoding| encoding == "utf-16"),
            "language server selected an unsupported position encoding"
        );
        self.notify("initialized", json!({ "capabilities": {} }))?;
        Ok(())
    }
}

/// Parsing a strict marker never grants an absent runtime capability or a future extension version.
fn semantic_linked_supported(authorized: bool, marker: Value) -> bool {
    authorized
        && serde_json::from_value::<SemanticLinkedEditingCapabilities>(marker)
            .is_ok_and(|marker| marker.version == SEMANTIC_LINKED_EDITING_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the currently negotiated marker shape enables the generic semantic request.
    #[test]
    fn semantic_linked_handshake_rejects_unknown_versions_fields_and_absent_authority() {
        assert!(semantic_linked_supported(true, json!({"version":1})));
        for marker in [
            Value::Null,
            json!(true),
            json!({"version":2}),
            json!({"version":1,"trusted":true}),
        ] {
            assert!(!semantic_linked_supported(true, marker));
        }
        assert!(!semantic_linked_supported(false, json!({"version":1})));
    }
}
