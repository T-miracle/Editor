mod app;
mod editor;
mod explorer;
mod extensions;
pub mod language;
mod sdk_export;
#[cfg(test)]
mod tests;
mod ui;

// Compile translations from the app's locale files and retain English as fallback.
rust_i18n::i18n!("locales", fallback = "en");

use editor::file_watch::{FileWatch, Reconciliation};
use editor_core::{DocumentSession, Workspace, WorkspaceSnapshot};
use futures::StreamExt;
use gpui_base::dock::{DockArea, DockEvent, DockLayout, PanelEvent};
use gpui_base::input::RopeExt as _;
use gpui_base::input::{
    EditorState, InputEvent, TabSize, TextDecoration, TextDecorationCollection,
};
use gpui_base::{TreeEvent, TreeItem, TreeState};
use gpui_kit::{
    App, AppContext as _, Bounds, ClickEvent, Context, Entity, EventEmitter, FocusHandle,
    Focusable, HighlightStyle, InteractiveElement as _, IntoElement, KeyBinding, Modifiers,
    MouseButton, MouseDownEvent, MouseUpEvent, ParentElement, Pixels, PlatformInput, Point, Render,
    ScrollHandle, ScrollStrategy, ScrollWheelEvent, StatefulInteractiveElement, StyleRefinement,
    Styled, Subscription, WeakEntity, Window, WindowBounds, WindowControlArea, WindowHandle,
    actions,
    component::{
        ActiveTheme, IconName, Root, StyledExt, TitleBar,
        dock::{self as dock, Panel as DockPanel},
        h_flex,
        input::Editor,
        v_flex,
    },
    div, point,
    prelude::FluentBuilder,
    px, size,
};
use platform_windows::{LocalHistory, NativeFileStore};
use plugin_schema::ThemeComponent;
use rust_i18n::t;
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(target_os = "windows")]
use app::WindowsTimerResolution;
use app::dialog as app_dialog;
use app::plugins::{PluginLoadEntry, PluginPopupKind};
use app::session as session_state;
use app::{EditorDockPanel, EditorDockPanelKind};
use assets::AppAssets;
use explorer::menu::ExplorerMenu;
use explorer::tree as explorer_tree;
use explorer::{ExplorerDelete, ExplorerEdit, ExplorerEditKind};
use explorer_tree::{find_tree_item, restore_expanded, tree_items};
use icons::file_icon;
use language::navigation as language_navigation;
pub use language::plugins as language_plugins;
use local_dock::LocalDock;
use session_state::SessionState;
#[cfg(test)]
use theme::builtin_theme;
use theme::{apply_theme, component_styles};
use ui::controls::dock as local_dock;
use ui::controls::{Button, Icon, StatusBar, Tooltip};
use ui::{assets, icons, theme, typography};

const EXPLORER_INITIAL_WIDTH: f32 = 280.;
/// Shared height for the Explorer title bar and editor tab bar, in pixels.
const PANEL_HEADER_HEIGHT: f32 = 28.;
actions!(
    me_editor,
    [
        SaveDocument,
        RefreshWorkspace,
        ToggleTheme,
        NavigateToDefinition,
        ShowDefinitionDetails,
        NextSyntaxError,
        PreviousSyntaxError
    ]
);

