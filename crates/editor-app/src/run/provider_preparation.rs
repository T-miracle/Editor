//! Provider preparations retain independent native output, stop barriers, and original ownership.
use super::*;

/// Host presentation of one preparation receipt; immutable request identity separates concurrent jobs.
#[derive(Clone, Debug)]
pub(super) struct ProviderPreparationView {
    pub config: String,
    pub name: String,
    pub snapshot: plugin_runtime::PreparationSnapshot,
}
impl RunControls {
    /// Stop/force addresses an active owned receipt after its sequence has already been sealed.
    pub fn provider_preparation_request(&self, config: &str) -> Option<u64> {
        self.provider_preparations
            .iter()
            .rev()
            .find(|(_, view)| view.config == config && view.snapshot.state.is_active())
            .map(|(request, _)| *request)
    }
    /// Leaving cancels every owned native preparation, including already stopping jobs.
    pub fn active_provider_preparations(&self) -> Vec<u64> {
        self.provider_preparations
            .iter()
            .filter(|(_, view)| view.snapshot.state.is_active())
            .map(|(request, _)| *request)
            .collect()
    }
    /// Register visible ownership before staging a request, including a failed enqueue.
    pub fn register_provider_preparation(&mut self, config: &str, index: usize, request: u64) {
        let Some(sequence) = self.sequences.get(config) else {
            return;
        };
        let Some((provider, _)) = sequence.planned_preparation(index) else {
            return;
        };
        let name = sequence
            .steps()
            .get(index)
            .map(|step| step.name.clone())
            .unwrap_or_default();
        self.provider_preparations.insert(
            request,
            ProviderPreparationView {
                config: config.into(),
                name,
                snapshot: plugin_runtime::PreparationSnapshot {
                    output: String::new(),
                    state: plugin_runtime::ExecutionState::Starting,
                    provider: provider.clone(),
                },
            },
        );
        self.preparation_output = Some(request);
        self.preparation_output_open = true;
        // Retain the newest 64 ended preparations as well as all bounded active preparations.
        let ended = self
            .provider_preparations
            .iter()
            .rev()
            .filter(|(_, view)| !view.snapshot.state.is_active())
            .skip(64)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in ended {
            self.provider_preparations.remove(&id);
        }
    }
    /// A publication cannot create a new owner or replace another request's provider.
    pub fn observe_provider_preparation(
        &mut self,
        request: u64,
        config: &str,
        snapshot: plugin_runtime::PreparationSnapshot,
    ) {
        let Some(view) = self
            .provider_preparations
            .get_mut(&request)
            .filter(|view| view.config == config && view.snapshot.provider == snapshot.provider)
        else {
            return;
        };
        view.snapshot = snapshot;
    }
    /// Rerun waits for the original preparation's actual native cleanup, never just its stop admission.
    pub fn wait_to_prepare_again(&mut self, config: &str, request: u64, debug: bool) {
        self.preparation_reruns
            .insert(config.into(), (request, debug));
    }
    pub fn take_ready_preparation_reruns(&mut self) -> Vec<(String, bool)> {
        let ready = self
            .preparation_reruns
            .iter()
            .filter(|(_, (request, _))| {
                !self
                    .pending
                    .iter()
                    .any(|pending| pending.request_id == *request)
            })
            .map(|(config, (_, debug))| (config.clone(), *debug))
            .collect::<Vec<_>>();
        for (config, _) in &ready {
            self.preparation_reruns.remove(config);
        }
        ready
    }
    /// Explicit output restoration selects its saved configuration and retained bounded history.
    pub fn show_preparation_output(&mut self, request: u64) {
        if let Some(view) = self.provider_preparations.get(&request) {
            self.configs.select(&view.config);
            self.preparation_output = Some(request);
            self.preparation_output_open = true;
        }
    }
    /// The viewed history follows explicit session selection and is independent from active processes.
    pub(super) fn preparation_output_view(&self) -> Option<&ProviderPreparationView> {
        if !self.preparation_output_open {
            return None;
        }
        self.provider_preparations.get(&self.preparation_output?)
    }
}
