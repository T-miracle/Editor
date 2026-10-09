//! Instance-bound document subscriptions share the same revocable ownership as other resources.
use super::*;
use crate::document_events::Subscription;
use api::{ErrorCode, Failure, Notification, Value};
use resource_roots::RootKind;

impl State {
    /// Subscription authority cannot be borrowed from a UI permission or an application owner.
    pub(super) fn subscribe_documents(&mut self) -> Result<Value, Failure> {
        let handle = self.document_subscription_handle(0)?;
        self.subscriptions.insert(
            handle.resource,
            Subscription {
                handle: handle.clone(),
                events: Default::default(),
            },
        );
        Ok(Value::Resource(handle))
    }

    /// Opt-in delivery preserves the existing notification format for older SDK consumers.
    pub(super) fn subscribe_document_stream(&mut self) -> Result<Value, Failure> {
        let handle = self.document_subscription_handle(1)?;
        self.document_streams.insert(
            handle.resource,
            crate::document_stream::Subscription {
                handle: handle.clone(),
                events: Default::default(),
            },
        );
        Ok(Value::Resource(handle))
    }

    /// All document streams share the same instance quota and read/trust boundary.
    fn document_subscription_handle(&mut self, minor: u64) -> Result<api::ResourceHandle, Failure> {
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
        if !self
            .api
            .capabilities
            .get("editor.documents")
            .is_some_and(|version| version >= &semver::Version::new(1, minor, 0))
        {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "editor.documents was not negotiated",
            ));
        }
        if self.subscriptions.len() + self.document_streams.len() >= 8 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Subscription quota exceeded",
            ));
        }
        let Value::Resource(handle) = self.roots.open(RootKind::Subscription)? else {
            unreachable!()
        };
        Ok(handle)
    }
}

impl Instance {
    /// The rich stream is separate from coalesced changes; resource identities confer no authority.
    pub(crate) fn document_event(&mut self, mut event: api::DocumentEvent) {
        if let Some(api::ResourceIdentity::Virtual { handle }) = &event.resource {
            let owner = &self.store.data().plugin_services.principal;
            if handle.instance != owner.instance || handle.scope != owner.scope {
                // Other instances learn that their formerly active local document lost focus,
                // without receiving the private provider's resource identity.
                if matches!(event.kind, api::DocumentEventKind::ActiveChanged(_)) {
                    event.resource = None;
                    event.kind = api::DocumentEventKind::ActiveChanged(None);
                } else {
                    return;
                }
            }
        }
        for subscription in self.store.data_mut().document_streams.values_mut() {
            subscription.events.push(event.clone());
        }
    }

    /// Ingress overflow forces a new snapshot instead of claiming a complete event history.
    pub(crate) fn document_stream_failed(&mut self, error: Failure) {
        for subscription in self.store.data_mut().document_streams.values_mut() {
            subscription.events.fail(error.clone());
        }
    }

    /// A callback may revoke its own handle; remaining delivery checks ownership again.
    pub(super) fn poll_document_streams(&mut self) -> anyhow::Result<()> {
        let deliveries = self
            .store
            .data_mut()
            .document_streams
            .values_mut()
            .map(|subscription| {
                (
                    subscription.handle.clone(),
                    subscription.events.take_batch(8),
                )
            })
            .collect::<Vec<_>>();
        for (handle, events) in deliveries {
            if !self
                .store
                .data()
                .document_streams
                .contains_key(&handle.resource)
            {
                continue;
            }
            match events {
                Ok(events) => {
                    for event in events {
                        if !self
                            .store
                            .data()
                            .document_streams
                            .contains_key(&handle.resource)
                        {
                            break;
                        }
                        self.call(api::Input::Event {
                            panel: None,
                            event: Notification::DocumentEvent {
                                subscription: handle.clone(),
                                event,
                            },
                        })?;
                    }
                }
                Err(error) => {
                    self.store
                        .data_mut()
                        .document_streams
                        .remove(&handle.resource);
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
