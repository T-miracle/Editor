//! Independently versioned capability contracts carried over the component transport.
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod navigation;
pub use navigation::{
    NavigationTarget, decode_uri_component, document_relative_path, is_windows_device_segment,
};

#[cfg(test)]
mod images_tests;

/// Maximum encoded host request size; SDKs reject oversized requests before transport.
pub const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;

/// Workspace is the default lifetime; application guests never inherit a current project root.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceScope {
    #[default]
    Workspace,
    Application,
}

/// Opaque host-issued identity binds every resource to one instance and logical scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceHandle {
    pub instance: String,
    pub scope: String,
    pub resource: u64,
}

/// Formats accepted by native image input; the host identifies encoded content before issuing a handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
    Svg,
}

impl ImageFormat {
    /// Canonical suffix without a dot, used to validate a caller-chosen document-sibling name.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Svg => "svg",
        }
    }
}

/// Metadata for a 30-second, instance-owned input. Encoded pixels never cross the 2 MiB JSON transport.
/// Batches contain at most eight images, each at most 8 MiB and together at most 32 MiB.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageInput {
    pub handle: ResourceHandle,
    pub format: ImageFormat,
    pub byte_len: u64,
}

/// Compatibility ranges describe API requirements, never the plugin package version.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirements {
    pub base: VersionReq,
    #[serde(default)]
    pub required: BTreeMap<String, VersionReq>,
    #[serde(default)]
    pub optional: BTreeMap<String, VersionReq>,
}

/// Only interfaces actually negotiated are callable by this instance.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Negotiated {
    pub base: Version,
    pub capabilities: BTreeMap<String, Version>,
}

impl Requirements {
    /// Missing optional interfaces are omitted; required interfaces fail before activation.
    pub fn negotiate(
        &self,
        base: Version,
        available: &BTreeMap<String, Version>,
    ) -> Result<Negotiated, String> {
        if !self.base.matches(&base) {
            return Err(format!(
                "Unsupported base API: requires {}, host {base}",
                self.base
            ));
        }
        if self.required.len() + self.optional.len() > 128 {
            return Err("Too many capability requirements".into());
        }
        let mut capabilities = BTreeMap::new();
        for (id, range) in self.required.iter().chain(&self.optional) {
            if id.is_empty() || id.len() > 128 {
                return Err("Invalid capability identity".into());
            }
            if self.required.contains_key(id) && self.optional.contains_key(id) {
                return Err(format!("Capability is both required and optional: {id}"));
            }
            if let Some(version) = available.get(id).filter(|version| range.matches(version)) {
                capabilities.insert(id.clone(), version.clone());
            } else if self.required.contains_key(id) {
                return Err(format!("Required capability unavailable: {id} {range}"));
            }
        }
        Ok(Negotiated { base, capabilities })
    }
}

/// Stable failure categories allow callers to react without parsing human-readable messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Cancelled,
    TimedOut,
    StaleRevision,
    Conflict,
    InvalidRequest,
    UnsupportedOperation,
    CapabilityUnavailable,
    PermissionDenied,
    InvalidState,
    InvalidHandle,
    InvalidPath,
    NotFound,
    LimitExceeded,
    OperationFailed,
}

/// A failure is data; only a broken component transport uses the WIT string error.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Failure {
    pub code: ErrorCode,
    pub message: String,
}

impl Failure {
    /// Preserve a machine-readable cause alongside the explanation shown to the user.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for Failure {}

/// Workspace-relative glob inputs are data; no filesystem path expansion is implied.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileQuery {
    /// Root-relative globs; `**/` also matches zero directory levels.
    pub include: Vec<String>,
    /// Excluded subtrees are pruned before their children are inspected.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// The host additionally bounds traversal work and encoded response size.
    pub max_results: u32,
}

/// A bounded discovery result contains workspace-relative paths, never new file authority.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileMatches {
    pub paths: Vec<String>,
    /// Unreadable branches are visible to the caller instead of silently claiming completeness.
    pub skipped: Vec<String>,
}

/// Public native toolchain inputs; these paths cannot be used as workspace file handles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdkDescriptor {
    /// Content identity of the exact SDK compiled into this host.
    pub digest: String,
    /// Absolute native path for external interface tools, with no WASI filesystem grant.
    pub root: String,
    /// The same Cargo override used by the public --plugin-cargo entry point.
    pub cargo_config: String,
}

