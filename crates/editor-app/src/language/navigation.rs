//! Connects plugin languages to their declared language servers.

use super::toolchains::resolve_server_executable;
use anyhow::{Context as _, anyhow, ensure};
use gpui_base::input::{DefinitionProvider, Rope};
use gpui_kit::gpui::{App, Task, Window};
use lsp_types::{CompletionResponse, Hover, Location, LocationLink, Position, Uri};
use plugin_schema::{LanguageContribution, LspReadiness};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead as _, BufReader, BufWriter, Read as _, Write as _},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Shares one language server and its document state across language tabs.
pub struct LanguageServer {
    root: PathBuf,
    root_uri: Uri,
    language: LanguageContribution,
    connection: Mutex<Option<LanguageServerConnection>>,
}

impl LanguageServer {
    /// Creates a lazy language-server session for a project root.
    pub fn new(root: &Path, language: LanguageContribution) -> Option<Self> {
        language.lsp_command.as_ref()?;
        let root = root.canonicalize().ok()?;
        let root_uri = file_uri(&root)?;
        Some(Self {
            root,
            root_uri,
            language,
            connection: Mutex::new(None),
        })
    }

    /// Starts and initializes a shared server before the first navigation request.
    pub fn prepare(&self) -> anyhow::Result<()> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        if connection.is_none() {
            *connection = Some(LanguageServerConnection::start(
                &self.root,
                &self.root_uri,
                &self.language,
            )?);
        }
        Ok(())
    }

    /// Waits for a plugin-declared readiness signal after the LSP handshake.
    pub fn prepare_until_ready(&self) -> anyhow::Result<()> {
        self.prepare()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        let connection = connection
            .as_mut()
            .expect("the server was just initialized");
        let Some(readiness) = connection.readiness.clone() else {
            return Ok(());
        };
        let Some(poll_method) = readiness.poll_method else {
            return Ok(());
        };
        let timeout = Duration::from_millis(readiness.timeout_ms.min(60_000));
        let started = Instant::now();
        while connection.ready != Some(true) && started.elapsed() < timeout {
            // A request drives the existing JSON-RPC reader and consumes status notifications.
            connection.request(&poll_method, json!({}))?;
            if connection.ready != Some(true) {
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        ensure!(
            connection.ready == Some(true),
            "language server readiness timed out"
        );
        Ok(())
    }

    /// Requests a definition after synchronizing the current document.
    pub(super) fn definitions(
        &self,
        document_uri: Uri,
        source: String,
        position: Position,
    ) -> anyhow::Result<Vec<LocationLink>> {
        self.prepare()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        let result = connection
            .as_mut()
            .expect("the language server was just initialized")
            .definitions(document_uri, source, position);
        if result.is_err() {
            // A later navigation request can start a fresh process after a server failure.
            *connection = None;
        }
        result
    }

    /// Shares the same document versions and connection with definition requests.
    pub(super) fn completions(
        &self,
        document_uri: Uri,
        source: String,
        position: Position,
    ) -> anyhow::Result<CompletionResponse> {
        self.prepare()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        let result = connection
            .as_mut()
            .expect("the language server was just initialized")
            .completions(document_uri, source, position);
        if result.is_err() {
            // A broken connection can be restarted by a later editor request.
            *connection = None;
        }
        result
    }

    /// Shares document versions with definitions and completions for hover details.
    pub(super) fn hover(
        &self,
        document_uri: Uri,
        source: String,
        position: Position,
    ) -> anyhow::Result<Option<Hover>> {
        self.prepare()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("language-server connection lock was poisoned"))?;
        let result = connection
            .as_mut()
            .expect("the language server was just initialized")
            .hover(document_uri, source, position);
        if result.is_err() {
            // A failed transport may be restarted by the next language request.
            *connection = None;
        }
        result
    }

    /// Supplies the plugin's automatic completion punctuation to the UI bridge.
    pub(super) fn completion_triggers(&self) -> &[String] {
        &self.language.completion_triggers
    }

    /// Exposes plugin-declared whitespace contexts without embedding Rust syntax in the host.
    pub(super) fn completion_after_whitespace(&self) -> &[String] {
        &self.language.completion_after_whitespace
    }
}

