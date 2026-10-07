//! Receipt provenance is checked on the actor immediately before any launch side effect.
use super::*;
use rust_i18n::t;
impl Work {
    /// Attach provenance; normal runtime permission and ownership checks still execute.
    pub(crate) fn validated(self, origin: Option<plugin_runtime::TargetOrigin>) -> Self {
        match origin {
            Some(origin) => Self::Validated {
                origin,
                operation: Box::new(self),
            },
            None => self,
        }
    }
    /// Return admitted work or publish the terminal failure its caller already understands.
    /// Rejection never affects Stop or existing session ownership.
    pub(in crate::extensions) fn admit(
        self,
        manager: &Manager,
        output: &Arc<Mutex<Published>>,
    ) -> Option<Self> {
        let Self::Validated { origin, operation } = self else {
            return Some(self);
        };
        if origin.valid_for(manager) {
            return operation.admit(manager, output);
        }
        let reason = t!("run.plugin_stale").to_string();
        let mut published = output.lock().unwrap();
        match *operation {
            Self::StartRun {
                config, request_id, ..
            } => published.run_errors.push((config, request_id, reason)),
            Self::PrepareTarget {
                config,
                index,
                request,
                ..
            } => published
                .target_preparations
                .push((config, index, request, Err(reason))),
            Self::DebugCall { request, .. } => published
                .debug_answers
                .push((request, DebugAnswerMessage::Failed(reason))),
            Self::ConfigurationCall { request, .. } => {
                published
                    .configuration_replies
                    .push(configurations::ConfigurationReply {
                        request,
                        origin: Some(origin),
                        result: Err(reason),
                    })
            }
            // Only launch operations carry provenance. An unexpected operation fails closed.
            _ => {}
        }
        published.configuration_revision += 1;
        None
    }
}
