//! Connects plugin languages to their declared language servers.

mod diagnostics;
mod documents;
pub(crate) use documents::DocumentLease;
mod recovery;
pub(crate) use recovery::RecoveryState;
mod service;
mod startup;
mod transport;

use anyhow::{Context as _, anyhow, ensure};
use gpui_base::input::{DefinitionProvider, Rope};
use gpui_kit::gpui::{App, Task, Window};
use lsp_types::{CompletionResponse, Hover, Location, LocationLink, Position, Uri};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[cfg(test)]
mod readiness_tests;
#[cfg(test)]
mod runtime_log_tests;

/// Shares one language server and its document state across language tabs.
pub struct LanguageServer {
    recovery: Mutex<recovery::Recovery>,
    attempt: Mutex<()>,
    started: Instant,
    documents: Mutex<HashMap<String, DocumentLease>>,
    root: PathBuf,
    root_uri: Uri,
    service: Arc<plugin_runtime::LanguageService>,
    /// Reader callbacks share this adapter lifetime so a retired view cannot publish new alerts.
    retired: Arc<std::sync::atomic::AtomicBool>,
    connection: Mutex<Option<LanguageServerConnection>>,
}

impl LanguageServer {
    /// Starts and initializes a shared server before the first navigation request.
    fn prepare_once(&self) -> anyhow::Result<()> {
        ensure!(self.is_active(), "LSP provider has been retired");
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        ensure!(self.is_active(), "LSP provider has been retired");
        if connection.is_none() {
            *connection = Some(LanguageServerConnection::start(
                &self.root_uri,
                &self.service,
                &self.retired,
            )?);
        }
        Ok(())
    }

    /// Waits for a plugin-declared readiness signal after the LSP handshake.
    fn prepare_ready_once(&self) -> anyhow::Result<()> {
        self.prepare_once()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        let connection = connection
            .as_mut()
            .context("LSP provider has been retired")?;
        let Some(readiness) = &connection.service.provider.readiness else {
            return Ok(());
        };
        let deadline = Instant::now() + Duration::from_millis(readiness.timeout_ms.into());
        loop {
            ensure!(self.is_active(), "LSP provider has been retired");
            ensure!(
                Instant::now() < deadline,
                "language server readiness timed out"
            );
            // Process pending server requests before accepting a reader-observed readiness signal.
            // The shared state also survives discarded ordinary notifications and log-only readiness.
            connection.drain_messages_until(deadline)?;
            // A ready signal received during an expired or retired reply wait cannot revive this plan.
            ensure!(self.is_active(), "LSP provider has been retired");
            ensure!(
                Instant::now() < deadline,
                "language server readiness timed out"
            );
            if connection.output.is_ready() {
                return Ok(());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            ensure!(!remaining.is_zero(), "language server readiness timed out");
            // Control-state updates do not need a document-queue slot to wake this bounded wait.
            match connection
                .output
                .recv_timeout(remaining.min(Duration::from_millis(100)))
            {
                Ok(message) => connection.handle_server_message_until(&message?, deadline)?,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => return Err(error).context("wait for LSP readiness"),
            }
        }
    }

    /// Requests a definition after synchronizing the current document.
    pub(super) fn definitions_for(
        &self,
        document: DocumentLease,
        source: String,
        position: Position,
    ) -> anyhow::Result<Vec<LocationLink>> {
        self.prepare()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        connection
            .as_mut()
            .context("LSP provider has been retired")?
            .bind_document(&document)?;
        let result = connection
            .as_mut()
            .context("LSP provider has been retired")?
            .definitions(document.uri.clone(), source, position);
        self.connection_failed("definition", &result, &mut connection);
        ensure!(
            self.is_active() && document.is_active(),
            "LSP request owner has been retired"
        );
        result
    }

    /// Shares the same document versions and connection with definition requests.
    pub(super) fn completions_for(
        &self,
        document: DocumentLease,
        source: String,
        position: Position,
    ) -> anyhow::Result<CompletionResponse> {
        self.prepare()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        connection
            .as_mut()
            .context("LSP provider has been retired")?
            .bind_document(&document)?;
        let result = connection
            .as_mut()
            .context("LSP provider has been retired")?
            .completions(document.uri.clone(), source, position);
        self.connection_failed("completion", &result, &mut connection);
        ensure!(
            self.is_active() && document.is_active(),
            "LSP request owner has been retired"
        );
        result
    }

    /// Shares document versions with definitions and completions for hover details.
    pub(super) fn hover_for(
        &self,
        document: DocumentLease,
        source: String,
        position: Position,
    ) -> anyhow::Result<Option<Hover>> {
        self.prepare()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        connection
            .as_mut()
            .context("LSP provider has been retired")?
            .bind_document(&document)?;
        let result = connection
            .as_mut()
            .context("LSP provider has been retired")?
            .hover(document.uri.clone(), source, position);
        self.connection_failed("hover", &result, &mut connection);
        ensure!(
            self.is_active() && document.is_active(),
            "LSP request owner has been retired"
        );
        result
    }

    /// Supplies the plugin's automatic completion punctuation to the UI bridge.
    pub(super) fn completion_triggers(&self) -> &[String] {
        &self.service.provider.completion_triggers
    }

    /// Exposes plugin-declared whitespace contexts without embedding Rust syntax in the host.
    pub(super) fn completion_after_whitespace(&self) -> &[String] {
        &self.service.provider.completion_after_whitespace
    }
}

/// Adapts the editor's Go to Definition hook to an asynchronous plugin server.
pub struct LanguageDefinitionProvider {
    server: Arc<LanguageServer>,
    document: DocumentLease,
}

impl LanguageDefinitionProvider {
    /// Creates a provider for one source file and a shared project language server.
    pub fn new(path: &Path, server: Arc<LanguageServer>) -> Option<Self> {
        Some(Self {
            document: server.open_document(file_uri(path)?),
            server,
        })
    }
}

impl DefinitionProvider for LanguageDefinitionProvider {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Vec<LocationLink>>> {
        // Convert the editor's character offset to the LSP UTF-16 position before dispatch.
        let source = text.to_string();
        let byte_offset = text.char_to_byte_idx(offset.min(text.len_chars()));
        let position = position_at_byte(&source, byte_offset);
        let server = self.server.clone();
        let document = self.document.clone();