/// Typed resource operations expand per capability, not per consuming plugin.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Service {
        operation: crate::service::Operation,
    },
    Process {
        operation: crate::process::Operation,
    },
    SubscribeDocuments,
    CancelRequest {
        handle: ResourceHandle,
        mode: CancelMode,
    },
    /// Editor calls are accepted on the worker and completed on the owning editor thread.
    Editor {
        operation: EditorOperation,
        timeout_ms: u32,
    },
    ReadAsset {
        path: String,
    },
    OpenWorkspace,
    /// Requires workspace.files 1.1 and the workspace handle's existing read permission.
    FindFiles {
        handle: ResourceHandle,
        query: FileQuery,
    },
    /// Describe a host-owned SDK without accepting a guest-chosen native path.
    DescribeSdk,
    OpenData,
    ReadFile {
        handle: ResourceHandle,
        path: String,
    },
    WriteFile {
        handle: ResourceHandle,
        path: String,
        bytes: Vec<u8>,
    },
    CloseResource {
        handle: ResourceHandle,
    },
}

/// Nonzero IDs correlate synchronous responses and leave room for future asynchronous calls.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: u64,
    pub operation: Operation,
}

/// Result variants carry structured values, never JSON hidden inside a string result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Value {
    Files(FileMatches),
    Sdk(SdkDescriptor),
    Process(crate::process::Update),
    Cancellation(CancellationEffect),
    Accepted(ResourceHandle),
    Asset { bytes: Vec<u8> },
    Resource(ResourceHandle),
    Bytes(Vec<u8>),
    Unit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub id: u64,
    pub result: Result<Value, Failure>,
}

/// Base lifecycle messages supply the negotiated interfaces before plugin activation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Input {
    Prepare {
        environment: crate::Environment,
        api: Negotiated,
        snapshot: Option<crate::Snapshot>,
    },
    Activate,
    Event {
        panel: Option<String>,
        event: Notification,
    },
    Snapshot,
}

/// Native UI notifications contain no legacy canvas or character-grid fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Notification {
    /// User-initiated native input belongs to this exact source version and UTF-8 selection.
    /// Only the authorized active workspace preview receives the ordered metadata batch.
    ImageInput {
        document: DocumentVersion,
        selection: TextRange,
        images: Vec<ImageInput>,
    },
    /// Only the isolated private-data root and package assets are available during this callback.
    MigrateData {
        from: u32,
        to: u32,
        snapshot: Option<crate::Snapshot>,
    },
    Service(crate::service::Notification),
    /// An authorized preview receives the current unsaved text with the open-document identity/version.
    Preview {
        document: Option<DocumentVersion>,
        text: String,
    },
    /// Prepare one host-managed LSP; the reply supplies data, never a process handle.
    LanguageService(crate::language::Context),
    Process {
        handle: ResourceHandle,
        update: crate::process::Update,
    },
    Configuration {
        phase: crate::settings::Phase,
        values: crate::settings::Effective,
    },
    Document {
        subscription: ResourceHandle,
        change: DocumentChange,
    },
    SubscriptionFailed {
        subscription: ResourceHandle,
        error: Failure,
    },
    Request {
        handle: ResourceHandle,
        update: RequestUpdate,
    },
    Ui(crate::ui::UiEvent),
    Theme(crate::Environment),
    Command {
        id: String,
        arguments: Option<serde_json::Value>,
    },
    Focus(bool),
    Resize {
        width: f32,
        height: f32,
    },
}

/// An open-document identity plus a revision prevents delayed work from targeting a reopened file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentVersion {
    pub id: String,
    pub path: String,
    pub revision: u64,
}

/// Half-open UTF-8 byte offsets in one explicitly versioned document.
/// Empty ranges represent carets; the host checks actual text length and character boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextRange {
    /// Inclusive byte offset.
    pub start: usize,
    /// Exclusive byte offset, greater than or equal to `start`.
    pub end: usize,
}

/// Notifications carry versions, not text deltas; intermediate revisions may be coalesced safely.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentChange {
    pub document: DocumentVersion,
    pub closed: bool,
}

