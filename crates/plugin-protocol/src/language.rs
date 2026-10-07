//! Language-service declarations and bounded optional hook data, independent of concrete language names.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// language.lsp 1.3 inserts this fixed number of dot directories at the native volume root.
/// Their percent-encoding case carries an immutable wire identity without creating a file.
/// Native plugins with absolute path globs may add a second pattern containing these dot directories.
pub const SNAPSHOT_URI_SEGMENTS: usize = 64;

/// language.editing 1.1 reserves this client/server experimental capability key for semantic pairing.
/// Only the host's negotiated grant can advertise it; provider client_experimental cannot override it.
pub const SEMANTIC_LINKED_EDITING_CAPABILITY: &str = "meEditorSemanticLinkedEditing";

/// This fixed request uses LinkedEditingRangeParams and LinkedEditingRanges over the owned LSP transport.
/// Unlike the standard method, its provider guarantees semantic pairing of initially different names.
pub const SEMANTIC_LINKED_EDITING_METHOD: &str = "meEditor/semanticLinkedEditingRange";

/// Version one permits different initial text/lengths while retaining UTF-16 boundaries and name patterns.
pub const SEMANTIC_LINKED_EDITING_VERSION: u32 = 1;

/// Strict initialization marker for the generic semantic linked-editing extension.
/// Both sides must advertise the supported version before the host selects the extension method.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SemanticLinkedEditingCapabilities {
    /// Semantic request version; currently only SEMANTIC_LINKED_EDITING_VERSION is supported.
    pub version: u32,
}

/// Recognition, highlighting, and this LSP provider may be contributed by different packages.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub language: String,
    pub service: String,
    /// Main analysis participates in lsp selection; a formatting-only service sets this to false.
    #[serde(default = "default_primary")]
    pub primary: bool,
    /// language.formatting 1.0: contribute to the independently selected formatter candidates.
    #[serde(default)]
    pub formatting: bool,
    /// language.editing: expose standard prepareRename/rename/linkedEditingRange to native input.
    /// Version 1.1 also allows the separately negotiated generic semantic linked-editing method.
    /// Pairing remains provider policy; this declaration grants neither file IO nor additional processes.
    #[serde(default)]
    pub editing: bool,
    /// A hook may select another approved startup plan, not invent an executable or argument vector.
    #[serde(default)]
    pub alternatives: Vec<String>,
    /// Only an explicitly saved user/project value overrides native executable resolution.
    #[serde(default)]
    pub executable_setting: Option<String>,
    #[serde(default)]
    pub hook: bool,
    /// dependencies 1.1: first installation may activate resource features after a failed service download.
    /// Updates still require successful preparation; the service remains unavailable until its cache is ready.
    #[serde(default)]
    pub optional_installation: bool,
    /// language.completion: a pure snapshot callback supplements native suggestions without owning text.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub completion_hook: bool,
    /// language.lsp 1.3: use immutable file URI aliases for services publishing unversioned diagnostics.
    /// Each text change closes the prior wire document; logical document identity and disk stay unchanged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub diagnostic_snapshots: bool,
    #[serde(default)]
    pub initialization_options: Value,
    /// Section names are opaque protocol data; the host never prefixes a concrete server name.
    #[serde(default)]
    pub configuration: BTreeMap<String, Value>,
    /// Experimental protocol flags are plugin data; standard and reserved transport capabilities stay host-owned.
    #[serde(default)]
    pub client_experimental: BTreeMap<String, Value>,
    #[serde(default)]
    pub readiness: Option<Readiness>,
    #[serde(default)]
    pub completion_triggers: Vec<String>,
    #[serde(default)]
    pub completion_after_whitespace: Vec<String>,
}

/// Existing current-protocol declarations retain their ordinary analysis role.
fn default_primary() -> bool {
    true
}

/// Without this optional condition, a successful initialize response plus initialized means ready.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Readiness {
    pub notification: String,
    /// RFC 6901 pointer into notification params.
    pub pointer: String,
    pub expected: Value,
    pub timeout_ms: u32,
}

/// Hooks see resolved settings and fixed candidate declarations within one trusted workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Context {
    pub provider: String,
    pub workspace: String,
    /// language.lsp 1.2: canonical assets owned by this package version, for native service configuration.
    /// This string grants no WASI filesystem access; native processes do not inherit the WASM sandbox.
    #[serde(default)]
    pub package_root: String,
    /// language.lsp 1.2: the current isolated private-data directory (including an unpublished candidate).
    /// The host refreshes it after transactional cutover; plugins must not save this location as identity.
    #[serde(default)]
    pub data_root: String,
    pub settings: crate::settings::Effective,
    pub candidates: BTreeMap<String, crate::process::Service>,
}

/// Omitted fields retain declarations. Project roots remain relative to the owning workspace.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    /// Dynamic download plans require the separate dependencies.prepare installation grant.
    pub installation: Option<crate::dependencies::Plan>,
    /// Dynamic executables/arguments require the separate process.exec grant; fixed plans do not.
    pub program: Option<String>,
    pub args: Option<Vec<String>>,
    pub service: Option<String>,
    pub project_root: Option<String>,
    pub initialization_options: Option<Value>,
    pub configuration: Option<BTreeMap<String, Value>>,
}