        // A dedicated executor keeps process I/O and workspace indexing off the UI thread.
        cx.background_executor()
            .scheduler_executor()
            .spawn_dedicated(move |_| async move {
                let result = server.definitions_for(document, source, position);
                if let Err(error) = &result {
                    tracing::warn!(%error, "language definition request failed");
                }
                result
            })
    }
}

/// Owns the language-server process and its framed JSON-RPC streams.
struct LanguageServerConnection {
    documents: HashMap<String, DocumentLease>,
    child: plugin_runtime::ServiceProcess,
    service: Arc<plugin_runtime::LanguageService>,
    input: transport::Writer,
    output: transport::Messages,
    root_uri: String,
    language_id: String,
    next_id: u64,
    diagnostics: diagnostics::DiagnosticsStore,
    /// None disables didSave; the boolean controls inclusion of the saved text.
    save_notifications: Option<bool>,
    /// Prefer standard pull diagnostics when advertised, including for unsaved buffers.
    pull_diagnostics: bool,
    /// Keep host-injected options available for subsequent workspace/configuration requests.
    configuration: Value,
    /// Fault teardown permits a bounded stderr EOF drain; ordinary retirement cuts off alerts first.
    failed: bool,
}

impl LanguageServerConnection {
    /// Opens or replaces a document snapshot, then returns all locations from the server.
    fn definitions(
        &mut self,
        uri: Uri,
        source: String,
        position: Position,
    ) -> anyhow::Result<Vec<LocationLink>> {
        let uri_text = self.sync_document(uri, source)?;
        let params = json!({
            "textDocument": { "uri": uri_text },
            "position": position
        });
        let response = self.request("textDocument/definition", params)?;
        decode_definitions(response)
    }

