//! Workspace-local logical terminal snapshots, committed atomically outside the project directory.

use super::*;

#[derive(Serialize, Deserialize)]
struct SavedSession {
    id: u64,
    name: String,
    profile: Profile,
    cwd: String,
    exited: bool,
    grid: engine::SavedGrid,
}
#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    settings: Settings,
    active: Option<u64>,
    next_id: u64,
    tab_width: f32,
    sessions: Vec<SavedSession>,
}

pub(super) fn directory(workspace: &Path) -> PathBuf {
    let key = format!(
        "{:x}",
        Sha256::digest(workspace.to_string_lossy().as_bytes())
    );
    editor_core::default_root()
        .unwrap_or_else(std::env::temp_dir)
        .join("terminal")
        .join(key)
}

impl TerminalPanel {
    /// Accept only the native logical format here; importing the old guest format is a separate upgrade.
    pub(super) fn restore(&mut self) -> anyhow::Result<()> {
        // Validate the whole snapshot and external settings before replacing any live state.
        let mut settings = self.settings.clone();
        let mut sessions = Vec::new();
        let mut next_id = self.next_id;
        let mut active = self.active;
        let mut tab_width = self.tab_width;
        let path = self.storage.join("state.json");
        if path.exists() {
            anyhow::ensure!(
                std::fs::metadata(&path)?.len() <= 8 * 1024 * 1024,
                t!("terminal.storage_quota")
            );
            let bytes = std::fs::read(path)?;
            anyhow::ensure!(bytes.len() <= 8 * 1024 * 1024, t!("terminal.storage_quota"));
            let saved: Saved = serde_json::from_slice(&bytes)?;
            anyhow::ensure!(
                saved.version == 1 && saved.sessions.len() <= 32,
                t!("terminal.invalid_snapshot")
            );
            settings = Settings::parse(&serde_json::to_string(&saved.settings)?)?;
            let ids = saved
                .sessions
                .iter()
                .map(|session| session.id)
                .collect::<std::collections::BTreeSet<_>>();
            anyhow::ensure!(
                ids.len() == saved.sessions.len()
                    && !ids.contains(&0)
                    && ids.last().is_none_or(|id| *id <= saved.next_id),
                t!("terminal.invalid_snapshot")
            );
            next_id = saved.next_id;
            active = saved
                .active
                .filter(|id| ids.contains(id))
                .or_else(|| saved.sessions.first().map(|session| session.id));
            anyhow::ensure!(saved.tab_width.is_finite(), t!("terminal.invalid_snapshot"));
            tab_width = saved.tab_width.clamp(112., 480.);
            for tab in saved.sessions {
                anyhow::ensure!(
                    (1..=500).contains(&tab.grid.rows) && (2..=1000).contains(&tab.grid.columns),
                    t!("terminal.invalid_snapshot")
                );
                let mut engine = Engine::new(
                    GridSize {
                        rows: tab.grid.rows,
                        columns: tab.grid.columns,
                    },
                    settings.history,
                );
                engine.restore(tab.grid)?;
                sessions.push(Session {
                    id: tab.id,
                    name: tab.name,
                    profile: tab.profile,
                    cwd: tab.cwd,
                    engine,
                    launched: false,
                    exited: tab.exited,
                    restored: true,
                });
            }
        }
        let settings_path = self.storage.join("settings.json");
        if settings_path.exists() {
            anyhow::ensure!(
                std::fs::metadata(&settings_path)?.len() <= 65536,
                t!("terminal.storage_quota")
            );
            settings = Settings::parse_for_os(
                &std::fs::read_to_string(settings_path)?,
                std::env::consts::OS,
            )?;
        }
        for session in &mut sessions {
            session.engine.set_history(settings.history);
        }
        self.settings = settings;
        self.sessions = sessions;
        self.next_id = next_id;
        self.active = active;
        self.tab_width = tab_width;
        Ok(())
    }

    /// Atomically replace only this workspace's snapshot; a failed write keeps the original intact.
    pub(super) fn save(&mut self) -> anyhow::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        anyhow::ensure!(!self.persistence_blocked, t!("terminal.invalid_snapshot"));
        let mut saved = Saved {
            version: 1,
            settings: self.settings.clone(),
            active: self.active,
            next_id: self.next_id,
            tab_width: self.tab_width,
            sessions: self
                .sessions
                .iter()
                .map(|tab| SavedSession {
                    id: tab.id,
                    name: tab.name.clone(),
                    profile: tab.profile.clone(),
                    cwd: tab.cwd.clone(),
                    exited: tab.exited,
                    grid: tab.engine.snapshot(),
                })
                .collect(),
        };
        let mut bytes = serde_json::to_vec(&saved)?;
        // Storage quota discards oldest history, never visible cells, names, settings or active identity.
        while bytes.len() > 8 * 1024 * 1024 {
            let mut reduced = false;
            for session in &mut saved.sessions {
                let history = session.grid.lines.len().saturating_sub(session.grid.rows);
                if history > 0 {
                    session.grid.lines.drain(..history.div_ceil(2));
                    session.grid.offset = session
                        .grid
                        .offset
                        .min(session.grid.lines.len() - session.grid.rows);
                    reduced = true;
                }
            }
            anyhow::ensure!(reduced, t!("terminal.storage_quota"));
            bytes = serde_json::to_vec(&saved)?;
        }
        anyhow::ensure!(bytes.len() <= 8 * 1024 * 1024, t!("terminal.storage_quota"));
        std::fs::create_dir_all(&self.storage)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.storage)?;
        std::io::Write::write_all(&mut temporary, &bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(self.storage.join("state.json"))?;
        self.dirty = false;
        Ok(())
    }
}