struct EditorApp {
    workspace: Workspace,
    /// Native events and fallback scans send immutable disk snapshots to the UI thread.
    file_watch: FileWatch,
    workspace_snapshot: Option<WorkspaceSnapshot>,
    language_servers: HashMap<String, Arc<language_navigation::LanguageServer>>,
    file_store: NativeFileStore,
    history: Option<LocalHistory>,
    editor: Entity<EditorState>,
    /// The dock entity must repaint when editor-owned popover state changes.
    editor_panel: Entity<EditorDockPanel>,
    /// Keeps keyboard selection and scrolling in the project-owned completion popover.
    completion_popup: Rc<editor::CompletionPopupState>,
    /// Text selection may take focus without dismissing the displayed details.
    definition_popup_focus: editor::DefinitionPopupFocus,
    /// Observe navigation before native keybindings consume their events.
    _hover_keyboard_subscription: Subscription,
    /// Preserve a pressed selection until it becomes a text move or an ordinary click.
    editor_text_drag: editor::TextDragState,
    /// Cancels app-level hover requests when the pointer moves to another symbol.
    pointer_hover_generation: u64,
    /// Keep cached details and Escape suppression within one document revision.
    pointer_hover_context: Option<(Option<PathBuf>, Option<u64>)>,
    pointer_hover_symbol: Option<std::ops::Range<usize>>,
    pointer_hover_pending: bool,
    /// Reuse the current symbol's details when native mouse handling clears its card.
    pointer_hover_cached: Option<lsp_types::Hover>,
    /// Escape keeps the current symbol dismissed until the pointer leaves it.
    pointer_hover_suppressed: Option<std::ops::Range<usize>>,
    /// Repaint the host when the upstream editor publishes a hover or completion.
    _editor_observer: Subscription,
    tree_state: Entity<TreeState>,
    dock_area: Entity<DockArea>,
    /// Host panels keep their original identity when Base reloads a saved split tree.
    explorer_panel: Entity<EditorDockPanel>,
    /// Saved plugin leaves are restored after the startup registry becomes available.
    pending_dock_restore: bool,
    /// Generic runtime plugin dock; packages own all feature behavior.
    extensions: Entity<extensions::ExtensionPanel>,
    /// Installed manifests dynamically contribute native dock panel entities.
    plugin_panels: HashMap<String, Entity<extensions::ExtensionPanel>>,
    /// The manager owns a modal window independently from settings and plugin dock surfaces.
    extensions_window: Option<WindowHandle<Root>>,
    _extensions_closed_subscription: Option<Subscription>,
    /// Deferred native editor navigation requested by a permission-checked plugin.
    pending_plugin_file: Option<PathBuf>,
    /// Native close waits for plugin snapshots before requesting platform shutdown.
    shutting_down: bool,
    explorer_visible: bool,
    explorer_visibility: Rc<Cell<bool>>,
    tabs_scroll: ScrollHandle,
    tabs_hovered: bool,
    titlebar_should_move: bool,
    dialog: Option<Entity<app_dialog::AppDialog>>,
    /// Native settings window handle used to activate an already open window.
    dialog_window: Option<WindowHandle<Root>>,
    /// Removes the settings view when its native window closes.
    _dialog_closed_subscription: Option<Subscription>,
    /// Remembers the selected settings category while the dialog is reopened.
    settings_section: app::SettingsSection,
    /// Preserve unsaved plugin form drafts while switching settings categories.
    plugin_settings: Option<Entity<extensions::SettingsView>>,
    /// Temporary name field for create and rename commands in the explorer.
    explorer_edit: Option<ExplorerEdit>,
    /// A path is removed only after the delete preview is explicitly confirmed.
    explorer_delete: Option<ExplorerDelete>,
    explorer_menu: Option<ExplorerMenu>,
    tabs: Vec<OpenTab>,
    active_path: Option<PathBuf>,
    /// Restoring saved tabs must not reveal files inside directories the user left collapsed.
    restoring_documents: bool,
    status: String,
    definition_notice: Option<DefinitionNotice>,
    /// A local nonmodal card replaces informational dialogs and never owns the editor's focus.
    notification: Option<Entity<ui::controls::Notification>>,
    definition_request_id: u64,
    plugin_loads: Vec<PluginLoadEntry>,
    /// Rejects completion from a grammar task belonging to an older package version.
    plugin_loading_generation: u64,
    /// Apply package lifecycle changes on the next frame with access to the editor window.
    pending_contribution_sync: bool,
    /// Requests are routed through the editor window; immutable save snapshots run off the UI thread.
    pending_editor_requests: Vec<(String, plugin_runtime::EditorRequest)>,
    plugin_saves: std::collections::BTreeSet<PathBuf>,
    /// Last published open identities allow versioned close notifications without retaining document text.
    plugin_documents:
        std::collections::BTreeMap<String, plugin_runtime::plugin_protocol::api::DocumentVersion>,
    plugin_popup: Option<(PluginPopupKind, Point<Pixels>)>,
    dark_theme: bool,
    session_state: SessionState,
    _tree_subscription: Subscription,
    _dock_subscription: Subscription,
    _bounds_subscription: Option<Subscription>,
    _activation_subscription: Option<Subscription>,
}

/// Anchors a short definition lookup message above the clicked window position.
#[derive(Clone, Copy)]
struct DefinitionNotice {
    position: Point<Pixels>,
    request_id: u64,
}

