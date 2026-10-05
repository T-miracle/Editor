//! Opaque private intent uses existing scoped files, atomic quotas and revocable instance watches.
use super::*;
use api::{
    ErrorCode, Failure, PreferenceKey, PreferenceRead, PreferenceValue, ResourceHandle, Value,
};
use resource_roots::RootKind;
use sha2::{Digest, Sha256};
use std::sync::Mutex;

/// Serialize compare-and-set across native embedders sharing one scoped private directory.
/// The bounded disk work happens on runtime workers, never the GPUI input/render thread.
static WRITES: Mutex<()> = Mutex::new(());

pub(super) struct Subscription {
    handle: ResourceHandle,
    key: PreferenceKey,
    revision: u64,
}

/// Hashing prevents file-type/name data from addressing arbitrary files or another owner.
fn record_path(key: &PreferenceKey) -> String {
    format!(
        "preference-{:x}.json",
        Sha256::digest(serde_json::to_vec(key).unwrap())
    )
}

/// Missing intent is explicit; malformed existing records are reported without overwriting them.
fn read(
    root: &Path,
    staged: &Option<std::collections::BTreeMap<PathBuf, Vec<u8>>>,
    key: &PreferenceKey,
) -> Result<PreferenceValue, Failure> {
    let value = match data_files::read(root, &record_path(key), staged) {
        Ok(bytes) => serde_json::from_slice::<PreferenceValue>(&bytes).map_err(|_| {
            Failure::new(
                ErrorCode::InvalidState,
                "Corrupt preference record; existing file preserved",
            )
        })?,
        Err(error) if error.code == ErrorCode::NotFound => PreferenceValue::default(),
        Err(error) => return Err(error),
    };
    value.validate()?;
    Ok(value)
}

/// Compare-and-set serializes read/atomic-write; a stale caller never replaces a newer intent.
fn compare_and_set(
    root: &Path,
    limit: usize,
    staged: &mut Option<std::collections::BTreeMap<PathBuf, Vec<u8>>>,
    key: &PreferenceKey,
    expected: u64,
    data: serde_json::Value,
) -> Result<PreferenceValue, Failure> {
    let _guard = WRITES
        .lock()
        .map_err(|_| Failure::new(ErrorCode::OperationFailed, "Preference writer failed"))?;
    let current = read(root, staged, key)?;
    if current.revision != expected {
        return Err(Failure::new(
            ErrorCode::Conflict,
            "Preference changed in another instance",
        ));
    }
    if current.data.as_ref() == Some(&data) {
        return Ok(current);
    }
    let value = PreferenceValue {
        revision: current
            .revision
            .checked_add(1)
            .ok_or_else(|| Failure::new(ErrorCode::Conflict, "Preference revisions exhausted"))?,
        data: Some(data),
    };
    value.validate()?;
    let bytes = serde_json::to_vec(&value)
        .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
    data_files::write(root, &record_path(key), bytes, limit, staged)?;
    Ok(value)
}

impl State {
    /// Preference operations add no path authority and use the same transaction's private file root.
    pub(super) fn preference_request(
        &mut self,
        operation: api::Operation,
    ) -> Result<Value, Failure> {
        if !self
            .api
            .capabilities
            .get("storage.private")
            .is_some_and(|version| *version >= semver::Version::new(1, 1, 0))
        {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "Preferences require storage.private 1.1",
            ));
        }
        if self.roots.application {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Preferences require an owning workspace",
            ));
        }
        let (key, watch, write) = match operation {
            api::Operation::ReadPreference { key, watch } => (key, watch, None),
            api::Operation::WritePreference {
                key,
                expected_revision,
                data,
            } => (key, false, Some((expected_revision, data))),
            _ => unreachable!("preference dispatch only"),
        };
        key.validate()?;
        if watch
            && self.preference_subscriptions.len() >= 32
            && !self
                .preference_subscriptions
                .values()
                .any(|sub| sub.key == key)
        {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Too many preference watches",
            ));
        }
        let root = self
            .file_authority(RootKind::Data, write.is_some())?
            .to_path_buf();
        let value = if let Some((expected, data)) = write {
            compare_and_set(
                &root,
                self.roots.limit,
                &mut self.staged_writes,
                &key,
                expected,
                data,
            )?
        } else {
            read(&root, &self.staged_writes, &key)?
        };
        let subscription = if watch {
            if let Some(sub) = self
                .preference_subscriptions
                .values()
                .find(|sub| sub.key == key)
            {
                Some(sub.handle.clone())
            } else {
                let Value::Resource(handle) = self.roots.open(RootKind::PreferenceSubscription)?
                else {
                    unreachable!()
                };
                self.preference_subscriptions.insert(
                    handle.resource,
                    Subscription {
                        handle: handle.clone(),
                        key,
                        revision: value.revision,
                    },
                );
                Some(handle)
            }
        } else {
            None
        };
        Ok(Value::Preference(PreferenceRead {
            value,
            subscription,
        }))
    }
}

#[cfg(test)]
#[path = "preferences_tests.rs"]
mod tests;

impl Instance {
    /// Each live watch compares its scoped record; other instances converge without sharing document state.
    pub(super) fn poll_preferences(&mut self) -> anyhow::Result<bool> {
        let watches = self
            .store
            .data()
            .preference_subscriptions
            .values()
            .map(|sub| (sub.handle.clone(), sub.key.clone(), sub.revision))
            .collect::<Vec<_>>();
        let mut changed = false;
        for (handle, key, revision) in watches {
            if !self
                .store
                .data()
                .preference_subscriptions
                .contains_key(&handle.resource)
            {
                continue;
            }
            let read = self
                .store
                .data_mut()
                .preference_request(api::Operation::ReadPreference {
                    key: key.clone(),
                    watch: false,
                });
            let event = match read {
                Ok(Value::Preference(read)) if read.value.revision != revision => {
                    self.store
                        .data_mut()
                        .preference_subscriptions
                        .get_mut(&handle.resource)
                        .unwrap()
                        .revision = read.value.revision;
                    api::Notification::PreferenceChanged {
                        subscription: handle,
                        key,
                        value: read.value,
                    }
                }
                Ok(_) => continue,
                Err(error) => {
                    self.store
                        .data_mut()
                        .preference_subscriptions
                        .remove(&handle.resource);
                    self.store.data_mut().roots.remove(&handle);
                    api::Notification::SubscriptionFailed {
                        subscription: handle,
                        error,
                    }
                }
            };
            changed = true;
            self.notify(None, event)?;
        }
        Ok(changed)
    }
}
