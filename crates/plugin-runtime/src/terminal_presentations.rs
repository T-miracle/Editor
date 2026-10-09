//! Bounded projection of existing, authenticated PTYs into the host terminal; no spawning or DAP policy.

use plugin_protocol::{
    api::{ErrorCode, Failure, ResourceHandle},
    process::Update,
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
};

/// Immutable host invocation metadata associates a projection with an already admitted task round.
#[derive(Clone, Debug)]
pub struct TerminalOwner {
    /// Host invocation scope also constrains resources created by an application-scoped provider.
    pub scope: String,
    pub configuration: Option<String>,
    pub invocation: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An application provider's host-owned invocation remains private to its original workspace.
    #[test]
    fn application_resources_keep_invocation_scope_and_final_error() {
        let registry = TerminalPresentations::default();
        let handle = ResourceHandle {
            instance: "provider".into(),
            scope: "application".into(),
            resource: 1,
        };
        registry
            .present(
                &handle,
                "Task A".into(),
                Some(TerminalOwner {
                    scope: "workspace-a".into(),
                    configuration: Some("task".into()),
                    invocation: "start-1".into(),
                }),
                true,
                "workspace-a".into(),
            )
            .unwrap();
        registry.fail(&handle, "native resize failed".into());
        registry.update(
            &handle,
            &Update::Output {
                stream: plugin_protocol::process::Stream::Pty,
                bytes: b"tail".to_vec(),
            },
        );
        registry.update(&handle, &Update::Terminated);
        assert!(registry.take("workspace-b").is_empty());
        let view = registry.take("workspace-a").pop().unwrap();
        assert_eq!(view.failure.as_deref(), Some("native resize failed"));
        assert!(matches!(&view.updates[0],Update::Output { bytes,.. } if bytes == b"tail"));
        assert!(matches!(&view.updates[1], Update::Terminated));
        assert!(registry.take("workspace-a").is_empty());
        assert!(
            registry.handles("workspace-a").is_empty(),
            "completion releases the projection quota"
        );
    }
}

/// Byte-exact presentation publication; the resource retains its original provider/scope identity.
#[derive(Clone, Debug)]
pub struct TerminalPresentation {
    pub handle: ResourceHandle,
    pub title: String,
    pub owner: Option<TerminalOwner>,
    /// Only a presented PTY accepts native input; decoded protocol diagnostics are read-only.
    pub interactive: bool,
    pub updates: Vec<Update>,
    pub failure: Option<String>,
}

