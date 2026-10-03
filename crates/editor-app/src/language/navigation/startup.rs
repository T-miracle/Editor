//! Create an owned stdio transport and perform the standard LSP initialization handshake.
use super::*;
impl LanguageServerConnection {
    /// Starts the plugin's server and completes the LSP handshake.
    pub(super) fn start(
        root: &Path,
        root_uri: &Uri,
        language: &LanguageContribution,
        service: Option<&Arc<plugin_runtime::LanguageService>>,
        retired: &std::sync::atomic::AtomicBool,
    ) -> anyhow::Result<Self> {
        let configuration = if let Some(service) = service {
            service.provider.initialization_options.clone()
        } else {
            Value::Null
        };
        let (child, input, stdout) = if let Some(service) = service {
            let (child, input, output) = service.spawn_for_owner(retired)?;
            (OwnedServer::Service(child), input, output)
        } else {
            let executable = resolve_server_executable(language)?;
            let mut child = Command::new(&executable)
                .args(&language.lsp_args)
                .current_dir(root)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .with_context(|| format!("start language server {}", executable.display()))?;
            let input = child.stdin.take().context("open language server stdin")?;
            let stdout = child.stdout.take().context("open language server stdout")?;
            (OwnedServer::Legacy(child), input, stdout)
        };
        let output = transport::reader(stdout)?;
        let mut connection = Self {
            documents: Default::default(),
            child,
            service: service.cloned(),
            input: transport::Writer::new(input),
            output,
            root_uri: root_uri.as_str().to_owned(),
            language_id: language.id.clone(),
            next_id: 1,
            diagnostics: diagnostics::DiagnosticsStore::default(),
            save_notifications: None,
            pull_diagnostics: false,
            readiness: language.lsp_readiness.clone(),
            ready: None,
            configuration,
        };

        connection.initialize(root_uri, language)?;
        Ok(connection)
    }

    /// Server capabilities control optional notifications; initialization options stay opaque plugin data.
    fn initialize(
        &mut self,
        root_uri: &Uri,
        language: &LanguageContribution,
    ) -> anyhow::Result<()> {
        let root_uri = root_uri.as_str();
        // Experimental capability names are data supplied by the language plugin.
        let mut experimental = serde_json::Map::new();
        if let Some(service) = &self.service {
            experimental.extend(service.provider.client_experimental.clone());
        } else if let Some(readiness) = &language.lsp_readiness {
            experimental.insert(readiness.client_capability.clone(), Value::Bool(true));
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
                "clientInfo": { "name": "Me Editor", "version": env!("CARGO_PKG_VERSION") }
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
