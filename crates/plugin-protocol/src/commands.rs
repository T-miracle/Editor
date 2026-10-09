//! Stable typed command shapes and declarative native contribution points.
use crate::{
    api::{Failure, ResourceHandle},
    service::{Caller, Method},
};
use serde::{Deserialize, Serialize};

/// Native host context is informational; paths and revisions never grant implicit access.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub has_selection: bool,
    pub writable: bool,
    pub directory: bool,
    pub language: Option<String>,
    pub extension: Option<String>,
    /// Workspace-relative menu target, supplied by the host rather than by the plugin.
    pub path: Option<String>,
}

/// All specified fields must match. An omitted condition is always true and grants no permissions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub has_selection: Option<bool>,
    pub writable: Option<bool>,
    pub directory: Option<bool>,
    pub language: Option<String>,
    pub extension: Option<String>,
}

impl Condition {
    /// Re-evaluate against the live native target before invocation, as well as during menu rendering.
    pub fn matches(&self, context: &Context) -> bool {
        self.has_selection
            .is_none_or(|value| value == context.has_selection)
            && self.writable.is_none_or(|value| value == context.writable)
            && self
                .directory
                .is_none_or(|value| value == context.directory)
            && self
                .language
                .as_ref()
                .is_none_or(|value| context.language.as_ref() == Some(value))
            && self
                .extension
                .as_ref()
                .is_none_or(|value| context.extension.as_ref() == Some(value))
    }
}

/// Stable native contribution locations. Selection is an editor menu filtered by selection presence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Location {
    Editor,
    Selection,
    Explorer,
    Tab,
}

/// Group and order are presentation metadata; admission always rechecks the current instance.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Menu {
    pub location: Location,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub when: Condition,
    #[serde(default)]
    pub enabled_when: Condition,
    /// Static menu parameters obey the same typed schema as all other command callers.
    #[serde(default)]
    pub arguments: serde_json::Value,
}

impl crate::Command {
    /// Bound every schema and native menu condition before a package can become discoverable.
    pub fn validate(&self) -> Result<(), String> {
        if self.title.is_empty() || self.title.len() > 256 || self.menus.len() > 16 {
            return Err("Invalid command title or menu quota".into());
        }
        if let Some(method) = &self.signature {
            method.parameters.validate(0)?;
            method.result.validate(0)?;
            if method.permissions.len() > 16 {
                return Err("Command permission quota exceeded".into());
            }
        }
        for menu in &self.menus {
            if serde_json::to_vec(&menu.arguments).map_or(true, |bytes| bytes.len() > 65536)
                || self
                    .signature
                    .as_ref()
                    .is_some_and(|signature| signature.parameters.accepts(&menu.arguments).is_err())
            {
                return Err(
                    "Native menu arguments do not match the command schema or payload budget"
                        .into(),
                );
            }
            if menu.group.len() > 64
                || [&menu.when, &menu.enabled_when].iter().any(|condition| {
                    condition
                        .language
                        .as_ref()
                        .is_some_and(|value| value.len() > 128)
                        || condition
                            .extension
                            .as_ref()
                            .is_some_and(|value| value.len() > 32)
                })
            {
                return Err("Invalid native menu group or condition".into());
            }
        }
        Ok(())
    }
}

/// The discovered contract never implies the caller has its required permissions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Descriptor {
    pub plugin: String,
    pub command: String,
    pub signature: Method,
}

/// Invocation is queued and returned as an owned request; discovery executes no provider code.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Discover,
    Invoke {
        plugin: String,
        command: String,
        arguments: serde_json::Value,
        timeout_ms: u32,
    },
    Reply {
        request: ResourceHandle,
        result: Result<serde_json::Value, Failure>,
    },
}

/// The host supplies source identity and the provider's own reply handle, never borrowed authority.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Invocation {
    pub id: String,
    pub arguments: serde_json::Value,
    /// Native target metadata is descriptive only; the arguments keep their declared schema.
    #[serde(default)]
    pub context: Option<Context>,
    pub caller: Caller,
    pub reply: ResourceHandle,
}

/// Commands reuse the established service completion gate and authority intersection.
pub const CONTRACT_PREFIX: &str = "nanobug.command.";

/// Reserved transport identity is derived only from package-validated metadata.
pub fn contract(plugin: &str, command: &str) -> String {
    format!("{CONTRACT_PREFIX}{plugin}/{command}")
}

#[cfg(feature = "guest")]
/// Discover the live typed commands in this caller's negotiated scope, without provider execution.
pub fn discover() -> Result<Vec<Descriptor>, Failure> {
    match crate::api::guest::request(crate::api::Operation::Commands {
        operation: Operation::Discover,
    })? {
        crate::api::Value::Commands(commands) => Ok(commands),
        _ => Err(Failure::new(
            crate::api::ErrorCode::InvalidRequest,
            "Unexpected command metadata result",
        )),
    }
}

#[cfg(feature = "guest")]
/// Invoke an explicit plugin command with schema-checked arguments and an instance-owned wait.
pub fn invoke(
    plugin: impl Into<String>,
    command: impl Into<String>,
    arguments: serde_json::Value,
    timeout_ms: u32,
) -> Result<ResourceHandle, Failure> {
    match crate::api::guest::request(crate::api::Operation::Commands {
        operation: Operation::Invoke {
            plugin: plugin.into(),
            command: command.into(),
            arguments,
            timeout_ms,
        },
    })? {
        crate::api::Value::Accepted(handle) => Ok(handle),
        _ => Err(Failure::new(
            crate::api::ErrorCode::InvalidRequest,
            "Unexpected command acceptance",
        )),
    }
}

#[cfg(feature = "guest")]
/// Finish only the provider's retained reply slot; repeated, expired and foreign replies are rejected.
pub fn reply(
    request: ResourceHandle,
    result: Result<serde_json::Value, Failure>,
) -> Result<(), Failure> {
    crate::api::guest::request(crate::api::Operation::Commands {
        operation: Operation::Reply { request, result },
    })
    .map(|_| ())
}

#[cfg(feature = "guest")]
/// Guest bookkeeping for one request; ownership and immutable final state remain enforced by the host.
pub struct Task {
    handle: ResourceHandle,
    terminal: bool,
}

#[cfg(feature = "guest")]
impl Task {
    /// Start a bounded typed command and retain only this caller's wait handle.
    pub fn start(
        plugin: impl Into<String>,
        command: impl Into<String>,
        arguments: serde_json::Value,
        timeout_ms: u32,
    ) -> Result<Self, Failure> {
        Ok(Self {
            handle: invoke(plugin, command, arguments, timeout_ms)?,
            terminal: false,
        })
    }
    /// The host-issued wait can be cancelled or released; it is never the provider's reply slot.
    pub fn handle(&self) -> &ResourceHandle {
        &self.handle
    }
    /// Ignore other requests and repeated late results after this task's first terminal update.
    pub fn update(
        &mut self,
        event: &crate::api::Notification,
    ) -> Option<crate::api::RequestUpdate<serde_json::Value>> {
        let crate::api::Notification::CommandRequest { handle, update } = event else {
            return None;
        };
        if self.terminal || handle != &self.handle {
            return None;
        }
        self.terminal = update.is_terminal();
        Some(update.clone())
    }
    /// Stop this wait through the same correlated host cancellation contract as editor tasks.
    pub fn cancel(
        &self,
        mode: crate::api::CancelMode,
    ) -> Result<crate::api::CancellationEffect, Failure> {
        crate::api::guest::cancel_request(&self.handle, mode)
    }
}
