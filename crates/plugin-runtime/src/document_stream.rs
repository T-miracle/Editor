//! Opt-in ordered metadata streams are bounded independently of legacy revision coalescing.
use plugin_protocol::api::{DocumentEvent, ErrorCode, Failure, ResourceHandle};
use std::collections::VecDeque;

/// FIFO metadata ingress never silently loses save, close, selection, or active transitions.
#[derive(Default)]
pub struct DocumentStream {
    pending: VecDeque<DocumentEvent>,
    last_sequence: u64,
    failure: Option<Failure>,
}

impl DocumentStream {
    /// Duplicate/late batches cannot reorder a stream; capacity exhaustion is a terminal error.
    pub fn push(&mut self, event: DocumentEvent) {
        if self.failure.is_some() || event.sequence <= self.last_sequence {
            return;
        }
        self.last_sequence = event.sequence;
        if self.pending.len() >= 128 {
            self.fail(Failure::new(
                ErrorCode::LimitExceeded,
                "Document stream overflow; enumerate and resubscribe",
            ));
            return;
        }
        self.pending.push_back(event);
    }

    /// A source failure replaces an incomplete stream with one explicit terminal notification.
    pub fn fail(&mut self, failure: Failure) {
        self.pending.clear();
        self.failure = Some(failure);
    }

    /// A bounded drain preserves order across worker ticks.
    pub fn take_batch(&mut self, limit: usize) -> Result<Vec<DocumentEvent>, Failure> {
        if let Some(failure) = self.failure.take() {
            return Err(failure);
        }
        Ok((0..limit)
            .filter_map(|_| self.pending.pop_front())
            .collect())
    }
}

/// Subscription ownership reuses the public resource revocation path.
pub(crate) struct Subscription {
    pub handle: ResourceHandle,
    pub events: DocumentStream,
}
