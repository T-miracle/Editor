//! Workspace/file-type preference intent is opaque and independent from native document state.
use super::*;

/// The host supplies the plugin and workspace namespace; callers select only their local purpose.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceKey {
    /// Lowercase extension without a dot; empty denotes a workspace window preference.
    pub file_type: String,
    /// A bounded plugin-local key, such as a group of display choices.
    pub name: String,
}
impl PreferenceKey {
    /// Keys cannot name paths, other plugins, workspaces, or an unbounded storage namespace.
    pub fn validate(&self) -> Result<(), Failure> {
        if self.file_type.len() > 32
            || self.name.is_empty()
            || self.name.len() > 64
            || !self.file_type.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'+')
            })
            || !self.name.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
            })
        {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Invalid private preference key",
            ));
        }
        Ok(())
    }
}

/// Missing data is revision zero. Existing corrupt records cause an error and are preserved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceValue {
    pub revision: u64,
    pub data: Option<serde_json::Value>,
}
impl PreferenceValue {
    /// Quotas and record invariants apply to persisted and notified values equally.
    pub fn validate(&self) -> Result<(), Failure> {
        if (self.revision == 0) != self.data.is_none() {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Invalid preference revision",
            ));
        }
        if serde_json::to_vec(&self.data).map_or(true, |bytes| bytes.len() > 64 * 1024) {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Preference exceeds 64 KiB",
            ));
        }
        Ok(())
    }
}

/// Watches are optional, bounded and explicitly revocable through the normal resource interface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceRead {
    pub value: PreferenceValue,
    pub subscription: Option<ResourceHandle>,
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Preference keys never expand into host paths or implicitly normalize a different file type.
    #[test]
    fn bounded_private_intent_rejects_foreign_names_and_corruption() {
        for file_type in ["md", "png", ""] {
            PreferenceKey {
                file_type: file_type.into(),
                name: "display".into(),
            }
            .validate()
            .unwrap();
        }
        for file_type in ["MD", "../another", "C:\\root", "other/plugin"] {
            assert!(
                PreferenceKey {
                    file_type: file_type.into(),
                    name: "display".into()
                }
                .validate()
                .is_err()
            );
        }
        PreferenceValue::default().validate().unwrap();
        assert!(
            PreferenceValue {
                revision: 0,
                data: Some(serde_json::json!(true))
            }
            .validate()
            .is_err()
        );
        assert!(
            PreferenceValue {
                revision: 1,
                data: Some(serde_json::json!("a".repeat(65536)))
            }
            .validate()
            .is_err()
        );
    }
}
