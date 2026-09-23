use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionState {
    pub workspace: String,
    pub window_width: f32,
    pub window_height: f32,
    pub explorer_width: f32,
    #[serde(default = "default_true")]
    pub explorer_visible: bool,
    pub output_height: f32,
    pub panel_visible: bool,
    pub open_tabs: Vec<String>,
    pub active_file: Option<String>,
    pub expanded_directories: Vec<String>,
}

impl SessionState {
    pub fn for_workspace(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_string_lossy().into_owned(),
            window_width: 1280.,
            window_height: 820.,
            explorer_width: 280.,
            explorer_visible: true,
            output_height: 160.,
            panel_visible: false,
            open_tabs: Vec::new(),
            active_file: None,
            expanded_directories: Vec::new(),
        }
    }

    fn file_path(&self) -> Option<PathBuf> {
        let mut hasher = DefaultHasher::new();
        self.workspace.hash(&mut hasher);
        Some(
            dirs::config_dir()?
                .join("MeEditor")
                .join(format!("{:016x}.json", hasher.finish())),
        )
    }

    pub fn load(workspace: &Path) -> Self {
        let mut state = Self::for_workspace(workspace);
        if let Some(path) = state.file_path() {
            if let Ok(contents) = fs::read_to_string(path) {
                if let Ok(saved) = serde_json::from_str::<Self>(&contents) {
                    if saved.workspace == state.workspace {
                        state = saved;
                        state.window_width = state.window_width.clamp(800., 7680.);
                        state.window_height = state.window_height.clamp(500., 4320.);
                        state.explorer_width = state.explorer_width.clamp(220., 520.);
                        state.output_height = state.output_height.clamp(100., 420.);
                    }
                }
            }
        }
        state
    }

    pub fn save(&self) {
        let Some(path) = self.file_path() else { return };
        let Some(parent) = path.parent() else { return };
        if fs::create_dir_all(parent).is_ok() {
            if let Ok(json) = serde_json::to_vec_pretty(self) {
                let temporary = path.with_extension("json.tmp");
                if fs::write(&temporary, json).is_ok() {
                    let _ = fs::rename(temporary, path);
                }
            }
        }
    }
}

fn default_true() -> bool {
    true
}