/// Adapts the editor's Go to Definition hook to an asynchronous plugin server.
pub struct LanguageDefinitionProvider {
    server: Arc<LanguageServer>,
    document_uri: Uri,
}

impl LanguageDefinitionProvider {
    /// Creates a provider for one source file and a shared project language server.
    pub fn new(path: &Path, server: Arc<LanguageServer>) -> Option<Self> {
        Some(Self {
            server,
            document_uri: file_uri(path)?,
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
        let document_uri = self.document_uri.clone();

        // A dedicated executor keeps process I/O and workspace indexing off the UI thread.
        cx.background_executor()
            .scheduler_executor()
            .spawn_dedicated(move |_| async move {
                let result = server.definitions(document_uri, source, position);
                if let Err(error) = &result {
                    tracing::warn!(%error, "language definition request failed");
                }
                result
            })
    }
}

/// Owns the language-server process and its framed JSON-RPC streams.
struct LanguageServerConnection {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
    root_uri: String,
    language_id: String,
    next_id: u64,
    document_versions: HashMap<String, i32>,
    readiness: Option<LspReadiness>,
    ready: Option<bool>,
}

impl LanguageServerConnection {
    /// Starts the plugin's server and completes the LSP handshake.
    fn start(root: &Path, root_uri: &Uri, language: &LanguageContribution) -> anyhow::Result<Self> {
        let executable = resolve_server_executable(language)?;
        let mut child = Command::new(&executable)
            .args(&language.lsp_args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("start language server {}", executable.display()))?;
        let input = BufWriter::new(child.stdin.take().context("open language server stdin")?);
        let output = BufReader::new(child.stdout.take().context("open language server stdout")?);
        let mut connection = Self {
            child,
            input,
            output,
            root_uri: root_uri.as_str().to_owned(),
            language_id: language.id.clone(),
            next_id: 1,
            document_versions: HashMap::new(),
            readiness: language.lsp_readiness.clone(),
            ready: None,
        };

        let root_uri = root_uri.as_str();
        // Experimental capability names are data supplied by the language plugin.
        let mut experimental = serde_json::Map::new();
        if let Some(readiness) = &language.lsp_readiness {
            experimental.insert(readiness.client_capability.clone(), Value::Bool(true));
        }
        connection.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "rootPath": root.to_string_lossy(),
                "workspaceFolders": [{ "uri": root_uri, "name": "workspace" }],
                "capabilities": {
                    "workspace": { "workspaceFolders": true },
                    "textDocument": {
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
        connection.notify("initialized", json!({ "capabilities": {} }))?;
        Ok(connection)
    }

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
        let mut response = self.request("textDocument/definition", params.clone())?;
        if let Some(readiness) = &self.readiness {
            let timeout = Duration::from_millis(readiness.timeout_ms.min(60_000));
            let started = Instant::now();
            // An empty answer during startup can precede workspace indexing.
            while definition_is_empty(&response)
                && self.ready != Some(true)
                && started.elapsed() < timeout
            {
                std::thread::sleep(Duration::from_millis(250));
                response = self.request("textDocument/definition", params.clone())?;
            }
        }
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

    /// Keeps a single monotonically increasing version for every opened document.
    fn sync_document(&mut self, uri: Uri, source: String) -> anyhow::Result<String> {
        let uri_text = uri.as_str().to_owned();
        let previous_version = self.document_versions.get(&uri_text).copied().unwrap_or(0);
        if previous_version == 0 {
            self.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri_text,
                        "languageId": self.language_id,
                        "version": 1,
                        "text": source
                    }
                }),
            )?;
            self.document_versions.insert(uri_text.clone(), 1);
        } else {
            let version = previous_version + 1;
            self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri_text, "version": version },
                    "contentChanges": [{ "text": source }]
                }),
            )?;
            self.document_versions.insert(uri_text.clone(), version);
        }

        Ok(uri_text)
    }

    /// Sends one JSON-RPC request and reads messages until its matching response arrives.
    fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        )?;

        loop {
            let message = self.read_message()?;
            if let Some(readiness) = &self.readiness
                && message.get("method").and_then(Value::as_str)
                    == Some(readiness.notification.as_str())
            {
                self.ready = message
                    .get("params")
                    .and_then(|params| params.get(&readiness.ready_field))
                    .and_then(Value::as_bool);
            }
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    anyhow::bail!("language server {method} request failed: {error}");
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }

            // Servers can issue workspace requests while initialization or indexing runs.
            if let (Some(server_id), Some(server_method)) = (
                message.get("id").cloned(),
                message.get("method").and_then(Value::as_str),
            ) {
                let result = match server_method {
                    "workspace/workspaceFolders" => json!([
                        { "uri": self.workspace_root_uri()?, "name": "workspace" }
                    ]),
                    "workspace/configuration" => message
                        .get("params")
                        .and_then(|params| params.get("items"))
                        .and_then(Value::as_array)
                        .map(|items| Value::Array(vec![Value::Null; items.len()]))
                        .unwrap_or_else(|| json!([])),
                    _ => Value::Null,
                };
                self.write_message(json!({ "jsonrpc": "2.0", "id": server_id, "result": result }))?;
            }
        }
    }

    /// Sends a JSON-RPC notification without waiting for a response.
    fn notify(&mut self, method: &str, params: Value) -> anyhow::Result<()> {
        self.write_message(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// Writes a UTF-8 JSON-RPC payload using the LSP Content-Length framing.
    fn write_message(&mut self, message: Value) -> anyhow::Result<()> {
        let payload = serde_json::to_vec(&message).context("serialize LSP message")?;
        write!(self.input, "Content-Length: {}\r\n\r\n", payload.len())
            .context("write LSP message header")?;
        self.input
            .write_all(&payload)
            .context("write LSP message body")?;
        self.input.flush().context("flush LSP message")
    }

    /// Reads one bounded UTF-8 JSON-RPC payload from the server.
    fn read_message(&mut self) -> anyhow::Result<Value> {
        let mut content_length = None;
        loop {
            let mut header = String::new();
            ensure!(
                self.output
                    .read_line(&mut header)
                    .context("read LSP header")?
                    > 0,
                "language server closed its output"
            );
            if header == "\r\n" || header == "\n" {
                break;
            }
            if let Some((name, value)) = header.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                content_length = Some(value.trim().parse::<usize>().context("parse LSP length")?);
            }
        }
        let length = content_length.context("language server response omitted Content-Length")?;
        ensure!(
            length <= 32 * 1024 * 1024,
            "language server response exceeded 32 MiB"
        );
        let mut payload = vec![0; length];
        self.output
            .read_exact(&mut payload)
            .context("read LSP message body")?;
        serde_json::from_slice(&payload).context("parse language server JSON-RPC response")
    }

    /// Returns the project root URI used during initialization for server-initiated requests.
    fn workspace_root_uri(&self) -> anyhow::Result<String> {
        Ok(self.root_uri.clone())
    }
}

/// Distinguishes a pending empty definition result from a resolved location.
fn definition_is_empty(response: &Value) -> bool {
    response.is_null() || response.as_array().is_some_and(Vec::is_empty)
}

impl Drop for LanguageServerConnection {
    fn drop(&mut self) {
        // Closing the editor session must not leave its language-server process behind.
        let _ = self.child.kill();
        let _ = self.child.wait();
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
pub(super) fn file_uri(path: &Path) -> Option<Uri> {
    let uri = url::Url::from_file_path(path).ok()?;
    Uri::from_str(uri.as_str()).ok()
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
