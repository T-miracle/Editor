//! Localized presentation of typed core debug reasons; failures retain their provider's text.
use rust_i18n::t;

/// Translate only host-owned control reasons, preserving source diagnostics as substitution values.
pub(super) fn control_reason(reason: editor_core::DebugControlReason) -> String {
    use editor_core::DebugControlReason as Reason;
    match reason {
        Reason::AlreadyConnected => t!("run.debug_already_connected"),
        Reason::Unavailable(reason) => return reason,
        Reason::Connecting => t!("run.debug_connecting"),
        Reason::NoSession => t!("run.debug_disconnected"),
        Reason::Exited => t!("run.debug_exited"),
        Reason::Failed(reason) => t!("run.debug_failed", reason = reason),
        Reason::Running => t!("run.debug_running"),
        Reason::Paused => t!("run.debug_already_paused"),
        Reason::PauseBeforeStep => t!("run.debug_pause_before_step"),
        Reason::ResumePauseUndeclared => t!("run.debug_resume_pause_undeclared"),
        Reason::StepUndeclared => t!("run.step_unsupported"),
    }
    .to_string()
}

/// Inspection errors are structured before display, so translated wording never controls validation.
pub(super) fn inspection_error(error: &editor_core::InspectionError) -> String {
    use editor_core::InspectionError as Error;
    match error {
        Error::NoSession => t!("run.debug_disconnected"),
        Error::NotPaused => t!("run.debug_inspection_not_paused"),
        Error::StalePause => t!("run.debug_stale_pause"),
        Error::NoSuchFrame { frame } => t!("run.debug_unknown_frame", frame = frame.to_string()),
        Error::WrongSession => t!("run.debug_wrong_session"),
        Error::Provider(reason) => t!("run.debug_provider_failure", reason = reason),
    }
    .to_string()
}
