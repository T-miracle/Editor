//! Create an owned stdio transport and perform the standard LSP initialization handshake.
use super::*;
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
            save_notifications: None,
            pull_diagnostics: false,
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
        // Experimental capability names are data supplied by the language plugin.
        let experimental = self.service.provider.client_experimental.clone();
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
                    "workspace": { "workspaceFolders": true },
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
                        "completion": {
                            "completionItem": { "snippetSupport": false }
                        },
                        "hover": { "contentFormat": ["markdown", "plaintext"] }
                    },
                    "experimental": experimental
                },
                "clientInfo": { "name": crate::app::APP_NAME, "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        // Only send optional save notifications when the server requests them.
        self.pull_diagnostics = initialized["capabilities"]["diagnosticProvider"].is_object()
            || initialized["capabilities"]["diagnosticProvider"] == Value::Bool(true);
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
