//! Versioned LSP diagnostics shared by document synchronization and server requests.

use super::*;
use lsp_types::{Diagnostic, PublishDiagnosticsParams};

/// Server versions belong to synchronized snapshots, not hover/completion requests.
#[derive(Default)]
pub(super) struct DiagnosticsStore {
    documents: HashMap<String, Document>,
    /// Versions never repeat within a connection, including closing and reopening the same URI.
    last_version: i32,
}

struct Document {
    version: i32,
    source: String,
    /// None means the server has not published for this snapshot yet; Some([]) clears it.
    diagnostics: Option<Vec<Diagnostic>>,
}

impl DiagnosticsStore {
    /// Closing drops snapshots so a reopened URI starts a fresh document lifetime.
    pub(super) fn close(&mut self, uri: &str) {
        self.documents.remove(&document_key(uri));
    }
    /// Repeated requests for unchanged text must not invalidate diagnostics or restart analysis.
    pub(super) fn next_version(&self, uri: &str, source: &str) -> Option<i32> {
        match self.documents.get(&document_key(uri)) {
            Some(document) if document.source == source => None,
            _ => Some(self.last_version.saturating_add(1)),
        }
    }

    /// Install only after didOpen/didChange was successfully written to the server.
    pub(super) fn synchronized(&mut self, uri: String, source: String, version: i32) {
        self.last_version = self.last_version.max(version);
        self.documents.insert(
            document_key(&uri),
            Document {
                version,
                source,
                diagnostics: None,
            },
        );
    }
    pub(super) fn is_open(&self, uri: &str) -> bool {
        self.documents.contains_key(&document_key(uri))
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
    pub(crate) fn diagnostics_for(
        &self,
        document: DocumentLease,
        source: &str,
    ) -> anyhow::Result<Option<Vec<Diagnostic>>> {
        self.prepare()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        // A retired document is an ownership failure, not a broken transport for the other open tabs.
        connection
            .as_mut()
            .context("language server is unavailable")?
            .bind_document(&document)?;
        let result = (|| {
            let connection = connection
                .as_mut()
                .context("language server is unavailable")?;
            let uri = document.uri.clone();
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
        self.connection_failed("diagnostics", &result, &mut connection);
        ensure!(
            self.is_active() && document.is_active(),
            "LSP request owner has been retired"
        );
        result
    }

    /// Saving triggers compiler-backed diagnostics when the server advertises didSave support.
    pub(crate) fn document_saved_for(
        &self,
        document: DocumentLease,
        source: String,
    ) -> anyhow::Result<()> {
        self.prepare()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        // An obsolete save belongs to a closed document; it must not consume the server's failure budget.
        connection
            .as_mut()
            .context("language server is unavailable")?
            .bind_document(&document)?;
        let result = (|| {
            let connection = connection
                .as_mut()
                .context("language server is unavailable")?;
            let uri_text = connection.sync_document(document.uri, source.clone())?;
            if let Some(include_text) = connection.save_notifications {
                let mut params = json!({ "textDocument": { "uri": uri_text } });
                if include_text {
                    params["text"] = Value::String(source);
                }
                connection.notify("textDocument/didSave", params)?;
            }
            Ok(())
        })();
        self.connection_failed("didSave", &result, &mut connection);
        result
    }
}

impl LanguageServerConnection {
    /// Bound each polling turn so completion and navigation can acquire the shared connection.
    pub(super) fn drain_messages(&mut self) -> anyhow::Result<()> {
        self.drain_messages_until(Instant::now() + Duration::from_secs(30))
    }

    /// Preserve server requests within the caller's existing budget, including readiness waits.
    /// An expired deadline fails the operation instead of silently discarding a reply.
    pub(super) fn drain_messages_until(&mut self, deadline: Instant) -> anyhow::Result<()> {
        for _ in 0..transport::MESSAGE_CAPACITY {
            match self.output.try_recv() {
                Ok(message) => {
                    ensure!(
                        Instant::now() < deadline,
                        "language server message handling timed out"
                    );
                    self.handle_server_message_until(&message?, deadline)?;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    anyhow::bail!("language server closed its output")
                }
            }
        }
        Ok(())
    }

    /// Use the request/readiness deadline for server replies, so a blocked stdin cannot renew it.
    /// Document publications still pass through their existing version/lifetime checks.
    pub(super) fn handle_server_message_until(
        &mut self,
        message: &Value,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        if message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
        {
            match serde_json::from_value::<PublishDiagnosticsParams>(message["params"].clone()) {
                Ok(params) => {
                    // Unversioned pushes cannot prove freshness; generic services use versioned push or pull.
                    if params.version.is_some() {
                        self.diagnostics.publish(params);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "invalid language diagnostic notification");
                }
            }
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
                                    let service = &self.service;
                                    item.get("section")
                                        .and_then(Value::as_str)
                                        .map_or_else(
                                            || service.provider.configuration.get(""),
                                            |section| service.provider.configuration.get(section),
                                        )
                                        .cloned()
                                        .unwrap_or(Value::Null)
                                })
                                .collect(),
                        )
                    })
                    .unwrap_or_else(|| json!([])),
                _ => Value::Null,
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            ensure!(!remaining.is_zero(), "language server reply timed out");
            self.input.send(
                json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                remaining,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
