//! Canonical debugger shapes shared by independent SDK providers and the host consumer.

/// Exact debug.session 1.1 signatures; optional methods are matched individually.
/// These messages do not choose a transport, language, or installed provider identity.
pub fn declaration() -> crate::service::Contract {
    serde_json::from_str(include_str!("debug-session.json"))
        .expect("the published debug.session contract is valid")
}