struct OpenTab {
    /// Unlike the dirty revision, this also advances on disk reloads and other programmatic changes.
    capability_revision: u64,
    session: DocumentSession,
    editor: Entity<EditorState>,
    /// Hash of the last disk text, avoiding a second full copy of every open document.
    disk_digest: [u8; 32],
    /// Ignore worker reads that began before the latest successful local save.
    last_saved_at: Instant,
    disk_state: DiskState,
    suppress_change: bool,
    overwrite_confirmed: bool,
    /// A separate decoration layer keeps a definition jump visible for two seconds.
    definition_highlight: TextDecorationCollection,
    definition_highlight_generation: u64,
    /// Diagnostics retain only derived parser state; EditorState owns the editable text.
    diagnostics: editor::diagnostics::DocumentDiagnostics,
    _subscription: Subscription,
    _observer: Subscription,
}

/// Open tabs remain present when their backing file changes or disappears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiskState {
    Synced,
    Conflict,
    Deleted,
}

impl EditorApp {
    fn new(
        workspace: Workspace,
        initial_file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (file_watch, mut watch_updates) = FileWatch::start(workspace.clone());
        let closing = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = closing.update(cx, |app, cx| app.shutdown_plugins(cx));
            false
        });
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("text".to_string())
                .line_number(true)
                .indent_guides(true)
                .folding(true)
                .tab_size(TabSize {
                    tab_size: 4,
                    hard_tabs: false,
                })
                .placeholder(t!("editor.select_file").to_string())
        });
        let session_state = SessionState::load(workspace.root());
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let tree_subscription = cx.subscribe(&tree_state, |this, _, event: &TreeEvent, cx| {
            let id = match event {
                TreeEvent::Expanded(id) => Some((id.to_string(), true)),
                TreeEvent::Collapsed(id) => Some((id.to_string(), false)),
            };
            if let Some((id, is_expanded)) = id {
                // The project root has a default expansion state independent of child folders.
                if Path::new(&id) == this.workspace.root() {
                    this.session_state.explorer_root_expanded = is_expanded;
                }
                this.session_state.expanded_directories.retain(|path| {
                    if is_expanded {
                        path != &id
                    } else {
                        // Collapsing by mouse or keyboard also forgets every descendant's expansion.
                        !Path::new(path).starts_with(Path::new(&id))
                    }
                });
                if is_expanded {
                    this.session_state.expanded_directories.push(id);
                } else {
                    let roots = explorer_tree::root_items(this.tree_state.read(cx));
                    if let Some(item) = find_tree_item(&roots, Path::new(&id)) {
                        explorer_tree::collapse_descendants(item);
                    }
                }
                this.session_state.save();
            }
            cx.notify();
        });
        let parent = cx.entity().downgrade();
        let explorer_visibility = Rc::new(Cell::new(session_state.explorer_visible));
        let extension_visibility = Rc::new(Cell::new(false));
        let explorer_panel = cx.new(|cx| {
            EditorDockPanel::new(
                parent.clone(),
                EditorDockPanelKind::Explorer,
                explorer_visibility.clone(),
                cx,
            )
        });
        let editor_panel = cx.new(|cx| {
            EditorDockPanel::new(
                parent.clone(),
                EditorDockPanelKind::Editor,
                explorer_visibility.clone(),
                cx,
            )
        });
        let popover_panel = editor_panel.downgrade();
        // The editor dock is a separate view, so notify it when LSP state changes.
        let editor_observer = cx.observe(&editor, move |_, _, cx| {
            let _ = popover_panel.update(cx, |_, cx| cx.notify());
        });
        let dock_area = LocalDock::new(PANEL_HEADER_HEIGHT).create_area(
            "me-editor-layout",
            Some(1),
            window,
            cx,
        );
        let extensions = cx.new(|cx| {
            extensions::ExtensionPanel::new(
                parent.clone(),
                workspace.root().to_owned(),
                extension_visibility.clone(),
                session_state.workspace_trusted,
                cx,
            )
        });
        dock_area.update(cx, |area, cx| {
            // Explorer and the editor share the center; plugin management has its own window.
            area.set_center(
                DockLayout::h_split()
                    .child(
                        DockLayout::tabs()
                            .panel_view(dock::panel_handle(explorer_panel.clone()), cx),
                        Some(px(session_state.explorer_width)),
                    )
                    .child(
                        DockLayout::tabs().panel_view(dock::panel_handle(editor_panel.clone()), cx),
                        None,
                    ),
                window,
                cx,
            );
        });
        let dock_subscription = cx.subscribe(&dock_area, |this, _, event, cx| {
            if matches!(event, DockEvent::LayoutChanged) {
                this.capture_dock_layout(cx);
                this.persist_session();
            }
        });
        let mut this = Self {
            workspace,
            file_watch,
            workspace_snapshot: None,
            language_servers: HashMap::new(),
            file_store: NativeFileStore,
            history: LocalHistory::for_current_user().ok(),
            editor,
            editor_panel,
            completion_popup: Rc::new(editor::CompletionPopupState::default()),
            definition_popup_focus: editor::DefinitionPopupFocus::new(window, cx),
            _hover_keyboard_subscription: Self::install_hover_keyboard_dismissal(window, cx),
            editor_text_drag: editor::TextDragState::default(),
            pointer_hover_generation: 0,
            pointer_hover_context: None,
            pointer_hover_symbol: None,
            pointer_hover_pending: false,
            pointer_hover_cached: None,
            pointer_hover_suppressed: None,
            _editor_observer: editor_observer,
            tree_state,
            dock_area,
            explorer_panel,
            pending_dock_restore: session_state.dock_layout.is_some(),
            extensions,
            plugin_panels: HashMap::new(),
            extensions_window: None,
            _extensions_closed_subscription: None,
            pending_plugin_file: None,
            shutting_down: false,
            explorer_visible: session_state.explorer_visible,
            explorer_visibility,
            tabs_scroll: ScrollHandle::new(),
            tabs_hovered: false,
            titlebar_should_move: false,
            dialog: None,
            dialog_window: None,
            _dialog_closed_subscription: None,
            settings_section: app::SettingsSection::AppearanceAndBehavior,
            plugin_settings: None,
            explorer_edit: None,
            explorer_delete: None,
            explorer_menu: None,
            tabs: Vec::new(),
            active_path: None,
            restoring_documents: true,
            status: t!("status.ready").to_string(),
            definition_notice: None,
            notification: None,
            definition_request_id: 0,
            plugin_loads: PluginLoadEntry::initial(),
            plugin_loading_generation: 0,
            pending_contribution_sync: false,
            pending_editor_requests: Vec::new(),
            plugin_saves: Default::default(),
            plugin_documents: Default::default(),
            plugin_popup: None,
            dark_theme: false,
            session_state,
            _tree_subscription: tree_subscription,
            _dock_subscription: dock_subscription,
            _bounds_subscription: None,
            _activation_subscription: None,
        };
        this.restore_dock_layout(window, cx);
        this.refresh_files(cx);

        let saved_tabs = this.session_state.open_tabs.clone();
        let desired_active = this.session_state.active_file.clone();
        let mut restored_any = false;
        for path in saved_tabs
            .into_iter()
            .map(PathBuf::from)
            .filter(|path| path.is_file())
        {
            restored_any = true;
            this.open_file(path, window, cx);
        }
        if let Some(active) = desired_active.map(PathBuf::from) {
            if let Some(index) = this.tabs.iter().position(|tab| {
                tab.session.path() == active.canonicalize().unwrap_or(active.clone())
            }) {
                this.activate_tab(index, window, cx);
            }
        }
        if let Some(path) = initial_file {
            // Explicit file requests can reveal their path after saved tabs finish restoring.
            this.restoring_documents = false;
            this.open_file(path, window, cx);
        } else if !restored_any {
            if let Some(path) = this.default_file() {
                this.open_file(path, window, cx);
            }
        }
        // The automatic fallback document also preserves a previously saved collapsed tree.
        this.restoring_documents = false;

        let focus = this.editor.focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));
        this.start_plugin_loading(cx);
        // The window observer repairs missed events after sleep or another app had focus.
        this._activation_subscription =
            Some(cx.observe_window_activation(window, |this, window, _cx| {
                if window.is_window_active() {
                    this.file_watch.reconcile();
                }
            }));
        // Apply all worker results on the GPUI thread; editor entities never cross threads.
        cx.spawn_in(window, async move |app, cx| {
            while let Some(update) = watch_updates.next().await {
                let _ = app.update_in(cx, |app, window, cx| {
                    app.apply_reconciliation(update, window, cx);
                });
            }
        })
        .detach();
        this
    }

    fn default_file(&self) -> Option<PathBuf> {
        ["README.md", "Cargo.toml"]
            .into_iter()
            .map(|name| self.workspace.root().join(name))
            .find(|path| path.is_file())
            .or_else(|| {
                self.workspace
                    .files()
                    .first()
                    .map(|file| file.absolute_path.clone())
            })
    }

    fn refresh_dialog(&self, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.dialog {
            dialog.update(cx, |_, cx| cx.notify());
        }
    }

    fn toggle_explorer(&mut self, cx: &mut Context<Self>) {
        self.explorer_visible = !self.explorer_visible;
        self.explorer_visibility.set(self.explorer_visible);
        self.dock_area.update(cx, |_, cx| cx.notify());
        self.persist_session();
        self.refresh_dialog(cx);
        cx.notify();
    }

    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dark_theme = !self.dark_theme;
        apply_theme(&theme::active_theme(self.dark_theme), cx);
        self.status = if self.dark_theme {
            "JetBrains 2023 Dark"
        } else {
            "JetBrains 2023 Light"
        }
        .into();
        window.refresh();
        self.refresh_dialog(cx);
        cx.notify();
    }

    fn on_save_action(&mut self, _: &SaveDocument, _: &mut Window, cx: &mut Context<Self>) {
        self.save_current(cx);
    }

    /// Requests a fresh definition at the caret without depending on hover state.
    fn on_navigate_to_definition(
        &mut self,
        _: &NavigateToDefinition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_definition(None, window, cx);
    }

    /// Ctrl+I requests the same hover information shown at the mouse position.
    fn on_show_definition_details(
        &mut self,
        _: &ShowDefinitionDetails,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source_path = self.active_path.clone();
        let source_revision = self
            .active_tab_index()
            .map(|index| self.tabs[index].session.revision());
        let request = self.editor.update(cx, |editor, cx| {
            let provider = editor.lsp().hover_provider.clone()?;
            let offset = editor.cursor();
            // The caret may sit immediately after the symbol, including at file end.
            let range = editor.text().word_range(offset).or_else(|| {
                offset
                    .checked_sub(1)
                    .and_then(|previous| editor.text().word_range(previous))
            })?;
            Some((
                offset,
                range,
                provider.hover(editor.text(), offset, window, cx),
            ))
        });
        let Some((offset, symbol_range, task)) = request else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |app, _, cx| {
                // A tab switch, edit, or cursor move invalidates the requested symbol.
                if app.active_path != source_path
                    || app
                        .active_tab_index()
                        .map(|index| app.tabs[index].session.revision())
                        != source_revision
                    || app.editor.read(cx).cursor() != offset
                {
                    return;
                }
                match result {
                    Ok(Some(hover)) => app.editor.update(cx, |editor, cx| {
                        editor.present_hover(symbol_range, hover, cx);
                    }),
                    Ok(None) => {}
                    Err(error) => tracing::warn!(%error, "definition details request failed"),
                }
            });
        })
        .detach();
    }

    /// Queries the current caret, with an optional mouse anchor for empty results.
    fn request_definition(
        &mut self,
        notice_position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.definition_request_id = self.definition_request_id.wrapping_add(1);
        let request_id = self.definition_request_id;
        if self.definition_notice.take().is_some() {
            cx.notify();
        }
        let source_path = self.active_path.clone();
        let source_revision = self
            .active_tab_index()
            .map(|index| self.tabs[index].session.revision());
        let task = self.editor.update(cx, |editor, cx| {
            let provider = editor.lsp_mut().definition_provider.clone()?;
            Some(provider.definitions(editor.text(), editor.cursor(), window, cx))
        });
        let Some(task) = task else {
            if let Some(position) = notice_position {
                self.show_no_definition(position, request_id, window, cx);
            }
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            if let Err(error) = &result {
                tracing::warn!(%error, "definition navigation failed");
            }
            let _ = this.update_in(cx, |app, window, cx| {
                // An edit, tab switch, or newer request invalidates this result.
                if app.definition_request_id != request_id
                    || app.active_path != source_path
                    || app
                        .active_tab_index()
                        .map(|index| app.tabs[index].session.revision())
                        != source_revision
                {
                    return;
                }
                let opened = result
                    .ok()
                    .and_then(|locations| locations.into_iter().next())
                    .is_some_and(|location| {
                        app.open_definition_uri(
                            &location.target_uri,
                            Some(location.target_selection_range),
                            window,
                            cx,
                        )
                    });
                if !opened && let Some(position) = notice_position {
                    app.show_no_definition(position, request_id, window, cx);
                }
            });
        })
        .detach();
    }

    /// Shows an ephemeral message and removes only that message after one second.
    fn show_no_definition(
        &mut self,
        position: Point<Pixels>,
        request_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.definition_notice = Some(DefinitionNotice {
            position,
            request_id,
        });
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update_in(cx, |app, _, cx| {
                if app
                    .definition_notice
                    .is_some_and(|notice| notice.request_id == request_id)
                {
                    app.definition_notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn on_refresh_action(&mut self, _: &RefreshWorkspace, _: &mut Window, cx: &mut Context<Self>) {
        self.file_watch.reconcile();
        self.status = t!("status.refreshing_workspace").to_string();
        cx.notify();
    }

    fn on_toggle_theme_action(
        &mut self,
        _: &ToggleTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_theme(window, cx);
    }
}

fn resolve_startup_target() -> anyhow::Result<(Workspace, Option<PathBuf>)> {
    let argument = std::env::args_os().nth(1).map(PathBuf::from);
    let target = argument.unwrap_or(std::env::current_dir()?);
    if target.is_file() {
        let file = target.canonicalize()?;
        let root = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        Ok((Workspace::open(root)?, Some(file)))
    } else {
        Ok((Workspace::open(target)?, None))
    }
}

fn main() -> anyhow::Result<()> {
    if sdk_export::run_cli()? {
        return Ok(());
    }
    // Use Simplified Chinese by default while keeping locale changes centralized.
    rust_i18n::set_locale("zh-CN");

    #[cfg(target_os = "windows")]
    let _timer_resolution = WindowsTimerResolution::enable_for_window_drag();

    let (workspace, initial_file) = resolve_startup_target()?;
    // Plain entries prevent bundled host grammars from running before plugin validation.
    language_plugins::prepare_bundled_plugins();
    let startup_state = SessionState::load(workspace.root());
    gpui_kit::application()
        .with_assets(AppAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(&theme::active_theme(false), cx);
            cx.activate(true);
            extensions::init(cx);
            cx.bind_keys([
                KeyBinding::new(
                    "ctrl-s",
                    SaveDocument,
                    Some("EditorShell && !PluginSurface"),
                ),
                KeyBinding::new(
                    "ctrl-shift-r",
                    RefreshWorkspace,
                    Some("EditorShell && !PluginSurface"),
                ),
                KeyBinding::new("ctrl-alt-t", ToggleTheme, Some("EditorShell")),
                KeyBinding::new(
                    "f12",
                    NavigateToDefinition,
                    Some("EditorShell && !PluginSurface"),
                ),
                KeyBinding::new(
                    "ctrl-i",
                    ShowDefinitionDetails,
                    Some("EditorShell && !PluginSurface"),
                ),
                // Error navigation follows the active document and wraps at either end.
                KeyBinding::new("f8", NextSyntaxError, Some("EditorShell && !PluginSurface")),
                KeyBinding::new(
                    "shift-f8",
                    PreviousSyntaxError,
                    Some("EditorShell && !PluginSurface"),
                ),
            ]);

            let bounds = Bounds::centered(
                None,
                size(
                    px(startup_state.window_width),
                    px(startup_state.window_height),
                ),
                cx,
            );
            let workspace = workspace.clone();
            let initial_file = initial_file.clone();
            let mut window_options = TitleBar::window_options();
            // Native materials are chosen by the active theme before first draw.
            window_options.window_background = theme::window_background(cx);
            window_options.window_bounds = Some(WindowBounds::Windowed(bounds));
            // GPUI's default throttles animations in inactive windows to 30 FPS.
            // Leave frame scheduling uncapped; active frames follow the display refresh rate.
            window_options.inactive_frame_interval = None;
            // Kit supplies the shared Root and its automatic overlay hosting.
            gpui_kit::open_window(window_options, cx, move |window, cx| {
                cx.new(|cx| EditorApp::new(workspace, initial_file, window, cx))
            })
            .expect("failed to open Me Editor window");
        });
    Ok(())
}