#[derive(Debug)]
struct Entry {
    /// Original resource creation scope is independent of optional host task placement.
    display_scope: String,
    title: String,
    owner: Option<TerminalOwner>,
    interactive: bool,
    pending: VecDeque<Update>,
    announce: bool,
    ended: bool,
    failure: Option<String>,
    /// A host-confirmed normal stop is bounded without releasing observation or inventing exit.
    exit_deadline: Option<std::time::Instant>,
}
/// At most 64 projections retain bounded chunks; slow views apply per-process read backpressure.
#[derive(Clone, Debug, Default)]
pub struct TerminalPresentations(
    Arc<Mutex<BTreeMap<(String, String, u64), Entry>>>,
    Arc<Mutex<Option<(String, String, u64)>>>,
    Arc<Mutex<String>>,
);
fn key(handle: &ResourceHandle) -> (String, String, u64) {
    (
        handle.instance.clone(),
        handle.scope.clone(),
        handle.resource,
    )
}
impl TerminalPresentations {
    /// The actor publishes its active window scope before any instance can create a native process.
    pub(crate) fn select_scope(&self, scope: String) {
        *self.2.lock().unwrap() = scope;
    }
    /// Direct application-owned execution captures this scope at creation, never at later delivery.
    pub(crate) fn current_scope(&self) -> String {
        self.2.lock().unwrap().clone()
    }
    /// Called only after negotiated capability, PTY type and source ownership checks.
    pub(crate) fn present(
        &self,
        handle: &ResourceHandle,
        title: String,
        owner: Option<TerminalOwner>,
        interactive: bool,
        display_scope: String,
    ) -> Result<(), Failure> {
        let mut entries = self.0.lock().unwrap();
        if let Some(entry) = entries.get(&key(handle)) {
            return if entry.interactive == interactive {
                Ok(())
            } else {
                Err(Failure::new(
                    ErrorCode::InvalidState,
                    "Terminal presentation mode cannot change",
                ))
            };
        }
        if entries.len() >= 64 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Terminal presentation quota exceeded",
            ));
        }
        if title.is_empty() || title.len() > 256 || title.chars().any(char::is_control) {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Invalid terminal title",
            ));
        }
        entries.insert(
            key(handle),
            Entry {
                display_scope,
                title,
                owner,
                interactive,
                pending: VecDeque::new(),
                announce: true,
                ended: false,
                failure: None,
                exit_deadline: None,
            },
        );
        Ok(())
    }
    /// The host cannot turn an arbitrary private process handle into an input capability.
    pub(crate) fn contains(&self, handle: &ResourceHandle) -> bool {
        self.0
            .lock()
            .unwrap()
            .get(&key(handle))
            .is_some_and(|entry| !entry.ended)
    }
    /// Resource type alone cannot grant input to a PTY explicitly published as read-only diagnostics.
    pub(crate) fn interactive(&self, handle: &ResourceHandle) -> bool {
        self.0
            .lock()
            .unwrap()
            .get(&key(handle))
            .is_some_and(|entry| !entry.ended && entry.interactive)
    }
    /// Repeated Stop clicks cannot extend the first accepted interrupt's grace period.
    pub(crate) fn stopping(
        &self,
        handle: &ResourceHandle,
        mode: plugin_protocol::process::ExitMode,
    ) {
        if let Some(entry) = self.0.lock().unwrap().get_mut(&key(handle)) {
            match mode {
                plugin_protocol::process::ExitMode::Graceful => {
                    entry.exit_deadline.get_or_insert_with(|| {
                        std::time::Instant::now()
                            + std::time::Duration::from_millis(crate::DEFAULT_STOP_GRACE_MS.into())
                    });
                }
                plugin_protocol::process::ExitMode::Force => entry.exit_deadline = None,
            }
        }
    }
    /// Only actual active resources whose grace expired are escalated by the manager's normal poll.
    pub(crate) fn overdue(&self) -> Vec<ResourceHandle> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, entry)| {
                !entry.ended
                    && entry
                        .exit_deadline
                        .is_some_and(|deadline| deadline <= std::time::Instant::now())
            })
            .map(|((instance, scope, resource), _)| ResourceHandle {
                instance: instance.clone(),
                scope: scope.clone(),
                resource: *resource,
            })
            .collect()
    }
    /// Eight chunks per native poll fit below this bound; other process streams continue draining.
    pub(crate) fn ready(&self, handle: &ResourceHandle) -> bool {
        self.0
            .lock()
            .unwrap()
            .get(&key(handle))
            .is_none_or(|entry| !entry.interactive || entry.pending.len() < 128)
    }
    /// Decoded bytes never borrow raw protocol stdout, and their producer observes bounded pressure.
    pub(crate) fn output(&self, handle: &ResourceHandle, bytes: Vec<u8>) -> Result<(), Failure> {
        let mut entries = self.0.lock().unwrap();
        let entry = entries.get_mut(&key(handle)).ok_or_else(|| {
            Failure::new(
                ErrorCode::InvalidHandle,
                "Terminal output resource is unavailable",
            )
        })?;
        if entry.ended || entry.interactive {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Expected a live read-only terminal",
            ));
        }
        if entry.pending.len() >= 128 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Terminal decoded-output quota exceeded",
            ));
        }
        entry.pending.push_back(Update::Output {
            stream: plugin_protocol::process::Stream::Stdout,
            bytes,
        });
        Ok(())
    }
    /// A native I/O error remains visible without falsely acknowledging that the process has exited.
    pub(crate) fn fail(&self, handle: &ResourceHandle, message: String) {
        if let Some(entry) = self.0.lock().unwrap().get_mut(&key(handle)) {
            entry.failure = Some(message.chars().take(4096).collect());
        }
    }
    /// Closing observers retain tail bytes after guest handles are revoked; hard overflow is visible.
    pub(crate) fn update(&self, handle: &ResourceHandle, update: &Update) {
        let mut entries = self.0.lock().unwrap();
        let Some(entry) = entries.get_mut(&key(handle)) else {
            return;
        };
        if !entry.interactive && matches!(update, Update::Output { .. }) {
            return;
        }
        if matches!(update, Update::Exited { .. } | Update::Terminated) {
            entry.ended = true;
        }
        if entry.pending.len() < 256 || !matches!(update, Update::Output { .. }) {
            entry.pending.push_back(update.clone());
        } else {
            entry.failure = Some("Terminal final-output quota exceeded".into());
        }
    }
    /// A bounded frame drains raw chunks once; ended projections disappear only after their tail.
    pub(crate) fn take(&self, visible_scope: &str) -> Vec<TerminalPresentation> {
        let mut entries = self.0.lock().unwrap();
        let mut result = Vec::new();
        let mut cursor = self.1.lock().unwrap();
        // Rotate admission across frames, so continuous output in early entries cannot starve stdin targets.
        let keys: Vec<_> = entries
            .keys()
            .filter(|key| entries[*key].display_scope == visible_scope)
            .filter(|key| cursor.as_ref().is_none_or(|last| *key > last))
            .chain(
                entries
                    .keys()
                    .filter(|key| entries[*key].display_scope == visible_scope)
                    .filter(|key| cursor.as_ref().is_some_and(|last| *key <= last)),
            )
            .cloned()
            .collect();
        for (instance, scope, resource) in keys {
            if result.len() >= 16 {
                break;
            }
            let entry = entries
                .get_mut(&(instance.clone(), scope.clone(), resource))
                .unwrap();
            if !entry.announce && entry.pending.is_empty() && entry.failure.is_none() {
                continue;
            }
            entry.announce = false;
            result.push(TerminalPresentation {
                handle: ResourceHandle {
                    instance: instance.clone(),
                    scope: scope.clone(),
                    resource,
                },
                title: entry.title.clone(),
                owner: entry.owner.clone(),
                interactive: entry.interactive,
                updates: entry.pending.drain(..entry.pending.len().min(32)).collect(),
                failure: entry.failure.take(),
            });
            *cursor = Some((instance, scope, resource));
        }
        entries.retain(|_, entry| !entry.ended || !entry.pending.is_empty());
        result
    }
    /// Workspace retirement revokes private views; an application owner retains its global scope.
    pub(crate) fn handles(&self, scope: &str) -> Vec<ResourceHandle> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, entry)| entry.display_scope == scope)
            .map(|((instance, scope, resource), _)| ResourceHandle {
                instance: instance.clone(),
                scope: scope.clone(),
                resource: *resource,
            })
            .collect()
    }
    /// Retired output is never queued for another workspace, including delayed native close callbacks.
    pub(crate) fn discard_scope(&self, scope: &str) {
        self.0
            .lock()
            .unwrap()
            .retain(|_, entry| entry.display_scope != scope);
    }
}
