//! Explicit file-provider choice and native recovery, scoped to the owning workspace and file type.
use super::*;
use crate::app::session::FileProviderChoice;
use crate::ui::controls::menu::{MenuStyle, PopupMenu};

/// Display ordering never selects a provider; stable keys preserve a user's valid prior choice.
struct Candidate {
    key: String,
    title: String,
    available: bool,
}

impl EditorApp {
    /// Auxiliary functions receive only compatible authorized files; only selection grants layout ownership.
    pub(super) fn file_panel_is_active(
        &self,
        panel: &Entity<ExtensionPanel>,
        selected: &Option<Entity<ExtensionPanel>>,
        cx: &App,
    ) -> bool {
        if selected.as_ref() == Some(panel) {
            return true;
        }
        let panel = panel.read(cx);
        if !panel.editor_auxiliary || !panel.visible.get() {
            return false;
        }
        let Some(extension) = self.file_provider_type() else {
            return false;
        };
        panel.entries.iter().any(|entry| {
            entry.enabled
                && entry.error.is_none()
                && entry.grants.contains("editor.read")
                && Some(&entry.manifest.id) == panel.active.as_ref()
                && entry.manifest.panels.iter().any(|descriptor| {
                    Some(&descriptor.id) == panel.surface_id.as_ref()
                        && (if self.active_text_tab_index().is_some() {
                            &descriptor.file_extensions
                        } else {
                            &descriptor.readonly_file_extensions
                        })
                        .iter()
                        .any(|kind| kind.eq_ignore_ascii_case(&extension))
                })
        })
    }
    /// Retain the chosen identity even when no corresponding live surface is currently available.
    pub(super) fn remembered_file_provider_key(&self) -> Option<String> {
        match self
            .session_state
            .file_view_providers
            .get(&self.file_provider_type()?)?
        {
            FileProviderChoice::Plugin(key) => Some(key.clone()),
            FileProviderChoice::Native => None,
        }
    }
    /// Binary fallback explains ambiguity instead of blaming an authorized candidate's permissions.
    pub(super) fn file_provider_selection_needed(&self, cx: &App) -> bool {
        self.file_provider_type()
            .is_some_and(|kind| !self.session_state.file_view_providers.contains_key(&kind))
            && self
                .file_provider_candidates(cx)
                .iter()
                .filter(|c| c.available)
                .count()
                > 1
    }
    /// The same file type shares one selection in this private workspace session.
    fn file_provider_type(&self) -> Option<String> {
        // File providers and their remembered choices describe local files, never provider-owned URIs.
        if self
            .tabs
            .get(self.active_tab_index()?)?
            .virtual_document
            .is_some()
        {
            return None;
        }
        Some(
            self.active_path
                .as_ref()?
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase(),
        )
    }

    /// Include unavailable compatible packages in the menu so failures do not silently choose another.
    fn file_provider_candidates(&self, cx: &App) -> Vec<Candidate> {
        let Some(extension) = self.file_provider_type() else {
            return Vec::new();
        };
        let has_text = self.active_text_tab_index().is_some();
        let mut matches = Vec::new();
        for entry in &self.extensions.read(cx).entries {
            for panel in &entry.manifest.panels {
                let types = if has_text {
                    &panel.file_extensions
                } else {
                    &panel.readonly_file_extensions
                };
                if panel.position != "editor"
                    || panel.auxiliary
                    || !types.iter().any(|s| s.eq_ignore_ascii_case(&extension))
                {
                    continue;
                }
                let key = format!("{}/{}", entry.manifest.id, panel.id);
                matches.push(Candidate {
                    available: self.session_state.workspace_trusted
                        && entry.enabled
                        && entry.error.is_none()
                        && entry.grants.contains("editor.read")
                        && self
                            .plugin_panels
                            .get(&key)
                            .is_some_and(|p| p.read(cx).visible.get()),
                    key,
                    title: format!("{} · {}", entry.manifest.name, panel.title),
                });
            }
        }
        matches.sort_by(|a, b| a.title.cmp(&b.title).then(a.key.cmp(&b.key)));
        matches
    }

    /// Auto-admit a unique candidate once; ambiguity always requires the user's explicit menu choice.
    pub(super) fn remember_single_file_provider(&mut self, cx: &App) {
        let Some(kind) = self.file_provider_type() else {
            return;
        };
        if self.session_state.file_view_providers.contains_key(&kind) {
            return;
        }
        let choices = self
            .file_provider_candidates(cx)
            .into_iter()
            .filter(|c| c.available)
            .collect::<Vec<_>>();
        if choices.len() == 1 {
            self.session_state
                .file_view_providers
                .insert(kind, FileProviderChoice::Plugin(choices[0].key.clone()));
            self.persist_session();
        }
    }