/// These operations are capability contracts; no plugin-specific command parser participates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditorOperation {
    /// Navigate from the exact active source version. Requires `editor.navigation` and `editor.read`;
    /// relative documents additionally require `workspace.read`, external URLs `navigation.external`.
    NavigateDocument {
        document: DocumentVersion,
        target: NavigationTarget,
    },
    /// Create an input's bytes beside its bound source document, without overwriting an existing file.
    /// Requires `editor.images`, `editor.read`, `editor.write` and `workspace.write`.
    /// `name` is a basename with the resource's canonical suffix. Conflict retains the input for retry;
    /// successful completion consumes it. No reference edit or file deletion is implied.
    SaveImageInput {
        input: ResourceHandle,
        name: String,
    },
    /// Clipboard calls are separately negotiated and authorized; they use the same asynchronous completion gate.
    ReadClipboard,
    WriteClipboard {
        text: String,
    },
    /// Open an existing file below this instance's private data root without exposing its host layout.
    OpenDataFile {
        path: String,
    },
    ReadSelection,
    /// Read the selection of this exact document/version without following toolbar or editor focus.
    /// Requires `editor.edit` and `editor.read` in the requesting workspace instance.
    ReadDocumentSelection {
        document: DocumentVersion,
    },
    /// Replace one source range atomically and set the selection in the resulting complete text.
    /// Requires `editor.edit` and `editor.write`; replacement text is bounded to 1 MiB.
    /// Optional expected selection rejects a toolbar action whose original selection has changed.
    ReplaceDocumentRange {
        document: DocumentVersion,
        range: TextRange,
        text: String,
        selection: TextRange,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_selection: Option<TextRange>,
    },
    ActiveDirectory,
    SaveDocument {
        document: DocumentVersion,
    },
    SetPanelVisibility {
        panel: String,
        visible: bool,
    },
}

/// Values describe the actual document and revision observed or saved, rather than an acknowledgement.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EditorValue {
    /// Actual identity/version opened by a controlled relative navigation, for optional follow-up anchors.
    Opened {
        document: DocumentVersion,
    },
    /// The complete file was created. `name` is relative to this original document's directory.
    /// A later source change cannot turn this receipt into permission to edit another document.
    ImageSaved {
        input: ResourceHandle,
        document: DocumentVersion,
        name: String,
    },
    Clipboard {
        text: String,
    },
    /// An effect completed successfully without a document result.
    Unit,
    Selection {
        document: DocumentVersion,
        text: String,
    },
    /// Actual source selection observed at the requested version, expressed as UTF-8 bytes.
    DocumentSelection {
        document: DocumentVersion,
        range: TextRange,
        text: String,
    },
    /// The resulting document version and full-text selection after one atomic editor transaction.
    Edited {
        document: DocumentVersion,
        selection: TextRange,
    },
    Directory {
        path: String,
    },
    Saved {
        document: DocumentVersion,
    },
    PanelVisibility {
        panel: String,
        visible: bool,
    },
}

/// Progress is replaceable; final results are retained until delivered to the owning instance.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RequestUpdate<T = EditorValue> {
    Accepted,
    Progress {
        message: String,
    },
    Completed {
        result: Result<T, Failure>,
    },
    Cancelled {
        reason: ErrorCode,
        effect: CancellationEffect,
    },
}

/// Best effort may stop only waiting once an atomic save has entered its irreversible section.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelMode {
    StopWaiting,
    TryTerminate,
}

/// Cancellation is never a claim that a completed filesystem side effect was rolled back.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationEffect {
    NotExecuted,
    WaitingStopped,
}

impl<T> RequestUpdate<T> {
    /// Terminal states are immutable, including after timeout or instance shutdown.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Cancelled { .. })
    }
}

/// One declared panel's document may compose native controls, canvases and optional grids.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    pub panel: String,
    pub document: crate::ui::Document,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    /// Only a service invocation may return this contract-validated result.
    #[serde(default)]
    pub service_reply: Option<Result<serde_json::Value, Failure>>,
    #[serde(default)]
    pub language_service: Option<crate::language::Proposal>,
    #[serde(default)]
    pub configuration: Option<crate::settings::Proposal>,
    #[serde(default)]
    pub views: Vec<View>,
    pub snapshot: Option<crate::Snapshot>,
}

/// Host lifecycle invocations use the same correlation rule as resource requests.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub id: u64,
    pub message: Input,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    pub id: u64,
    pub result: Result<Output, Failure>,
}

#[cfg(feature = "guest")]
pub mod guest;
