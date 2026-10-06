//! Plugin functions contribute versioned toolbar data; host focus never changes the captured target.
use super::*;
use crate::api::{ErrorCode, Failure, FileVersion};

/// Both supported UI languages are supplied by the package, including accessible button names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalizedText {
    pub zh_cn: String,
    pub en: String,
}
impl LocalizedText {
    /// Resolve host locale; an empty locale follows the established Simplified Chinese default.
    pub fn for_locale(&self, locale: &str) -> &str {
        if locale.is_empty() || locale.starts_with("zh") {
            &self.zh_cn
        } else {
            &self.en
        }
    }
}

/// Bounded geometry-only SVGs belong to the publishing package; neither path is a host icon name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolIcon {
    pub light: String,
    pub dark: String,
}
impl ToolIcon {
    /// Select package artwork for the actual host theme without plugin-specific branches.
    pub fn path(&self, dark: bool) -> &str {
        if dark { &self.dark } else { &self.light }
    }
    /// Validate path syntax here; the runtime rechecks canonical package ownership and SVG contents.
    pub(super) fn validate(&self) -> Result<(), String> {
        for path in [&self.light, &self.dark] {
            if path.is_empty()
                || path.len() > 1024
                || !path.to_ascii_lowercase().ends_with(".svg")
                || path.contains(['\\', ':', '?'])
                || path.chars().any(char::is_control)
                || path.split('/').any(|part| {
                    part.is_empty()
                        || part == "."
                        || part == ".."
                        || crate::api::is_windows_device_segment(part)
                })
            {
                return Err("Tool icons require owned relative SVG paths".into());
            }
        }
        Ok(())
    }
}

/// File ownership is exact; a window target names only the contribution's own independent panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolTarget {
    File { version: FileVersion },
    Window { panel: String },
}

/// One native button's meaning and state are supplied by the guest, never inferred from its icon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolButton {
    pub id: String,
    pub label: LocalizedText,
    pub tooltip: LocalizedText,
    pub icon: ToolIcon,
    pub target: ToolTarget,
    pub visible: bool,
    pub selected: bool,
    pub disabled: bool,
    pub order: i32,
}

/// Deferred and overflow activations carry the same tree revision and captured function target.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolEvent {
    pub revision: u64,
    pub tool: String,
    pub target: ToolTarget,
}

impl Document {
    /// Accept only an enabled current contribution outside a modal; errors do not crash the guest.
    pub fn validate_tool_event(&self, event: &ToolEvent) -> Result<(), Failure> {
        if event.revision != self.revision {
            return Err(Failure::new(
                ErrorCode::StaleRevision,
                "Tool scene was replaced",
            ));
        }
        if self.dialog.is_some() || self.menu.is_some() {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Modal owns function input",
            ));
        }
        let tool = self
            .tools
            .iter()
            .find(|tool| tool.id == event.tool && tool.visible && !tool.disabled)
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::InvalidHandle,
                    "Tool is absent, hidden or disabled",
                )
            })?;
        if tool.target != event.target {
            return Err(Failure::new(
                ErrorCode::StaleRevision,
                "Function target changed",
            ));
        }
        Ok(())
    }
}
