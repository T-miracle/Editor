//! Versioned LSP diagnostics shared by document synchronization and server requests.

use super::*;
use lsp_types::{Diagnostic, PublishDiagnosticsParams};

/// Server versions belong to synchronized snapshots, not hover/completion requests.
#[derive(Default)]
pub(super) struct DiagnosticsStore {
    documents: HashMap<String, Document>,
}

struct Document {
    version: i32,
    source: String,
    /// None means the server has not published for this snapshot yet; Some([]) clears it.
    diagnostics: Option<Vec<Diagnostic>>,
}

impl DiagnosticsStore {
    /// Repeated requests for unchanged text must not invalidate diagnostics or restart analysis.
    pub(super) fn next_version(&self, uri: &str, source: &str) -> Option<i32> {
        match self.documents.get(&document_key(uri)) {
            Some(document) if document.source == source => None,
            Some(document) => Some(document.version + 1),
            None => Some(1),
        }
    }

    /// Install only after didOpen/didChange was successfully written to the server.
    pub(super) fn synchronized(&mut self, uri: String, source: String, version: i32) {
        self.documents.insert(
            document_key(&uri),
            Document {
                version,
                source,
                diagnostics: None,
            },
        );
    }

    /// Reject a delayed push for a prior version; an empty publication is meaningful.
    pub(super) fn publish(&mut self, params: PublishDiagnosticsParams) {
        let Some(document) = self.documents.get_mut(&document_key(params.uri.as_str())) else {
            return;
        };
        if params
            .version
            .is_some_and(|version| version != document.version)
        {
            return;
        }
        // Servers omitting version assign notifications to the currently synchronized
        // snapshot. Queued notifications are drained before changing that snapshot.
        document.diagnostics = Some(params.diagnostics);
    }

    /// Return results only for the exact text requested by the editor's revision-bound worker.
    pub(super) fn snapshot(&self, uri: &str, source: &str) -> Option<Vec<Diagnostic>> {
        self.documents
            .get(&document_key(uri))
            .filter(|document| document.source == source)
            .and_then(|document| document.diagnostics.clone())
    }
}

/// URI spellings may differ in percent encoding and Windows drive-letter casing.
fn document_key(uri: &str) -> String {
    let Some(path) = url::Url::parse(uri)
        .ok()
        .and_then(|url| url.to_file_path().ok())
    else {
        return uri.to_owned();
    };
    let path = path.to_string_lossy();
    #[cfg(target_os = "windows")]
    {
        path.trim_start_matches("\\\\?\\")
            .replace('/', "\\")
            .to_lowercase()
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.into_owned()
    }
}

impl LanguageServer {
    /// Synchronize live text and drain unsolicited diagnostics without waiting for a hover.
    pub(crate) fn diagnostics(
        &self,
        uri: Uri,
        source: &str,
    ) -> anyhow::Result<Option<Vec<Diagnostic>>> {
        self.prepare()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        let result = (|| {
            let connection = connection
                .as_mut()
                .context("language server is unavailable")?;
            // Idle polling must not copy large unchanged documents every 400 ms.
            let uri_text = if connection
                .diagnostics
                .next_version(uri.as_str(), source)
                .is_some()
            {
                connection.sync_document(uri, source.to_owned())?
            } else {
                uri.as_str().to_owned()
            };
            connection.drain_messages()?;
            let pushed = connection.diagnostics.snapshot(&uri_text, source);
            if connection.pull_diagnostics {
                // Pull analyzes the live document even when a server defers its
                // push-based semantic pass. Omit previousResultId to request a
                // full report; retain pushed compiler diagnostics as a supplement.
                let report = connection.request(
                    "textDocument/diagnostic",
                    json!({ "textDocument": { "uri": uri_text } }),
                )?;
                let mut items: Vec<Diagnostic> = serde_json::from_value(report["items"].clone())
                    .context("decode full document diagnostic report")?;
                items.extend(pushed.into_iter().flatten());
                Ok(Some(items))
            } else {
                Ok(pushed)
            }
        })();
        if result.is_err() {
            *connection = None;
        }
        result
    }

    /// Saving triggers compiler-backed diagnostics when the server advertises didSave support.
    pub(crate) fn document_saved(&self, uri: Uri, source: String) -> anyhow::Result<()> {
        self.prepare()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        let connection = connection
            .as_mut()
            .context("language server is unavailable")?;
        let uri_text = connection.sync_document(uri, source.clone())?;
        if let Some(include_text) = connection.save_notifications {
            let mut params = json!({ "textDocument": { "uri": uri_text } });
            if include_text {
                params["text"] = Value::String(source);
            }
            connection.notify("textDocument/didSave", params)?;
        }
        Ok(())
    }
}

impl LanguageServerConnection {
    /// Bound each polling turn so completion and navigation can acquire the shared connection.
    pub(super) fn drain_messages(&mut self) -> anyhow::Result<()> {
        for _ in 0..256 {
            match self.output.try_recv() {
                Ok(message) => self.handle_server_message(&message?)?,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    anyhow::bail!("language server closed its output")
                }
            }
        }
        Ok(())
    }

    /// Both request waits and idle polling must process the same server notifications.
    pub(super) fn handle_server_message(&mut self, message: &Value) -> anyhow::Result<()> {
        if message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
        {
            match serde_json::from_value::<PublishDiagnosticsParams>(message["params"].clone()) {
                Ok(params) => self.diagnostics.publish(params),
                Err(error) => {
                    tracing::warn!(%error, "invalid language diagnostic notification");
                }
            }
        }
        if let Some(readiness) = &self.readiness
            && message.get("method").and_then(Value::as_str)
                == Some(readiness.notification.as_str())
        {
            self.ready = message
                .get("params")
                .and_then(|params| params.get(&readiness.ready_field))
                .and_then(Value::as_bool);
        }
        if let (Some(id), Some(method)) = (
            message.get("id"),
            message.get("method").and_then(Value::as_str),
        ) {
            let result = match method {
                "workspace/workspaceFolders" => {
                    json!([{ "uri": self.root_uri, "name": "workspace" }])
                }
                "workspace/configuration" => message
                    .get("params")
                    .and_then(|params| params.get("items"))
                    .and_then(Value::as_array)
                    .map(|items| {
                        Value::Array(
                            items
                                .iter()
                                .map(|item| {
                                    super::super::sdk::configuration_section(
                                        &self.configuration,
                                        item.get("section").and_then(Value::as_str),
                                    )
                                })
                                .collect(),
                        )
                    })
                    .unwrap_or_else(|| json!([])),
                _ => Value::Null,
            };
            self.write_message(json!({ "jsonrpc": "2.0", "id": id, "result": result }))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