    /// Only the remembered, live provider may own the center; auxiliary tools do not enter this choice.
    pub(super) fn selected_file_provider(&self, cx: &App) -> Option<Entity<ExtensionPanel>> {
        let kind = self.file_provider_type()?;
        let candidates = self.file_provider_candidates(cx);
        let key = match self.session_state.file_view_providers.get(&kind) {
            Some(FileProviderChoice::Native) => return None,
            Some(FileProviderChoice::Plugin(key)) => key.clone(),
            None => {
                let available = candidates
                    .iter()
                    .filter(|c| c.available)
                    .collect::<Vec<_>>();
                if available.len() != 1 {
                    return None;
                }
                available[0].key.clone()
            }
        };
        candidates
            .iter()
            .any(|c| c.available && c.key == key)
            .then(|| self.plugin_panels[&key].clone())
    }

    /// Selection advances transport epochs for matching open tabs, preserving native text/Undo state.
    fn choose_file_provider(
        &mut self,
        choice: FileProviderChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(kind) = self.file_provider_type() else {
            return;
        };
        if matches!(choice, FileProviderChoice::Native) && self.active_text_tab_index().is_none() {
            return;
        }
        if let FileProviderChoice::Plugin(key) = &choice
            && !self
                .file_provider_candidates(cx)
                .iter()
                .any(|c| c.available && &c.key == key)
        {
            return;
        }
        self.session_state
            .file_view_providers
            .insert(kind.clone(), choice);
        for tab in &mut self.tabs {
            if tab
                .path()
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .eq_ignore_ascii_case(&kind)
            {
                tab.file_revision = tab.file_revision.saturating_add(1);
                if let Some(text) = &mut tab.text {
                    text.capability_revision = text.capability_revision.saturating_add(1);
                }
            }
        }
        self.prepare_file_layout_change(window, cx);
        self.invalidate_editor_previews(cx);
        self.sync_editor_previews(cx);
        if let Some(index) = self.active_text_tab_index() {
            self.tabs[index]
                .text
                .as_ref()
                .unwrap()
                .editor
                .focus_handle(cx)
                .focus(window, cx);
        }
        self.persist_session();
        self.editor_panel.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// A file Tab context menu offers generic recovery and all compatible providers without new tabs.
    pub(crate) fn open_file_provider_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self
            .active_tab_index()
            .and_then(|i| self.plugin_file_context(i).ok())
        else {
            return;
        };
        let mut choices = Vec::new();
        let mut items = Vec::new();
        if self.active_text_tab_index().is_some() {
            choices.push(FileProviderChoice::Native);
            items.push(protocol::ui::MenuItem {
                id: "provider-0".into(),
                label: t!("file_view.restore_text").to_string(),
                disabled: false,
                separator_before: false,
            });
        }
        for candidate in self.file_provider_candidates(cx) {
            let index = choices.len();
            items.push(protocol::ui::MenuItem {
                id: format!("provider-{index}"),
                label: candidate.title,
                disabled: !candidate.available,
                separator_before: index == 1,
            });
            choices.push(FileProviderChoice::Plugin(candidate.key));
        }
        let target = self.plugin_menu_target(self.active_path.as_deref().unwrap());
        let command_rows =
            self.plugin_menu_entries(&target, &[protocol::commands::Location::Tab], cx);
        let mut group = None;
        for (index, row) in command_rows.iter().enumerate() {
            let separator_before = group.as_ref() != Some(&row.group);
            group = Some(row.group.clone());
            items.push(protocol::ui::MenuItem {
                id: format!("plugin-command-{index}"),
                label: row.label.clone(),
                disabled: row.disabled,
                separator_before,
            });
        }
        let parent = cx.entity().downgrade();
        // Escape restores the clicked file's input target, rather than a previously active Tab.
        // A content-only layout has no mounted native handler and will retire this focus on redraw.
        if self.active_text_tab_index().is_some() {
            self.editor.focus_handle(cx).focus(window, cx);
        }
        self.file_view_menu = Some(cx.new(|cx| {
            PopupMenu::new(
                items,
                MenuStyle::current(cx),
                position,
                move |action, window, cx| {
                    let _ = parent.update(cx, |app, cx| {
                        app.file_view_menu = None;
                        // Native popup focus restoration is provisional until the captured file is revalidated.
                        if app
                            .active_tab_index()
                            .and_then(|i| app.plugin_file_context(i).ok())
                            .as_ref()
                            != Some(&file)
                        {
                            return;
                        }
                        if let protocol::ui::Action::Select(id) = &action
                            && let Some(index) = id
                                .strip_prefix("plugin-command-")
                                .and_then(|id| id.parse::<usize>().ok())
                            && let Some(row) = command_rows.get(index)
                        {
                            app.invoke_plugin_menu(row, &target, window, cx);
                            return;
                        }
                        if let protocol::ui::Action::Select(id) = action
                            && let Some(index) = id
                                .strip_prefix("provider-")
                                .and_then(|s| s.parse::<usize>().ok())
                            && let Some(choice) = choices.get(index)
                        {
                            app.choose_file_provider(choice.clone(), window, cx);
                        }
                        cx.notify();
                    });
                },
                window,
                cx,
            )
        }));
        cx.notify();
    }
}
