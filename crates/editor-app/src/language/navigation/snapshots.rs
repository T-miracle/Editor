//! Immutable native URI aliases retain physical paths while isolating delayed diagnostic publications.
use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_CONNECTION: AtomicU32 = AtomicU32::new(1);

struct WireDocument {
    uri: Uri,
    version: i32,
}

/// Only metadata is duplicated; text and native undo remain owned by the original editor session.
pub(super) struct SnapshotUris {
    incarnation: u32,
    documents: HashMap<String, WireDocument>,
    owners: HashMap<String, Uri>,
}

impl SnapshotUris {
    /// Never reuse a wire identity after process recovery, even if a native reader finishes late.
    pub(super) fn new() -> anyhow::Result<Self> {
        let incarnation = NEXT_CONNECTION
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| anyhow!("LSP snapshot identities exhausted; restart application"))?;
        Ok(Self {
            incarnation,
            documents: HashMap::new(),
            owners: HashMap::new(),
        })
    }

    /// A fixed decoded prefix preserves the filename and enables exact absolute-glob adaptation.
    fn candidate(&self, original: &Uri, version: i32) -> anyhow::Result<Uri> {
        let raw = original.as_str();
        let parsed = url::Url::parse(raw)?;
        ensure!(
            parsed.scheme() == "file"
                && parsed.query().is_none()
                && parsed.fragment().is_none()
                && parsed.to_file_path().is_ok(),
            "Diagnostic snapshots require a local file URI"
        );
        let remainder = raw
            .strip_prefix("file://")
            .context("Unsupported file URI spelling")?;
        let authority_end = remainder.find('/').context("File URI has no root")? + 7;
        let path = &raw[authority_end + 1..];
        // UNC inserts after the share, local Windows after the drive; Unix uses the slash root.
        let root_end = if !remainder.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
            authority_end + 1 + path.find('/').context("File URI has no volume path")? + 1
        } else {
            authority_end + 1
        };
        let identity = (u64::from(self.incarnation) << 32) | u64::from(version as u32);
        let mut wire = String::with_capacity(
            raw.len() + 4 * plugin_runtime::plugin_protocol::language::SNAPSHOT_URI_SEGMENTS,
        );
        wire.push_str(&raw[..root_end]);
        for bit in (0..plugin_runtime::plugin_protocol::language::SNAPSHOT_URI_SEGMENTS).rev() {
            wire.push_str(if identity & (1_u64 << bit) == 0 {
                "%2e/"
            } else {
                "%2E/"
            });
        }
        wire.push_str(&raw[root_end..]);
        // URL parsers normalize dot segments; the LSP URI parser must preserve the raw identity.
        Uri::from_str(&wire).context("Construct immutable diagnostic URI")
    }

    fn current(&self, original: &Uri) -> Option<Uri> {
        self.documents
            .get(original.as_str())
            .map(|document| document.uri.clone())
    }

    /// Revoke the reverse mapping before sending close, so its clear/late pushes are discarded.
    fn retire(&mut self, original: &Uri) -> Option<Uri> {
        let document = self.documents.remove(original.as_str())?;
        self.owners.remove(document.uri.as_str());
        Some(document.uri)
    }

    fn activate(&mut self, original: Uri, wire: Uri, version: i32) {
        self.owners.insert(wire.as_str().into(), original.clone());
        self.documents.insert(
            original.as_str().into(),
            WireDocument { uri: wire, version },
        );
    }

    /// Raw identity is essential: canonical path matching would merge distinct immutable snapshots.
    fn publication(&self, wire: &Uri) -> Option<(Uri, i32)> {
        let original = self.owners.get(wire.as_str())?;
        let document = self.documents.get(original.as_str())?;
        Some((original.clone(), document.version))
    }
}

impl LanguageServerConnection {
    /// Unchanged requests reuse the same wire snapshot and never restart its native analysis.
    pub(super) fn wire_uri(&self, original: &Uri) -> Uri {
        self.snapshot_uris
            .as_ref()
            .and_then(|uris| uris.current(original))
            .unwrap_or_else(|| original.clone())
    }

    /// Close revokes both logical diagnostics and any immutable wire mapping before notifying.
    pub(super) fn close_synchronized_document(&mut self, original: &Uri) -> anyhow::Result<()> {
        let wire = self
            .snapshot_uris
            .as_mut()
            .and_then(|uris| uris.retire(original))
            .unwrap_or_else(|| original.clone());
        self.diagnostics.close(original.as_str());
        self.notify(
            "textDocument/didClose",
            json!({"textDocument":{"uri":wire}}),
        )
    }

    /// Publish one full source snapshot, retaining the versioned transport for other providers.
    pub(super) fn sync_document(&mut self, uri: Uri, source: String) -> anyhow::Result<String> {
        self.drain_messages()?;
        let Some(version) = self.diagnostics.next_version(uri.as_str(), &source) else {
            return Ok(self.wire_uri(&uri).as_str().into());
        };
        ensure!(
            version < i32::MAX,
            "LSP document version exhausted; restart service"
        );
        let wire = if let Some(uris) = &mut self.snapshot_uris {
            let candidate = uris.candidate(&uri, version)?;
            if let Some(old) = uris.retire(&uri) {
                self.notify("textDocument/didClose", json!({"textDocument":{"uri":old}}))?;
            }
            candidate
        } else {
            uri.clone()
        };
        if self.snapshot_uris.is_some() || !self.diagnostics.is_open(uri.as_str()) {
            self.notify("textDocument/didOpen", json!({
                "textDocument":{"uri":wire,"languageId":self.language_id,"version":version,"text":source}
            }))?;
        } else {
            self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument":{"uri":wire,"version":version},"contentChanges":[{"text":source}]
                }),
            )?;
        }
        if let Some(uris) = &mut self.snapshot_uris {
            uris.activate(uri.clone(), wire.clone(), version);
        }
        self.diagnostics
            .synchronized(uri.as_str().into(), source, version);
        Ok(wire.as_str().into())
    }

    /// Navigation and workspace edits expose physical/logical URIs, never the private wire alias.
    pub(super) fn logical_uri(&self, uri: &Uri) -> Uri {
        if let Some(original) = self
            .snapshot_uris
            .as_ref()
            .and_then(|uris| uris.owners.get(uri.as_str()))
        {
            return original.clone();
        }
        // Schema targets can inherit the dot prefix without being open document aliases.
        url::Url::parse(uri.as_str())
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .and_then(|path| file_uri(&path))
            .unwrap_or_else(|| uri.clone())
    }

    /// Attribute unversioned pushes only to a still-current immutable wire identity.
    pub(super) fn publish_diagnostics(&mut self, mut params: lsp_types::PublishDiagnosticsParams) {
        if let Some(uris) = &self.snapshot_uris {
            let Some((original, version)) = uris.publication(&params.uri) else {
                return;
            };
            if params.version.is_some_and(|published| published != version) {
                return;
            }
            params.uri = original;
            params.version = Some(version);
        }
        if params.version.is_some() {
            self.diagnostics.publish(params);
        }
    }
}
