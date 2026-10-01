//! Independently versioned capability contracts carried over the component transport.
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Maximum encoded host request size; SDKs reject oversized requests before transport.
pub const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;

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
    InvalidRequest,
    UnsupportedOperation,
    CapabilityUnavailable,
    PermissionDenied,
    InvalidState,
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

/// Typed resource operations expand per capability, not per consuming plugin.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    ReadAsset { path: String },
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
    Asset { bytes: Vec<u8> },
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

/// A native view is separate from canvas and character-grid data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    pub panel: String,
    pub document: crate::ui::Document,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
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
