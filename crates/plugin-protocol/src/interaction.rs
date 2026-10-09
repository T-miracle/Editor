//! Stable native interactions: cancellation is a terminal result, never an empty successful choice.
use crate::api::{ErrorCode, Failure};
use serde::{Deserialize, Serialize};

/// A native picker grants only the selected object and its declared access intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    File,
    Directory,
    Save,
}

/// The display name is informational; authority resides in the selecting instance's opaque slot.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedResource {
    pub handle: crate::api::ResourceHandle,
    pub name: String,
    pub kind: SelectionMode,
}

/// Severity affects native appearance, never the plugin's installation permissions.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Information,
    Warning,
    Error,
}

/// IDs are the returned values; presentation labels may be translated independently.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// A host interaction always runs under its owning plugin's original request lifetime.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Save selection obtains an exact intent; it does not write or overwrite the chosen file.
    Select {
        title: String,
        mode: SelectionMode,
        multiple: bool,
        suggested_name: Option<String>,
    },
    Confirm {
        title: String,
        message: String,
    },
    /// Nonmodal attributed notice. Its result is delivered when the user dismisses it.
    Notify {
        title: String,
        message: String,
        severity: Severity,
    },
    /// A nonmodal live task retains its pending request until finish, cancel or expiry.
    Progress {
        title: String,
        message: String,
        cancellable: bool,
    },
    /// Updates coalesce on the existing owned task; no second UI request is allocated.
    UpdateProgress {
        request: crate::api::ResourceHandle,
        message: String,
        percent: Option<u8>,
    },
    FinishProgress {
        request: crate::api::ResourceHandle,
    },
    QuickPick {
        title: String,
        items: Vec<PickItem>,
    },
    /// Password presentation never changes the bounded string result; callers must not log it.
    Input {
        title: String,
        value: String,
        placeholder: Option<String>,
        password: bool,
        max_bytes: usize,
    },
}

/// Successful native results never encode cancellation as an empty string or false selection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Value {
    Selected(Vec<SelectedResource>),
    Confirmed,
    Dismissed,
    Finished,
    Picked(String),
    Input(String),
}

impl Operation {
    /// Validate transport bounds and unique identities before allocating a native request.
    pub fn validate(&self) -> Result<(), Failure> {
        match self {
            Self::Select {
                title,
                mode,
                multiple,
                suggested_name,
            } => {
                if title.is_empty()
                    || title.len() > 256
                    || (*mode == SelectionMode::Save && *multiple)
                    || suggested_name.as_ref().is_some_and(|name| {
                        name.is_empty()
                            || name.len() > 255
                            || name.contains(['/', '\\', ':', '\0'])
                            || name == "."
                            || name == ".."
                    })
                {
                    return Err(Failure::new(
                        ErrorCode::InvalidRequest,
                        "Invalid native selection title, mode or basename",
                    ));
                }
                return Ok(());
            }
            Self::Confirm { title, message }
            | Self::Notify { title, message, .. }
            | Self::Progress { title, message, .. } => {
                if title.is_empty() || title.len() > 256 || message.len() > 4096 {
                    return Err(Failure::new(
                        ErrorCode::InvalidRequest,
                        "Invalid interaction title or message",
                    ));
                }
                return Ok(());
            }
            Self::UpdateProgress {
                message, percent, ..
            } => {
                if message.len() > 4096 || percent.is_some_and(|value| value > 100) {
                    return Err(Failure::new(
                        ErrorCode::InvalidRequest,
                        "Invalid progress message or percentage",
                    ));
                }
                return Ok(());
            }
            Self::FinishProgress { .. } => return Ok(()),
            _ => {}
        }
        if let Self::Input {
            title,
            value,
            placeholder,
            max_bytes,
            ..
        } = self
        {
            if title.is_empty()
                || title.len() > 256
                || *max_bytes == 0
                || *max_bytes > 65536
                || value.len() > *max_bytes
                || placeholder.as_ref().is_some_and(|text| text.len() > 256)
            {
                return Err(Failure::new(
                    ErrorCode::InvalidRequest,
                    "Invalid input title or byte budget",
                ));
            }
            return Ok(());
        }
        let Self::QuickPick { title, items } = self else {
            unreachable!()
        };
        let mut ids = std::collections::BTreeSet::new();
        if title.is_empty()
            || title.len() > 256
            || items.is_empty()
            || items.len() > 512
            || items.iter().any(|item| {
                item.id.is_empty()
                    || item.id.len() > 128
                    || !ids.insert(&item.id)
                    || item.label.is_empty()
                    || item.label.len() > 256
                    || item
                        .description
                        .as_ref()
                        .is_some_and(|text| text.len() > 1024)
            })
        {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Invalid quick-pick title, items or duplicate identity",
            ));
        }
        Ok(())
    }
}

#[cfg(feature = "guest")]
/// Start a native interaction; confirmation and cancellation arrive through Notification::Request.
pub fn start(operation: Operation, timeout_ms: u32) -> Result<crate::api::ResourceHandle, Failure> {
    crate::api::guest::editor(
        crate::api::EditorOperation::Interaction { operation },
        timeout_ms,
    )
}

#[cfg(feature = "guest")]
/// Replace the visible progress under an owned pending task; updates coalesce without new UI slots.
pub fn update(
    request: crate::api::ResourceHandle,
    message: impl Into<String>,
    percent: Option<u8>,
) -> Result<(), Failure> {
    crate::api::guest::request(crate::api::Operation::Editor {
        operation: crate::api::EditorOperation::Interaction {
            operation: Operation::UpdateProgress {
                request,
                message: message.into(),
                percent,
            },
        },
        timeout_ms: 30000,
    })
    .map(|_| ())
}

#[cfg(feature = "guest")]
/// Complete a live progress task. A user cancellation wins over this late finish.
pub fn finish(request: crate::api::ResourceHandle) -> Result<(), Failure> {
    crate::api::guest::request(crate::api::Operation::Editor {
        operation: crate::api::EditorOperation::Interaction {
            operation: Operation::FinishProgress { request },
        },
        timeout_ms: 30000,
    })
    .map(|_| ())
}
