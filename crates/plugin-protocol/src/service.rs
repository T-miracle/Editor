//! Versioned, bounded plugin collaboration contracts; provider identities are selected by the host.
use crate::api::{ErrorCode, Failure, ResourceHandle};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Schema types deliberately bound every collection and string before entering another component.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Schema {
    Null,
    Boolean,
    Integer {
        min: i64,
        max: i64,
    },
    String {
        max_bytes: usize,
    },
    Array {
        items: Box<Schema>,
        max_items: usize,
    },
    Record {
        fields: BTreeMap<String, Schema>,
        #[serde(default)]
        optional: BTreeSet<String>,
    },
}

impl Schema {
    /// Closed records reject undeclared fields rather than silently widening a contract.
    pub fn accepts(&self, value: &Value) -> Result<(), Failure> {
        let valid = match (self, value) {
            (Self::Null, Value::Null) | (Self::Boolean, Value::Bool(_)) => true,
            (Self::Integer { min, max }, Value::Number(number)) => {
                number.as_i64().is_some_and(|v| (*min..=*max).contains(&v))
            }
            (Self::String { max_bytes }, Value::String(text)) => text.len() <= *max_bytes,
            (Self::Array { items, max_items }, Value::Array(values)) => {
                values.len() <= *max_items
                    && values.iter().all(|value| items.accepts(value).is_ok())
            }
            (Self::Record { fields, optional }, Value::Object(values)) => {
                values.iter().all(|(key, value)| {
                    fields
                        .get(key)
                        .is_some_and(|schema| schema.accepts(value).is_ok())
                }) && fields
                    .keys()
                    .all(|key| optional.contains(key) || values.contains_key(key))
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Value does not match the declared service schema",
            ))
        }
    }
    /// Reject pathological schemas during package inspection, before payload validation can recurse.
    fn validate(&self, depth: usize) -> Result<(), String> {
        if depth > 8 {
            return Err("Service schema nesting exceeds 8".into());
        }
        match self {
            Self::String { max_bytes } if *max_bytes > 65536 => {
                Err("Service string quota exceeds 64 KiB".into())
            }
            Self::Integer { min, max } if min > max => Err("Invalid integer bounds".into()),
            Self::Array { items, max_items } => {
                if *max_items > 1024 {
                    return Err("Service array quota exceeds 1024".into());
                }
                items.validate(depth + 1)
            }
            Self::Record { fields, optional } => {
                if fields.len() > 32 || !optional.iter().all(|key| fields.contains_key(key)) {
                    return Err("Invalid service record fields".into());
                }
                for (key, schema) in fields {
                    if !identifier(key) {
                        return Err("Invalid service field name".into());
                    }
                    schema.validate(depth + 1)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Both sides declare identical method shapes and authority; matching a name alone grants nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Method {
    pub parameters: Schema,
    pub result: Schema,
    #[serde(default)]
    pub permissions: BTreeSet<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub version: Version,
    pub methods: BTreeMap<String, Method>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub version: VersionReq,
    #[serde(default)]
    pub optional: bool,
    pub methods: BTreeMap<String, Method>,
}

/// Native executable services remain a separate manifest field and permission family.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declarations {
    #[serde(default)]
    pub provides: BTreeMap<String, Contract>,
    #[serde(default)]
    pub requires: BTreeMap<String, Dependency>,
}
impl Declarations {
    pub fn validate(&self) -> Result<(), String> {
        if self.provides.len() + self.requires.len() > 32 {
            return Err("Too many plugin services".into());
        }
        for (id, methods) in self
            .provides
            .iter()
            .map(|(id, c)| (id, &c.methods))
            .chain(self.requires.iter().map(|(id, c)| (id, &c.methods)))
        {
            if !identifier(id) || !id.contains('.') || methods.is_empty() || methods.len() > 32 {
                return Err("Invalid service contract identity or methods".into());
            }
            for (name, method) in methods {
                if !identifier(name) || method.permissions.len() > 16 {
                    return Err("Invalid service method".into());
                }
                method.parameters.validate(0)?;
                method.result.validate(0)?;
                // Delegated authority has explicit v1 support; private provider handles cannot be borrowed.
                if method.permissions.iter().any(|p| {
                    !matches!(
                        p.as_str(),
                        "workspace.read"
                            | "editor.read"
                            | "editor.write"
                            | "ui.panels"
                            | "process.exec"
                    )
                }) {
                    return Err("Unsupported delegated service permission".into());
                }
            }
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > 256 * 1024 {
            return Err("Service declarations exceed 256 KiB".into());
        }
        Ok(())
    }
}

impl Dependency {
    /// A newer provider may add methods, but cannot reinterpret a consumer's declared method.
    pub fn matches(&self, contract: &Contract) -> bool {
        self.version.matches(&contract.version)
            && self
                .methods
                .iter()
                .all(|(id, method)| contract.methods.get(id) == Some(method))
    }
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// A host-issued reference pins one provider incarnation; switching providers invalidates old references.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Open {
        contract: String,
    },
    Call {
        reference: ResourceHandle,
        method: String,
        arguments: Value,
        timeout_ms: u32,
    },
    /// plugin.services 1.1 completes one deferred invocation using its host-issued provider handle.
    /// Result shape, source lifetime and deadline remain those of the original invocation.
    Reply {
        request: ResourceHandle,
        result: Result<Value, Failure>,
    },
}

/// Source and scope are host metadata, never accepted from consumer-provided arguments.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Caller {
    pub plugin: String,
    pub instance: String,
    pub scope: String,
    pub permissions: BTreeSet<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Invocation {
    pub caller: Caller,
    pub contract: String,
    pub method: String,
    pub arguments: Value,
    /// Available only with plugin.services 1.1. Omit the synchronous service_reply to defer, then
    /// complete this handle through Reply; closing it rejects the call without undoing side effects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<ResourceHandle>,
}

/// The same accepted/progress/completed/cancelled lifecycle is shared with editor requests.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Notification {
    Invoke(Invocation),
    /// plugin.services 1.1 retires the provider's deferred handle; no native rollback is implied.
    InvocationCancelled {
        request: ResourceHandle,
        reason: Failure,
    },
    Request {
        handle: ResourceHandle,
        update: crate::api::RequestUpdate<Value>,
    },
}

/// UI selection contains provider identities as data; guests cannot choose their own elevated provider.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub scope: crate::api::InstanceScope,
    pub contract: String,
    pub candidates: Vec<String>,
    pub selected: Option<String>,
    /// Explicit values distinguish automatic resolution from a saved choice in each settings layer.
    pub user: Option<String>,
    pub project: Option<String>,
}

/// The SDK hides envelope IDs while retaining the same cancellation contract as editor tasks.
#[cfg(feature = "guest")]
pub mod guest {
    use super::*;
    use crate::api::{self, CancelMode, CancellationEffect, RequestUpdate};
    /// Complete a deferred invocation from a later native or user event.
    /// InvalidHandle means the source, deadline or invocation already ended; invalid shapes retain
    /// the handle so the provider can return a corrected result or close it explicitly.
    pub fn reply(request: &ResourceHandle, result: Result<Value, Failure>) -> Result<(), Failure> {
        api::guest::request(api::Operation::Service {
            operation: Operation::Reply {
                request: request.clone(),
                result,
            },
        })
        .map(|_| ())
    }
    pub fn open(contract: impl Into<String>) -> Result<ResourceHandle, Failure> {
        match api::guest::request(api::Operation::Service {
            operation: Operation::Open {
                contract: contract.into(),
            },
        })? {
            api::Value::Resource(handle) => Ok(handle),
            _ => Err(Failure::new(
                ErrorCode::OperationFailed,
                "Expected service reference",
            )),
        }
    }
    pub struct Task {
        handle: ResourceHandle,
        terminal: bool,
    }
    impl Task {
        pub fn start(
            reference: &ResourceHandle,
            method: impl Into<String>,
            arguments: Value,
            timeout_ms: u32,
        ) -> Result<Self, Failure> {
            match api::guest::request(api::Operation::Service {
                operation: Operation::Call {
                    reference: reference.clone(),
                    method: method.into(),
                    arguments,
                    timeout_ms,
                },
            })? {
                api::Value::Accepted(handle) => Ok(Self {
                    handle,
                    terminal: false,
                }),
                _ => Err(Failure::new(
                    ErrorCode::OperationFailed,
                    "Expected accepted service request",
                )),
            }
        }
        /// Discard unrelated completions and preserve the first terminal outcome.
        pub fn update(&mut self, notification: &Notification) -> Option<RequestUpdate<Value>> {
            let Notification::Request { handle, update } = notification else {
                return None;
            };
            if self.terminal || *handle != self.handle {
                return None;
            }
            self.terminal = update.is_terminal();
            Some(update.clone())
        }
        pub fn cancel(&self, mode: CancelMode) -> Result<CancellationEffect, Failure> {
            api::guest::cancel_request(&self.handle, mode)
        }
    }
}