    /// Requests completion after publishing the current editor snapshot.
    fn completions(
        &mut self,
        uri: Uri,
        source: String,
        position: Position,
    ) -> anyhow::Result<CompletionResponse> {
        let uri_text = self.sync_document(uri, source)?;
        let response = self.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri_text },
                "position": position,
                "context": { "triggerKind": 1 }
            }),
        )?;
        // An absent result means the server has no suggestions at this position.
        if response.is_null() {
            return Ok(CompletionResponse::Array(Vec::new()));
        }
        serde_json::from_value(response).context("decode language completion response")
    }

    /// Reads the language server's type signature and documentation at a position.
    fn hover(
        &mut self,
        uri: Uri,
        source: String,
        position: Position,
    ) -> anyhow::Result<Option<Hover>> {
        let uri_text = self.sync_document(uri, source)?;
        let response = self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri_text },
                "position": position
            }),
        )?;
        if response.is_null() {
            return Ok(None);
        }
        serde_json::from_value(response)
            .map(Some)
            .context("decode language hover response")
    }

    /// Publish changed snapshots once; identical hover and diagnostic requests share a version.
    fn sync_document(&mut self, uri: Uri, source: String) -> anyhow::Result<String> {
        // Attribute already queued, unversioned pushes to the old snapshot before replacing it.
        self.drain_messages()?;
        let uri_text = uri.as_str().to_owned();
        let Some(version) = self.diagnostics.next_version(&uri_text, &source) else {
            return Ok(uri_text);
        };
        ensure!(
            version < i32::MAX,
            "LSP document version exhausted; restart service"
        );
        if !self.diagnostics.is_open(&uri_text) {
            self.notify("textDocument/didOpen", json!({
                "textDocument": { "uri": uri_text, "languageId": self.language_id, "version": version, "text": source }
            }))?;
        } else {
            self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri_text, "version": version },
                    "contentChanges": [{ "text": source }]
                }),
            )?;
        }
        self.diagnostics
            .synchronized(uri_text.clone(), source, version);
        Ok(uri_text)
    }

    /// Wait for the matching response while processing retained diagnostic pushes and server requests.
    fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        ensure!(self.service.is_active(), "LSP provider has been retired");
        let id = self.next_id;
        self.next_id += 1;
        let deadline = Instant::now() + Duration::from_secs(30);
        self.write_message(
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        )?;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let _ = self.input.enqueue(
                    json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":id}}),
                );
                anyhow::bail!("language server {method} request timed out");
            }
            let message = match self.output.recv_timeout(remaining) {
                Ok(message) => message?,
                Err(error) => {
                    // Stop waiting is explicit JSON-RPC cancellation, never a claim of server rollback.
                    let _ = self.input.enqueue(
                        json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":id}}),
                    );
                    return Err(error).context(format!("wait for language server {method}"));
                }
            };
            self.handle_server_message_until(&message, deadline)?;
            // Client and server IDs are independent; only a response satisfies this request.
            if message.get("method").is_none()
                && message.get("id").and_then(Value::as_u64) == Some(id)
            {
                if let Some(error) = message.get("error") {
                    anyhow::bail!("language server {method} request failed: {error}");
                }
                ensure!(self.service.is_active(), "LSP provider has been retired");
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    /// Sends a JSON-RPC notification without waiting for a response.
    fn notify(&mut self, method: &str, params: Value) -> anyhow::Result<()> {
        self.write_message(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// Writes a UTF-8 JSON-RPC payload using the LSP Content-Length framing.
    fn write_message(&mut self, message: Value) -> anyhow::Result<()> {
        self.input.send(message, Duration::from_secs(30))
    }
}

impl Drop for LanguageServerConnection {
    fn drop(&mut self) {
        // Stop reader alerts before terminating the pipe; deliberate cleanup must not look like a crash.
        self.output.stop_logging();
        if self.failed {
            // Failure output belongs to the old immutable owner and must survive its connection's drop.
            // Runtime teardown also respects any concurrent controlled retirement's earlier cutoff.
            self.child.stop_after_failure();
        } else {
            self.child.stop();
        }
        // The immutable plan retains the old owner even when a replacement is already selected.
        self.service.runtime_logs().append(
            &self.service.owner,
            plugin_runtime::LogLevel::Info,
            &format!("lsp/{}", self.service.provider.id),
            rust_i18n::t!("plugins.logs.lsp_stopped").to_string(),
        );
    }
}

/// Converts Location or LocationLink results into the editor's link format.
fn decode_definitions(value: Value) -> anyhow::Result<Vec<LocationLink>> {
    let values = match value {
        Value::Null => return Ok(Vec::new()),
        Value::Array(values) => values,
        value => vec![value],
    };

    values
        .into_iter()
        .map(|value| {
            if value.get("targetUri").is_some() {
                let link: LocationLink =
                    serde_json::from_value(value).context("decode LSP LocationLink")?;
                return Ok(link);
            }
            let location: Location =
                serde_json::from_value(value).context("decode LSP Location")?;
            Ok(LocationLink {
                origin_selection_range: None,
                target_uri: location.uri,
                target_range: location.range.clone(),
                target_selection_range: location.range,
            })
        })
        .collect()
}

/// Creates an LSP file URI from a canonical or workspace file path.
pub(crate) fn file_uri(path: &Path) -> Option<Uri> {
    let uri = url::Url::from_file_path(path).ok()?;
    let text = uri.to_string();
    // Normalize workspace and document URIs to the same Windows drive spelling; mismatched drive casing can make buffers read-only.
    #[cfg(windows)]
    let text = {
        let mut text = text;
        if text.starts_with("file:///") && text.as_bytes().get(9) == Some(&b':') {
            text[8..9].make_ascii_lowercase();
        }
        text
    };
    Uri::from_str(&text).ok()
}

/// Converts a UTF-8 byte offset into the UTF-16 line and column required by LSP.
pub(super) fn position_at_byte(source: &str, byte_offset: usize) -> Position {
    let prefix = &source[..byte_offset.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, tail)| tail)
        .encode_utf16()
        .count() as u32;
    Position::new(line, column)
}
