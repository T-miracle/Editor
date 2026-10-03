//! Instance-bound document subscriptions share the same revocable ownership as other resources.
use super::*;
use crate::document_events::Subscription;
use api::{ErrorCode, Failure, Notification, Value};
use resource_roots::RootKind;

impl State {
    /// Subscription authority cannot be borrowed from a UI permission or an application owner.
    pub(super) fn subscribe_documents(&mut self) -> Result<Value, Failure> {
        if !self.active
            || self.roots.retired
            || self.roots.application
            || !self.permissions.contains("editor.read")
        {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Workspace document read authority required",
            ));
        }
        if !self.api.capabilities.contains_key("editor.documents") {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "editor.documents was not negotiated",
            ));
        }
        if self.subscriptions.len() >= 8 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Subscription quota exceeded",
            ));
        }
        let Value::Resource(handle) = self.roots.open(RootKind::Subscription)? else {
            unreachable!()
        };
        self.subscriptions.insert(
            handle.resource,
            Subscription {
                handle: handle.clone(),
                events: Default::default(),
            },
        );
        Ok(Value::Resource(handle))
    }
}

impl Instance {
    /// Host document revisions enter only the owning instance's bounded subscriptions.
    pub(crate) fn document_changed(&mut self, change: api::DocumentChange) {
        self.invalidate_image_inputs(&change);
        for subscription in self.store.data_mut().subscriptions.values_mut() {
            subscription.events.push(change.clone());
        }
    }
    /// Ingress overflow terminates each affected subscription explicitly.
    pub(crate) fn document_events_failed(&mut self, error: Failure) {
        for subscription in self.store.data_mut().subscriptions.values_mut() {
            subscription.events.fail(error.clone());
        }
    }
    /// A guest can unsubscribe from inside a callback; remaining queued deliveries then stop immediately.
    pub(super) fn poll_document_events(&mut self) -> anyhow::Result<()> {
        let deliveries = self
            .store
            .data_mut()
            .subscriptions
            .values_mut()
            .map(|subscription| {
                (
                    subscription.handle.clone(),
                    subscription.events.take_batch(8),
                )
            })
            .collect::<Vec<_>>();
        for (handle, changes) in deliveries {
            // An earlier callback can release this subscription, including its queued terminal failure.
            if !self
                .store
                .data()
                .subscriptions
                .contains_key(&handle.resource)
            {
                continue;
            }
            match changes {
                Ok(changes) => {
                    for change in changes {
                        if !self
                            .store
                            .data()
                            .subscriptions
                            .contains_key(&handle.resource)
                        {
                            break;
                        }
                        self.call(api::Input::Event {
                            panel: None,
                            event: Notification::Document {
                                subscription: handle.clone(),
                                change,
                            },
                        })?;
                    }
                }
                Err(error) => {
                    self.store.data_mut().subscriptions.remove(&handle.resource);
                    self.store.data_mut().roots.remove(&handle);
                    self.call(api::Input::Event {
                        panel: None,
                        event: Notification::SubscriptionFailed {
                            subscription: handle,
                            error,
                        },
                    })?;
                }
            }
        }
        Ok(())
    }
}