/// A readonly snapshot is tied to the editor incarnation and revision; it is never a document handle.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSnapshot {
    pub document: crate::api::DocumentVersion,
    /// At most 1 MiB of UTF-8; the native editor remains the sole mutable text owner.
    pub text: String,
}

impl SourceSnapshot {
    /// Bounded metadata and text are checked before guest execution or accepting an asynchronous result.
    pub fn valid(&self) -> bool {
        self.text.len() <= 1024 * 1024
            && crate::api::FileVersion {
                id: self.document.id.clone(),
                path: self.document.path.clone(),
                revision: self.document.revision,
            }
            .validate()
            .is_ok()
    }
}

/// Explicit opt-in supplements run in a separate pure guest, without initialization side effects or IO.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionRequest {
    pub request: u64,
    pub provider: String,
    pub source: SourceSnapshot,
    /// UTF-8 byte caret position in exactly `source.text`.
    pub cursor: usize,
    /// The logical document URI, before native transport aliases; grants no file or external URL access.
    pub uri: String,
    /// Standard native diagnostics proven to belong to this exact source; None means not known yet.
    /// Language policy may use the code/message to distinguish unavailable rule sources from content errors.
    pub diagnostics: Option<Vec<CompletionDiagnostic>>,
    pub settings: crate::settings::Effective,
}

/// A bounded standard diagnostic summary, with no executable action or server-specific host interpretation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionDiagnostic {
    /// Standard LSP codes are strings or integers; the guest owns their domain meaning.
    pub code: Option<Value>,
    pub message: String,
}

impl CompletionRequest {
    /// Source authority, cursor boundaries and native summaries are checked before invoking the pure guest.
    pub fn valid(&self) -> bool {
        self.source.valid()
            && self.cursor <= self.source.text.len()
            && self.source.text.is_char_boundary(self.cursor)
            && !self.uri.is_empty()
            && self.uri.len() <= 8192
            && !self.uri.chars().any(char::is_control)
            && self.diagnostics.as_ref().is_none_or(|items| {
                items.len() <= 128
                    && items.iter().all(|item| {
                        item.message.len() <= 1024
                            && item.code.as_ref().is_none_or(|code| match code {
                                Value::String(value) => value.len() <= 128,
                                Value::Number(value) => value.is_i64() || value.is_u64(),
                                _ => false,
                            })
                    })
            })
    }
}

/// Plain text replacement; there are no commands, extra edits or mutable guest-owned selections.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionItem {
    pub label: String,
    pub replace: crate::api::TextRange,
    pub new_text: String,
}

/// Native items keep precedence; supplements are bounded and tied to their exact source/request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionProposal {
    pub request: u64,
    pub document: crate::api::DocumentVersion,
    pub items: Vec<CompletionItem>,
}

impl CompletionProposal {
    /// Reject the whole reply on stale metadata, invalid UTF-8 ranges or excessive response work.
    pub fn valid_for(&self, request: &CompletionRequest) -> bool {
        self.request == request.request
            && self.document == request.source.document
            && self.items.len() <= 256
            && self.items.iter().all(|item| {
                !item.label.is_empty()
                    && item.label.len() <= 256
                    && item.new_text.len() <= 4096
                    && item.replace.start <= request.cursor
                    && request.cursor <= item.replace.end
                    && item.replace.end <= request.source.text.len()
                    && request.source.text.is_char_boundary(item.replace.start)
                    && request.source.text.is_char_boundary(item.replace.end)
            })
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 256 * 1024)
    }
}

impl Provider {
    /// Package inspection rejects unbounded protocol data before invoking a guest or starting native code.
    pub fn valid(&self) -> bool {
        let id = |text: &str| {
            !text.is_empty()
                && text.len() <= 100
                && text.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
                })
        };
        id(&self.id)
            && id(&self.language)
            && id(&self.service)
            && self.alternatives.len() <= 32
            && self.alternatives.iter().all(|value| id(value))
            && self.configuration.len() <= 64
            && self.configuration.keys().all(|key| key.len() <= 256)
            && self.client_experimental.len() <= 64
            && self
                .client_experimental
                .iter()
                .all(|(key, value)| !key.is_empty() && key.len() <= 256 && bounded_json(value, 0))
            && self.readiness.as_ref().is_none_or(|ready| {
                !ready.notification.is_empty()
                    && ready.notification.len() <= 256
                    && (ready.pointer.is_empty() || ready.pointer.starts_with('/'))
                    && ready.pointer.len() <= 256
                    && (1..=300_000).contains(&ready.timeout_ms)
            })
            && self.completion_triggers.len() <= 64
            && self.completion_after_whitespace.len() <= 64
            && self
                .completion_triggers
                .iter()
                .chain(&self.completion_after_whitespace)
                .all(|value| value.len() <= 256)
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 256 * 1024)
    }
}

/// The overall encoded budget limits breadth; a separate depth budget prevents pathological nested flags.
fn bounded_json(value: &Value, depth: usize) -> bool {
    depth <= 16
        && match value {
            Value::Array(values) => values.iter().all(|value| bounded_json(value, depth + 1)),
            Value::Object(values) => values.values().all(|value| bounded_json(value, depth + 1)),
            _ => true,
        }
}
