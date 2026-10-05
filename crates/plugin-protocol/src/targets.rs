//! Portable target bindings and provider-owned preparation over versioned plugin services.
use serde::{Deserialize, Serialize};
/// Discovery never runs a target; bindings are portable provider data, not executable host commands.
pub const CONTRACT: &str = "run.targets";
/// Exact 1.0 signatures shared by an independent provider and the host consumer.
pub fn declaration() -> crate::service::Contract {
    serde_json::from_str(include_str!("run-targets.json")).expect("published run.targets contract")
}
/// One provider candidate. The host namespaces identity with the authenticated provider incarnation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub identity: String,
    pub label: String,
    pub source: String,
    pub target_type: String,
    pub type_version: u32,
    /// Versioned JSON containing only portable fields; no credentials, grants or machine paths.
    pub binding: String,
}
