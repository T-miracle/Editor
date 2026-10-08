//! A service-owned pure worker consumes snapshots; retirement revokes its memory and pending results.
use super::*;
use plugin_protocol::language::{
    CompletionDiagnostic, CompletionProposal, CompletionRequest, SourceSnapshot,
};
use std::sync::atomic::{AtomicU64, Ordering};

/// No mutable editor state, restored private snapshot or package side effect lives in this worker.
pub(crate) struct CompletionHook {
    pub instance: Mutex<Option<crate::Instance>>,
    pub settings: plugin_protocol::settings::Effective,
    pub sequence: AtomicU64,
}

impl LanguageService {
    /// Return a bounded supplement for one immutable document; absence of the opt-in is an empty result.
    /// The caller must still verify the editor's current DocumentVersion before presenting suggestions.
    pub fn complete_snapshot(
        &self,
        source: SourceSnapshot,
        cursor: usize,
        uri: String,
        diagnostics: Option<Vec<CompletionDiagnostic>>,
    ) -> anyhow::Result<Option<CompletionProposal>> {
        anyhow::ensure!(self.is_active(), "Completion provider has been retired");
        let Some(hook) = &self.completion else {
            return Ok(None);
        };
        anyhow::ensure!(
            source.valid() && cursor <= source.text.len() && source.text.is_char_boundary(cursor),
            "Invalid completion snapshot"
        );
        let request = CompletionRequest {
            request: hook
                .sequence
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                    value.checked_add(1)
                })
                .map_err(|_| anyhow::anyhow!("Completion request identity exhausted"))?,
            provider: self.provider.id.clone(),
            source,
            uri,
            diagnostics,
            cursor,
            settings: hook.settings.clone(),
        };
        anyhow::ensure!(request.valid(), "Invalid completion context");
        anyhow::ensure!(
            request.request != 0,
            "Completion request identity exhausted"
        );
        // WASM calls are serialized and fuel/deadline bounded, while revocation remains observable outside the lock.
        let output = hook
            .instance
            .lock()
            .unwrap()
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Completion executor has been retired"))?
            .complete_snapshot(request.clone())?;
        anyhow::ensure!(self.is_active(), "Completion provider has been retired");
        let proposal = output
            .language_completion
            .ok_or_else(|| anyhow::anyhow!("Completion hook returned no proposal"))?;
        anyhow::ensure!(
            proposal.valid_for(&request),
            "Invalid or stale completion proposal"
        );
        Ok(Some(proposal))
    }
}
