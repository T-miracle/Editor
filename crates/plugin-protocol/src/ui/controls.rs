//! Portable collection and popup models used by native documents.
use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SideTabs {
    pub id: String,
    /// The default edge is right; both edges share the same resizing semantics.
    #[serde(default)]
    pub position: SideTabsPosition,
    pub items: Vec<SideTab>,
    pub selected: Option<String>,
    pub rename: Option<String>,
    pub width: f32,
    pub min_width: f32,
    pub max_width: f32,
}

/// Selects the sidebar's dock edge; its inner border and resize handle face the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideTabsPosition {
    Left,
    #[default]
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SideTab {
    pub id: String,
    pub label: String,
    /// Optional status suffix, kept outside the editable name.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    pub closable: bool,
}

/// Panel-local anchor. The host clamps the popup to the visible window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PopupMenu {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub items: Vec<MenuItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub separator_before: bool,
}

fn identity(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}
fn items<'a>(items: impl Iterator<Item = (&'a str, &'a str)>) -> Result<(), String> {
    let mut ids = std::collections::BTreeSet::new();
    let mut bytes = 0;
    for (id, label) in items {
        bytes += label.len();
        if !identity(id)
            || !ids.insert(id)
            || ids.len() > 512
            || label.len() > 4096
            || bytes > 65536
        {
            return Err("Invalid canvas controls items or quota exceeded".into());
        }
    }
    Ok(())
}
impl SideTabs {
    /// Validate collection identities, selection and geometry before creating native controls.
    pub fn validate(&self) -> Result<(), String> {
        let tabs = self;

        if !identity(&tabs.id)
            || ![tabs.width, tabs.min_width, tabs.max_width]
                .iter()
                .all(|v| v.is_finite() && *v >= 0. && *v <= 1200.)
            || tabs.min_width > tabs.width
            || tabs.width > tabs.max_width
        {
            return Err("Invalid sidebar geometry".into());
        }
        items(
            tabs.items
                .iter()
                .map(|item| (item.id.as_str(), item.label.as_str())),
        )?;
        if tabs
            .items
            .iter()
            .any(|item| item.status.as_ref().is_some_and(|s| s.len() > 4096))
            || tabs
                .items
                .iter()
                .map(|item| item.label.len() + item.status.as_ref().map_or(0, |s| s.len()))
                .sum::<usize>()
                > 65536
        {
            return Err("Sidebar status quota exceeded".into());
        }
        for selected in [&tabs.selected, &tabs.rename].into_iter().flatten() {
            if !tabs
                .items
                .iter()
                .any(|item| &item.id == selected && !item.disabled)
            {
                return Err("Unknown/disabled sidebar item".into());
            }
        }
        Ok(())
    }
}
impl PopupMenu {
    /// Bound popup anchors and item workload before native layout.
    pub fn validate(&self) -> Result<(), String> {
        let menu = self;

        if !identity(&menu.id)
            || ![menu.x, menu.y]
                .iter()
                .all(|v| v.is_finite() && *v >= 0. && *v <= 10000.)
        {
            return Err("Invalid popup identity/anchor".into());
        }
        items(
            menu.items
                .iter()
                .map(|item| (item.id.as_str(), item.label.as_str())),
        )?;
        Ok(())
    }
}
