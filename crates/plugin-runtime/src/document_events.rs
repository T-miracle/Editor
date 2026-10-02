//! Latest-version notifications are coalesced; capacity exhaustion closes a stream explicitly.
use plugin_protocol::api::{DocumentChange, ErrorCode, Failure, ResourceHandle};
use std::collections::{BTreeMap, VecDeque};

/// Shared ingress and per-subscription queues have identical bounded semantics.
#[derive(Default)]
pub struct DocumentEvents {
    pending: BTreeMap<String, DocumentChange>,
    order: VecDeque<String>,
    seen: BTreeMap<String, u64>,
    failure: Option<Failure>,
}
impl DocumentEvents {
    /// Older arrivals cannot replace the newest revision for the same open-document identity.
    pub fn push(&mut self, change: DocumentChange) {
        if self.failure.is_some() {
            return;
        }
        if change.document.id.len() > 128 || change.document.path.len() > 4096 {
            self.fail(Failure::new(
                ErrorCode::LimitExceeded,
                "Document identity exceeds event budget",
            ));
            return;
        }
        // Retain watermarks after delivery, including closed documents, so late batches cannot resurrect them.
        let id = &change.document.id;
        if self
            .seen
            .get(id)
            .is_some_and(|revision| *revision >= change.document.revision)
        {
            return;
        }
        if (!self.pending.contains_key(id) && self.pending.len() >= 64)
            || (!self.seen.contains_key(id) && self.seen.len() >= 1024)
        {
            self.fail(Failure::new(
                ErrorCode::LimitExceeded,
                "Document event queue overflow; resubscribe and refresh state",
            ));
            return;
        }
        if !self.pending.contains_key(id) {
            self.order.push_back(id.clone());
        }
        self.seen.insert(id.clone(), change.document.revision);
        self.pending.insert(change.document.id.clone(), change);
    }
    /// An explicit terminal error replaces the incomplete stream, never a silent partial success.
    pub fn fail(&mut self, error: Failure) {
        self.pending.clear();
        self.order.clear();
        self.seen.clear();
        self.failure = Some(error);
    }
    /// Small batches bound work per worker tick and retain all remaining latest versions.
    pub fn take_batch(&mut self, limit: usize) -> Result<Vec<DocumentChange>, Failure> {
        if let Some(error) = self.failure.take() {
            return Err(error);
        }
        // FIFO document identities prevent a frequently edited early-sort path from starving others.
        let keys = (0..limit)
            .filter_map(|_| self.order.pop_front())
            .collect::<Vec<_>>();
        Ok(keys
            .into_iter()
            .filter_map(|key| self.pending.remove(&key))
            .collect())
    }
}

pub(crate) struct Subscription {
    pub handle: ResourceHandle,
    pub events: DocumentEvents,
}
