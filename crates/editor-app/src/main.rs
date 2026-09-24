mod app_dialog;
mod assets;
mod icons;
mod local_dock;
mod session_state;
mod theme;
mod typography;

use editor_core::{DocumentSession, Workspace};
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, MouseButton, ParentElement, Render,
    ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement, Styled, Subscription, WeakEntity,
    Window, WindowBounds, WindowControlArea, actions,
    component::{
        ActiveTheme, Icon, IconName, Root, Sizable, StyledExt, TitleBar,
        button::{Button, ButtonVariants as _},
        dock::{self as dock, DockArea, DockEvent, DockLayout, Panel as DockPanel, PanelEvent},
        h_flex,
        input::{Editor, EditorState, InputEvent, TabSize},
        list::ListItem,
        status_bar::StatusBar,
        tree::{TreeEvent, TreeItem, TreeState, tree},
        v_flex,
    },
    div, point,
    prelude::FluentBuilder,
    px, size,
};
use platform_windows::{LocalHistory, NativeFileStore};
use plugin_schema::ThemeComponent;
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
};

use assets::AppAssets;
use icons::file_icon;
use local_dock::LocalDock;
use pinyin::ToPinyin;
use session_state::SessionState;
use theme::{apply_theme, builtin_theme, component_styles};

const EXPLORER_INITIAL_WIDTH: f32 = 280.;
/// Shared height for the Explorer title bar and editor tab bar, in pixels.
const PANEL_HEADER_HEIGHT: f32 = 28.;
#[cfg(target_os = "windows")]
const WINDOWS_TIMER_RESOLUTION_MS: u32 = 1;

#[cfg(target_os = "windows")]
#[link(name = "winmm")]
unsafe extern "system" {
    fn timeBeginPeriod(period: u32) -> u32;
    fn timeEndPeriod(period: u32) -> u32;
}

#[cfg(target_os = "windows")]
struct WindowsTimerResolution {
    enabled: bool,
}

#[cfg(target_os = "windows")]
impl WindowsTimerResolution {
    fn enable_for_window_drag() -> Self {
        // GPUI's Win32 modal move loop uses a short SetTimer interval to keep
        // processing and painting while the OS owns the title-bar drag loop.
        let enabled = unsafe { timeBeginPeriod(WINDOWS_TIMER_RESOLUTION_MS) } == 0;
        Self { enabled }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsTimerResolution {
    fn drop(&mut self) {
        if self.enabled {
            unsafe {
                timeEndPeriod(WINDOWS_TIMER_RESOLUTION_MS);
            }
        }
    }
}

actions!(
    me_editor,
    [
        SaveDocument,
        RefreshWorkspace,
        ToggleBottomPanel,
        ToggleTheme
    ]
);

/// Draws the same window glyph and native control region as GPUI Kit's TitleBar.
fn title_bar_window_control(
    id: &'static str,
    icon: IconName,
    area: WindowControlArea,
    close: bool,
    cx: &App,
) -> impl IntoElement {
    let hover_foreground = if close {
        cx.theme().danger_foreground
    } else {
        cx.theme().secondary_foreground
    };
    let hover_background = if close {
        cx.theme().danger
    } else {
        cx.theme().secondary_hover
    };
    let active_background = if close {
        cx.theme().danger_active
    } else {
        cx.theme().secondary_active
    };
    div()
        .id(id)
        .flex()
        .w(px(34.))
        .h_full()
        .flex_shrink_0()
        .justify_center()
        .content_center()
        .items_center()
        .text_color(cx.theme().foreground)
        .hover(|style| style.bg(hover_background).text_color(hover_foreground))
        .active(|style| style.bg(active_background).text_color(hover_foreground))
        .when(cfg!(target_os = "windows"), |this| {
            this.window_control_area(area)
        })
        .when(!cfg!(target_os = "windows"), |this| {
            this.on_click(move |_, window, cx| {
                cx.stop_propagation();
                match area {
                    WindowControlArea::Min => window.minimize_window(),
                    WindowControlArea::Max => window.zoom_window(),
                    WindowControlArea::Close => window.remove_window(),
                    WindowControlArea::Drag => {}
                }
            })
        })
        .child(Icon::new(icon).small())
}
struct EditorApp {
    workspace: Workspace,
    file_store: NativeFileStore,
    history: Option<LocalHistory>,
    editor: Entity<EditorState>,
    tree_state: Entity<TreeState>,
    dock_area: Entity<DockArea>,
    output_visible: Rc<Cell<bool>>,
    explorer_visible: bool,
    explorer_visibility: Rc<Cell<bool>>,
    tabs_scroll: ScrollHandle,
    tabs_hovered: bool,
    titlebar_should_move: bool,
    dialog: Option<Entity<app_dialog::AppDialog>>,
    hovered_tree_entry: Option<String>,
    tabs: Vec<OpenTab>,
    active_path: Option<PathBuf>,
    status: String,
    panel_visible: bool,
    dark_theme: bool,
    session_state: SessionState,
    _tree_subscription: Subscription,
    _dock_subscription: Subscription,
    _bounds_subscription: Option<Subscription>,
}

struct OpenTab {
    session: DocumentSession,
    editor: Entity<EditorState>,
    _subscription: Subscription,
}

#[derive(Clone)]
struct EditorTabDrag {
    path: PathBuf,
    label: String,
}

impl Render for EditorTabDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = component_styles(cx, ThemeComponent::EditorTabDragPreview).base;
        div()
            .px(px(style.padding_x_px.unwrap_or(8.)))
            .py(px(style.padding_y_px.unwrap_or(4.)))
            .rounded(px(style.radius_px.unwrap_or(4.)))
            .text_size(px(style.font_size_px.unwrap_or(12.)))
            .bg(style.background.unwrap_or(cx.theme().primary))
            .text_color(style.foreground.unwrap_or(cx.theme().primary_foreground))
            .child(self.label.clone())
    }
}

#[derive(Clone, Copy)]
enum EditorDockPanelKind {
    Explorer,
    Editor,
    Output,
}

struct EditorDockPanel {
    parent: WeakEntity<EditorApp>,
    kind: EditorDockPanelKind,
    output_visible: Rc<Cell<bool>>,
    explorer_visible: Rc<Cell<bool>>,
    focus_handle: FocusHandle,
}

impl EditorDockPanel {
    fn new(
        parent: WeakEntity<EditorApp>,
        kind: EditorDockPanelKind,
        output_visible: Rc<Cell<bool>>,
        explorer_visible: Rc<Cell<bool>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            parent,
            kind,
            output_visible,
            explorer_visible,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<PanelEvent> for EditorDockPanel {}

impl Focusable for EditorDockPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl dock::BasePanel for EditorDockPanel {
    fn panel_name(&self) -> &'static str {
        match self.kind {
            EditorDockPanelKind::Explorer => "Explorer",
            EditorDockPanelKind::Editor => "Editor",
            EditorDockPanelKind::Output => "Output",
        }
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }

    fn visible(&self, _: &App) -> bool {
        match self.kind {
            EditorDockPanelKind::Explorer => self.explorer_visible.get(),
            EditorDockPanelKind::Editor => true,
            EditorDockPanelKind::Output => self.output_visible.get(),
        }
    }
}

impl DockPanel for EditorDockPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.kind {
            EditorDockPanelKind::Explorer => div()
                .w_full()
                .h(px(PANEL_HEADER_HEIGHT))
                .flex()
                .items_center()
                .px(px(component_styles(cx, ThemeComponent::DockTitleBar)
                    .base
                    .padding_x_px
                    .unwrap_or(8.)))
                .text_size(px(component_styles(cx, ThemeComponent::DockTitleBar)
                    .base
                    .font_size_px
                    .unwrap_or(14.)))
                .font_normal()
                .bg(component_styles(cx, ThemeComponent::DockTitleBar)
                    .base
                    .background
                    .unwrap_or(cx.theme().tab_bar))
                .text_color(
                    component_styles(cx, ThemeComponent::DockTitleBar)
                        .base
                        .foreground
                        .unwrap_or(cx.theme().foreground),
                )
                .child("资源管理器")
                .into_any_element(),
            EditorDockPanelKind::Editor => div().child("编辑器").into_any_element(),
            EditorDockPanelKind::Output => div().child("Output").into_any_element(),
        }
    }

    fn title_bar(&self, _: &App) -> bool {
        !matches!(self.kind, EditorDockPanelKind::Editor)
    }
}

impl Render for EditorDockPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self.kind;
        self.parent
            .update(cx, |app, cx| match kind {
                EditorDockPanelKind::Explorer => app.render_file_tree(cx).into_any_element(),
                EditorDockPanelKind::Editor => {
                    app.render_editor_panel(window, cx).into_any_element()
                }
                EditorDockPanelKind::Output => app.render_panel(cx).into_any_element(),
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl EditorApp {
    fn new(
        workspace: Workspace,
        initial_file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
                .placeholder("Select a file from the Explorer")
        });
        let session_state = SessionState::load(workspace.root());
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let tree_subscription = cx.subscribe(&tree_state, |this, _, event: &TreeEvent, cx| {
            let id = match event {
                TreeEvent::Expanded(id) => Some((id.to_string(), true)),
                TreeEvent::Collapsed(id) => Some((id.to_string(), false)),
            };
            if let Some((id, is_expanded)) = id {
                this.session_state
                    .expanded_directories
                    .retain(|path| path != &id);
                if is_expanded {
                    this.session_state.expanded_directories.push(id);
                }
                this.session_state.save();
            }
            cx.notify();
        });
        let parent = cx.entity().downgrade();
        let output_visible = Rc::new(Cell::new(session_state.panel_visible));
        let explorer_visibility = Rc::new(Cell::new(session_state.explorer_visible));
        let explorer_panel = cx.new(|cx| {
            EditorDockPanel::new(
                parent.clone(),
                EditorDockPanelKind::Explorer,
                output_visible.clone(),
                explorer_visibility.clone(),
                cx,
            )
        });
        let editor_panel = cx.new(|cx| {
            EditorDockPanel::new(
                parent.clone(),
                EditorDockPanelKind::Editor,
                output_visible.clone(),
                explorer_visibility.clone(),
                cx,
            )
        });
        let output_panel = cx.new(|cx| {
            EditorDockPanel::new(
                parent,
                EditorDockPanelKind::Output,
                output_visible.clone(),
                explorer_visibility.clone(),
                cx,
            )
        });
        let dock_area = LocalDock::new(PANEL_HEADER_HEIGHT).create_area(
            "me-editor-layout",
            Some(1),
            window,
            cx,
        );
        dock_area.update(cx, |area, cx| {
            let editor_and_output = DockLayout::v_split()
                .child(
                    DockLayout::tabs().panel_view(dock::panel_handle(editor_panel), cx),
                    None,
                )
                .child(
                    DockLayout::tabs().panel_view(dock::panel_handle(output_panel), cx),
                    Some(px(session_state.output_height)),
                );
            area.set_center(
                DockLayout::h_split()
                    .child(
                        DockLayout::tabs().panel_view(dock::panel_handle(explorer_panel), cx),
                        Some(px(session_state.explorer_width)),
                    )
                    .child(editor_and_output, None),
                window,
                cx,
            );
        });
        let dock_subscription = cx.subscribe(&dock_area, |this, area, event, cx| {
            if matches!(event, DockEvent::LayoutChanged) {
                let layout = area.read(cx).dump(cx);
                if let Some(width) = layout.center.info.sizes().and_then(|sizes| sizes.first()) {
                    this.session_state.explorer_width = *width / px(1.);
                }
                if let Some(height) = layout
                    .center
                    .children
                    .get(1)
                    .and_then(|center| center.info.sizes())
                    .and_then(|sizes| sizes.get(1))
                {
                    this.session_state.output_height = *height / px(1.);
                }
                this.persist_session();
            }
        });
        let mut this = Self {
            workspace,
            file_store: NativeFileStore,
            history: LocalHistory::for_current_user().ok(),
            editor,
            tree_state,
            dock_area,
            output_visible,
            explorer_visible: session_state.explorer_visible,
            explorer_visibility,
            tabs_scroll: ScrollHandle::new(),
            tabs_hovered: false,
            titlebar_should_move: false,
            dialog: None,
            hovered_tree_entry: None,
            tabs: Vec::new(),
            active_path: None,
            status: "Ready".into(),
            panel_visible: session_state.panel_visible,
            dark_theme: false,
            session_state,
            _tree_subscription: tree_subscription,
            _dock_subscription: dock_subscription,
            _bounds_subscription: None,
        };
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
            this.open_file(path, window, cx);
        } else if !restored_any {
            if let Some(path) = this.default_file() {
                this.open_file(path, window, cx);
            }
        }

        let focus = this.editor.focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));
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

    fn refresh_files(&mut self, cx: &mut Context<Self>) {
        let root = self.workspace.root().to_path_buf();
        let files = self.workspace.files();
        let items = restore_expanded(
            tree_items(&root, files.iter().map(|file| file.absolute_path.as_path())),
            &self.session_state.expanded_directories,
        );
        let selected_item = self
            .active_path
            .as_ref()
            .and_then(|active| find_tree_item(&items, active))
            .cloned();
        self.tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
            state.set_selected_item(selected_item.as_ref(), cx);
        });
        self.status = format!("Workspace refreshed · {} files", files.len());
        cx.notify();
    }

    fn persist_session(&mut self) {
        self.session_state.open_tabs = self
            .tabs
            .iter()
            .map(|tab| tab.session.path().to_string_lossy().into_owned())
            .collect();
        self.session_state.active_file = self
            .active_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        self.session_state.panel_visible = self.panel_visible;
        self.session_state.explorer_visible = self.explorer_visible;
        self.session_state.save();
    }

    fn select_file_in_tree(&self, path: &Path, cx: &mut Context<Self>) {
        let root = self.workspace.root().to_path_buf();
        let files = self.workspace.files();
        let items = restore_expanded(
            tree_items(&root, files.iter().map(|file| file.absolute_path.as_path())),
            &self.session_state.expanded_directories,
        );
        let selected = find_tree_item(&items, path).cloned();
        self.tree_state.update(cx, |state, cx| {
            state.set_selected_item(selected.as_ref(), cx);
        });
    }

    fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path = path.canonicalize().unwrap_or(path);
        self.select_file_in_tree(&path, cx);
        if let Some(index) = self.tabs.iter().position(|tab| tab.session.path() == path) {
            self.activate_tab(index, window, cx);
            return;
        }

        match DocumentSession::open(&self.file_store, path) {
            Ok(opened) => {
                let language = language_for_path(opened.session.path()).to_string();
                let contents = opened.contents;
                let editor = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .language(language.clone())
                        .line_number(true)
                        .indent_guides(true)
                        .folding(true)
                        .tab_size(TabSize {
                            tab_size: 4,
                            hard_tabs: false,
                        })
                });
                editor.update(cx, |editor, cx| {
                    editor.set_highlighter(language, cx);
                    editor.set_value(contents, window, cx);
                });
                let subscription =
                    cx.subscribe(&editor, |this, changed_editor, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change)
                            && this.editor.entity_id() == changed_editor.entity_id()
                        {
                            if let Some(index) = this.active_tab_index() {
                                let tab = &mut this.tabs[index];
                                tab.session.note_edit();
                                this.status =
                                    format!("Modified · revision {}", tab.session.revision());
                            }
                            cx.notify();
                        }
                    });
                self.tabs.push(OpenTab {
                    session: opened.session,
                    editor,
                    _subscription: subscription,
                });
                self.activate_tab(self.tabs.len() - 1, window, cx);
            }
            Err(error) => self.status = format!("Open failed: {error}"),
        }
        cx.notify();
    }

    fn active_tab_index(&self) -> Option<usize> {
        let path = self.active_path.as_ref()?;
        self.tabs.iter().position(|tab| tab.session.path() == path)
    }

    fn activate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let path = tab.session.path().to_path_buf();
        let language = language_for_path(&path);
        let file_name = tab.session.file_name().unwrap_or("Untitled").to_string();

        self.active_path = Some(path);
        self.editor = tab.editor.clone();
        let focus = self.editor.focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));
        self.status = format!("Opened {file_name} · {language}");
        self.persist_session();
        cx.notify();
    }

    fn close_tab(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.session.path() == path) else {
            return;
        };
        if self.tabs[index].session.is_dirty() {
            self.status = "Save changes before closing this tab".into();
            cx.notify();
            return;
        }

        let was_active = self.active_path.as_ref() == Some(&path);
        self.tabs.remove(index);
        if !was_active {
            self.persist_session();
            cx.notify();
            return;
        }

        self.active_path = None;
        if !self.tabs.is_empty() {
            self.activate_tab(index.min(self.tabs.len() - 1), window, cx);
        } else {
            self.editor = cx.new(|cx| {
                EditorState::new(window, cx)
                    .language("text".to_string())
                    .line_number(true)
                    .indent_guides(true)
                    .folding(true)
                    .tab_size(TabSize {
                        tab_size: 4,
                        hard_tabs: false,
                    })
            });
            self.status = "No open files".into();
            self.persist_session();
            cx.notify();
        }
    }

    fn move_tab_before(&mut self, source: &Path, target: &Path, cx: &mut Context<Self>) {
        let Some(source_index) = self
            .tabs
            .iter()
            .position(|tab| tab.session.path() == source)
        else {
            return;
        };
        let Some(tab) = self.tabs.get(source_index) else {
            return;
        };
        if tab.session.path() == target {
            return;
        }

        let tab = self.tabs.remove(source_index);
        let target_index = self
            .tabs
            .iter()
            .position(|tab| tab.session.path() == target)
            .unwrap_or(self.tabs.len());
        self.tabs.insert(target_index, tab);
        self.persist_session();
        cx.notify();
    }

    fn save_current(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.active_tab_index() else {
            self.status = "Nothing to save".into();
            cx.notify();
            return;
        };
        if !self.tabs[index].session.is_dirty() {
            self.status = "No changes to save".into();
            cx.notify();
            return;
        }

        let value = self.editor.read(cx).value().to_string();
        if let Some(history) = &self.history {
            let _ = history.snapshot_file(self.tabs[index].session.path());
        }
        let tab = &mut self.tabs[index];
        match tab.session.save(&self.file_store, &value) {
            Ok(()) => {
                self.status = format!("Saved {}", tab.session.path().display());
            }
            Err(error) => self.status = format!("Save failed: {error}"),
        }
        cx.notify();
    }

    fn refresh_dialog(&self, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.dialog {
            dialog.update(cx, |_, cx| cx.notify());
        }
    }

    fn toggle_bottom_panel(&mut self, cx: &mut Context<Self>) {
        self.panel_visible = !self.panel_visible;
        self.output_visible.set(self.panel_visible);
        self.dock_area.update(cx, |_, cx| {
            cx.notify();
        });
        self.persist_session();
        self.refresh_dialog(cx);
        cx.notify();
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
        apply_theme(builtin_theme(self.dark_theme), cx);
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

    fn on_refresh_action(&mut self, _: &RefreshWorkspace, _: &mut Window, cx: &mut Context<Self>) {
        self.refresh_files(cx);
    }

    fn on_toggle_panel_action(
        &mut self,
        _: &ToggleBottomPanel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_bottom_panel(cx);
    }

    fn on_toggle_theme_action(
        &mut self,
        _: &ToggleTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_theme(window, cx);
    }

    fn render_file_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let tree_style = component_styles(cx, ThemeComponent::ExplorerTree).base;
        let row_styles = component_styles(cx, ThemeComponent::ExplorerRow);
        let tree = tree(
            &self.tree_state,
            move |index, entry, selected, _window, cx| {
                let hover_view = view.clone();
                view.update(cx, |app, cx| {
                    let item = entry.item();
                    let row_id = item.id.clone();
                    let hovered = app
                        .hovered_tree_entry
                        .as_deref()
                        .is_some_and(|id| id == row_id.as_str());
                    let row_style = if selected {
                        row_styles.selected
                    } else if hovered {
                        row_styles.hover
                    } else {
                        row_styles.base
                    };
                    let is_folder = item.is_folder();
                    let icon = file_icon(Path::new(item.id.as_str()), is_folder, false);
                    let disclosure = if is_folder {
                        Icon::new(if entry.is_expanded() {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall()
                        .into_any_element()
                    } else {
                        div().size(px(12.)).into_any_element()
                    };
                    ListItem::new(index)
                        .w_full()
                        .bg(row_style.background.unwrap_or(cx.theme().background))
                        .text_color(row_style.foreground.unwrap_or(cx.theme().foreground))
                        .py(px(row_style.padding_y_px.unwrap_or(0.3)))
                        .px(px(row_style.padding_x_px.unwrap_or(4.)))
                        .pl(px(14.) * entry.depth() + px(8.))
                        .when_some(row_style.border, |this, border| {
                            this.border_l_1().border_color(border)
                        })
                        .on_hover(move |is_hovered, _, cx| {
                            let row_id = row_id.clone();
                            let _ = hover_view.update(cx, |app, cx| {
                                if *is_hovered {
                                    app.hovered_tree_entry = Some(row_id.to_string());
                                    cx.notify();
                                } else if app.hovered_tree_entry.as_deref() == Some(row_id.as_str())
                                {
                                    app.hovered_tree_entry = None;
                                    cx.notify();
                                }
                            });
                        })
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .size(px(12.))
                                        .flex_shrink_0()
                                        .justify_center()
                                        .child(disclosure),
                                )
                                .child(div().size(px(16.)).flex_shrink_0().child(icon))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .truncate()
                                        .child(item.label.clone()),
                                ),
                        )
                        .on_click(cx.listener({
                            let item = item.clone();
                            move |this, _, window, cx| {
                                if !is_folder {
                                    this.open_file(PathBuf::from(item.id.as_str()), window, cx);
                                }
                            }
                        }))
                })
            },
        )
        .p_1()
        .text_size(px(tree_style.font_size_px.unwrap_or(12.)))
        .font_family(cx.theme().mono_font_family.clone())
        .flex_1()
        .min_h_0()
        .bg(tree_style.background.unwrap_or(cx.theme().background))
        .text_color(tree_style.foreground.unwrap_or(cx.theme().foreground));

        v_flex()
            .size_full()
            .min_h_0()
            .bg(tree_style.background.unwrap_or(cx.theme().background))
            .child(tree)
    }

    fn render_tabs(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_styles = component_styles(cx, ThemeComponent::EditorTab);
        let close_styles = component_styles(cx, ThemeComponent::EditorTabClose);
        let tabs = self.tabs.iter().map(|tab| {
            let path = tab.session.path().to_path_buf();
            let is_active = self.active_path.as_ref() == Some(&path);
            let is_dirty = tab.session.is_dirty();
            let name = tab.session.file_name().unwrap_or("Untitled").to_string();
            let icon = file_icon(&path, false, self.dark_theme);
            let activate_path = path.clone();
            let close_path = path.clone();
            let middle_close_path = path.clone();
            let drop_path = path.clone();
            let drag_payload = EditorTabDrag {
                path: path.clone(),
                label: name.clone(),
            };
            h_flex()
                .id(format!("editor-tab:{}", path.to_string_lossy()))
                .h_full()
                .w(px(190.))
                .flex_shrink_0()
                .gap_2()
                .px(px(tab_styles.base.padding_x_px.unwrap_or(8.)))
                .text_size(px(if is_active {
                    tab_styles
                        .selected
                        .font_size_px
                        .or(tab_styles.base.font_size_px)
                } else {
                    tab_styles.base.font_size_px
                }
                .unwrap_or(14.)))
                .border_r_1()
                .border_color(tab_styles.base.border.unwrap_or(cx.theme().border))
                .bg(if is_active {
                    tab_styles
                        .selected
                        .background
                        .unwrap_or(cx.theme().background)
                } else {
                    tab_styles.base.background.unwrap_or(cx.theme().tab_bar)
                })
                .text_color(if is_active {
                    tab_styles
                        .selected
                        .foreground
                        .unwrap_or(cx.theme().foreground)
                } else {
                    tab_styles
                        .base
                        .foreground
                        .unwrap_or(cx.theme().tab_foreground)
                })
                .hover(|style| {
                    style
                        .bg(tab_styles.hover.background.unwrap_or(if is_active {
                            tab_styles
                                .selected
                                .background
                                .unwrap_or(cx.theme().background)
                        } else {
                            tab_styles.base.background.unwrap_or(cx.theme().tab_bar)
                        }))
                        .text_color(tab_styles.hover.foreground.unwrap_or(cx.theme().foreground))
                })
                .child(div().size(px(16.)).flex_shrink_0().child(icon))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_sm()
                        .child(format!("{}{}", name, if is_dirty { " ●" } else { "" })),
                )
                .child(
                    div()
                        .id(format!("close-editor-tab:{}", path.to_string_lossy()))
                        .flex_shrink_0()
                        .rounded(px(close_styles.base.radius_px.unwrap_or(3.)))
                        .p_1()
                        .hover(|style| {
                            style.bg(close_styles
                                .hover
                                .background
                                .unwrap_or(cx.theme().list_hover))
                        })
                        .child(Icon::new(IconName::Close).xsmall())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_tab(close_path.clone(), window, cx);
                        })),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_file(activate_path.clone(), window, cx);
                }))
                .on_drag(drag_payload, |drag, _, _, cx| cx.new(|_| drag.clone()))
                .on_drop(cx.listener(move |this, drag: &EditorTabDrag, _, cx| {
                    this.move_tab_before(&drag.path, &drop_path, cx);
                }))
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.close_tab(middle_close_path.clone(), window, cx);
                    }),
                )
        });

        let measured_viewport = self.tabs_scroll.bounds().size.width;
        let viewport = if measured_viewport > px(0.) {
            measured_viewport
        } else {
            (window.bounds().size.width - px(EXPLORER_INITIAL_WIDTH) - px(24.)).max(px(0.))
        };
        let content_width = px(190.) * self.tabs.len();
        let max_scroll = if measured_viewport > px(0.) {
            self.tabs_scroll.max_offset().x
        } else {
            (content_width - viewport).max(px(0.))
        };
        let thumb_width = if max_scroll > px(0.) {
            (viewport * (viewport / (viewport + max_scroll)))
                .max(px(24.))
                .min(viewport)
        } else {
            viewport
        };
        let scroll_position = (-self.tabs_scroll.offset().x).clamp(px(0.), max_scroll);
        let thumb_left = if max_scroll > px(0.) && viewport > thumb_width {
            (scroll_position / max_scroll) * (viewport - thumb_width)
        } else {
            px(0.)
        };

        div()
            .id("editor-tabs-container")
            .relative()
            .w_full()
            .h(px(PANEL_HEADER_HEIGHT))
            .border_b_1()
            .border_color(
                component_styles(cx, ThemeComponent::EditorTabs)
                    .base
                    .border
                    .unwrap_or(cx.theme().border),
            )
            .bg(component_styles(cx, ThemeComponent::EditorTabs)
                .base
                .background
                .unwrap_or(cx.theme().tab_bar))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.tabs_hovered = *hovered;
                cx.notify();
            }))
            .child(
                div()
                    .id("editor-tabs-scroll")
                    .w_full()
                    .h_full()
                    .flex()
                    .flex_row()
                    .track_scroll(&self.tabs_scroll)
                    .overflow_x_scroll()
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                        let delta = event.delta.pixel_delta(window.line_height());
                        let scroll_delta = if delta.x != px(0.) { delta.x } else { delta.y };
                        let max_scroll = this.tabs_scroll.max_offset().x;
                        let current = this.tabs_scroll.offset().x;
                        let next = (current + scroll_delta).clamp(-max_scroll, px(0.));
                        if next != current {
                            this.tabs_scroll.set_offset(point(next, px(0.)));
                            cx.notify();
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                    }))
                    .child(
                        // Let the content grow to the combined tab width so GPUI
                        // retains a nonzero horizontal scroll range after layout.
                        h_flex()
                            .h_full()
                            .flex_none()
                            .w_auto()
                            .min_w_full()
                            .children(tabs),
                    ),
            )
            .when(max_scroll > px(0.) && self.tabs_hovered, |this| {
                let thumb_color = component_styles(cx, ThemeComponent::EditorTabs)
                    .active
                    .background
                    .unwrap_or(cx.theme().primary);
                this.child(
                    div()
                        .absolute()
                        .left(thumb_left)
                        .top_0()
                        .w(thumb_width)
                        .h(px(2.))
                        .bg(thumb_color),
                )
            })
    }

    fn render_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let style = component_styles(cx, ThemeComponent::OutputPanel).base;
        v_flex()
            .w_full()
            .size_full()
            .min_h_0()
            .px(px(style.padding_x_px.unwrap_or(12.)))
            .py(px(style.padding_y_px.unwrap_or(12.)))
            .gap_2()
            .bg(style.background.unwrap_or(cx.theme().muted))
            .child(
                h_flex()
                    .text_sm()
                    .text_size(px(style.font_size_px.unwrap_or(14.)))
                    .text_color(style.foreground.unwrap_or(cx.theme().muted_foreground))
                    .child(self.status.clone()),
            )
    }

    fn render_editor_panel(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = component_styles(cx, ThemeComponent::Editor).base;
        v_flex()
            .size_full()
            .min_h_0()
            .child(self.render_tabs(window, cx))
            .child(
                Editor::new(&self.editor)
                    .bordered(false)
                    .p_0()
                    .flex_1()
                    .min_h_0()
                    .bg(style.background.unwrap_or(cx.theme().background))
                    .text_color(style.foreground.unwrap_or(cx.theme().foreground))
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(
                        style
                            .font_size_px
                            .map(px)
                            .unwrap_or(cx.theme().mono_font_size),
                    )
                    .into_any_element(),
            )
    }

    fn render_panel_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_style = component_styles(cx, ThemeComponent::PanelToggle).selected;
        h_flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("explorer-panel-toggle")
                    .icon(IconName::PanelLeft)
                    .small()
                    .compact()
                    .ghost()
                    .tooltip("资源管理器")
                    .when(self.explorer_visible, |button| {
                        button
                            .bg(selected_style.background.unwrap_or(cx.theme().list_active))
                            .text_color(selected_style.foreground.unwrap_or(cx.theme().foreground))
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_explorer(cx))),
            )
            .child(
                Button::new("output-panel-toggle")
                    .icon(IconName::PanelBottom)
                    .small()
                    .compact()
                    .ghost()
                    .tooltip("Output")
                    .when(self.panel_visible, |button| {
                        button
                            .bg(selected_style.background.unwrap_or(cx.theme().list_active))
                            .text_color(selected_style.foreground.unwrap_or(cx.theme().foreground))
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_bottom_panel(cx))),
            )
    }
}

impl Render for EditorApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self._bounds_subscription.is_none() {
            self._bounds_subscription =
                Some(cx.observe_window_bounds(window, |this, window, _cx| {
                    let bounds = window.bounds();
                    this.session_state.window_width = bounds.size.width / px(1.);
                    this.session_state.window_height = bounds.size.height / px(1.);
                    this.session_state.save();
                }));
        }
        let cursor = self.editor.read(cx).cursor_position();
        let project_initial = self
            .workspace
            .root()
            .file_name()
            .map(|name| name.to_string_lossy())
            .and_then(|name| name.chars().next())
            .map(|initial| initial.to_uppercase().collect::<String>())
            .unwrap_or_else(|| "M".to_string());

        let shell_style = component_styles(cx, ThemeComponent::AppShell).base;
        let title_bar_style = component_styles(cx, ThemeComponent::WindowTitleBar).base;
        let badge_style = component_styles(cx, ThemeComponent::ProjectBadge).base;
        let status_style = component_styles(cx, ThemeComponent::StatusBar).base;
        v_flex()
            .id("editor-shell")
            .key_context("EditorShell")
            .on_action(cx.listener(Self::on_save_action))
            .on_action(cx.listener(Self::on_refresh_action))
            .on_action(cx.listener(Self::on_toggle_panel_action))
            .on_action(cx.listener(Self::on_toggle_theme_action))
            .size_full()
            .bg(shell_style.background.unwrap_or(cx.theme().background))
            .text_color(shell_style.foreground.unwrap_or(cx.theme().foreground))
            .child(
                h_flex()
                    .id("custom-title-bar")
                    .w_full()
                    .h(px(34.))
                    .items_center()
                    .bg(title_bar_style.background.unwrap_or(cx.theme().tab_bar))
                    .text_color(title_bar_style.foreground.unwrap_or(cx.theme().foreground))
                    .text_size(px(title_bar_style.font_size_px.unwrap_or(14.)))
                    .border_b_1()
                    .border_color(title_bar_style.border.unwrap_or(cx.theme().border))
                    .child(
                        h_flex()
                            .h_full()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .child(
                                h_flex()
                                    .size(px(20.))
                                    .rounded(px(badge_style.radius_px.unwrap_or(5.)))
                                    .bg(badge_style.background.unwrap_or(cx.theme().primary))
                                    .text_color(
                                        badge_style
                                            .foreground
                                            .unwrap_or(cx.theme().primary_foreground),
                                    )
                                    .text_size(px(badge_style.font_size_px.unwrap_or(12.)))
                                    .font_semibold()
                                    .justify_center()
                                    .items_center()
                                    .child(project_initial),
                            )
                            .child(div().text_sm().font_semibold().child("Me Editor")),
                    )
                    .child(
                        div()
                            .id("title-bar-drag-region")
                            .flex_1()
                            .h_full()
                            .window_control_area(WindowControlArea::Drag)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| this.titlebar_should_move = true),
                            )
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| this.titlebar_should_move = false),
                            )
                            .on_mouse_move(cx.listener(|this, _, window, _| {
                                if this.titlebar_should_move {
                                    this.titlebar_should_move = false;
                                    window.start_window_move();
                                }
                            })),
                    )
                    .child(self.render_settings_dialog(cx))
                    .child(self.render_window_controls(window, cx)),
            )
            .child(h_flex().flex_1().min_h_0().child(self.dock_area.clone()))
            .child(
                div()
                    .w_full()
                    .bg(status_style.background.unwrap_or(cx.theme().background))
                    .text_color(status_style.foreground.unwrap_or(cx.theme().foreground))
                    .text_size(px(status_style.font_size_px.unwrap_or(12.)))
                    .border_t_1()
                    .border_color(status_style.border.unwrap_or(cx.theme().border))
                    .child(StatusBar::new().left(self.render_panel_buttons(cx)).right(
                        if self.active_path.is_some() {
                            format!("Ln {}, Col {}", cursor.line + 1, cursor.character + 1)
                        } else {
                            "Ln –, Col –".to_string()
                        },
                    )),
            )
            .when_some(self.dialog.clone(), |this, dialog| this.child(dialog))
    }
}

impl EditorApp {
    fn render_window_controls(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let supported = window.window_controls();
        h_flex()
            .id("window-controls")
            .h_full()
            .items_center()
            .flex_shrink_0()
            .when(!cfg!(target_os = "macos") && supported.minimize, |this| {
                this.child(title_bar_window_control(
                    "minimize",
                    IconName::WindowMinimize,
                    WindowControlArea::Min,
                    false,
                    cx,
                ))
            })
            .when(!cfg!(target_os = "macos") && supported.maximize, |this| {
                this.child(title_bar_window_control(
                    if window.is_maximized() {
                        "restore"
                    } else {
                        "maximize"
                    },
                    if window.is_maximized() {
                        IconName::WindowRestore
                    } else {
                        IconName::WindowMaximize
                    },
                    WindowControlArea::Max,
                    false,
                    cx,
                ))
            })
            .when(!cfg!(target_os = "macos"), |this| {
                this.child(title_bar_window_control(
                    "close",
                    IconName::WindowClose,
                    WindowControlArea::Close,
                    true,
                    cx,
                ))
            })
    }

    fn render_settings_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let owner = view.clone();

        div()
            .debug_selector(|| "settings-trigger".into())
            .child(app_dialog::app_dialog(
                Button::new("open-settings")
                    .icon(IconName::Settings)
                    .small()
                    .compact()
                    .ghost()
                    .tooltip("Settings"),
                "Settings",
                move |content, _, cx| {
                    let (dark, explorer_visible, panel_visible) = {
                        let settings = view.read(cx);
                        (
                            settings.dark_theme,
                            settings.explorer_visible,
                            settings.panel_visible,
                        )
                    };
                    let font_size = typography::font_size(cx) / px(1.);
                    let light_view = view.clone();
                    let dark_view = view.clone();
                    let decrease_view = view.clone();
                    let increase_view = view.clone();
                    let explorer_view = view.clone();
                    let panel_view = view.clone();
                    content.child(
                        v_flex()
                            .gap_6()
                            .py_2()
                            .child(
                                v_flex()
                                    .gap_3()
                                    .child(div().text_sm().font_semibold().child("Appearance"))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .child(div().child("Theme"))
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .child(
                                                        Button::new("settings-theme-light")
                                                            .label("Light")
                                                            .when(!dark, |button| button.primary())
                                                            .on_click(move |_, window, cx| {
                                                                light_view.update(
                                                                    cx,
                                                                    |this, cx| {
                                                                        if this.dark_theme {
                                                                            this.toggle_theme(
                                                                                window, cx,
                                                                            );
                                                                        }
                                                                    },
                                                                );
                                                            }),
                                                    )
                                                    .child(
                                                        Button::new("settings-theme-dark")
                                                            .label("Dark")
                                                            .when(dark, |button| button.primary())
                                                            .on_click(move |_, window, cx| {
                                                                dark_view.update(cx, |this, cx| {
                                                                    if !this.dark_theme {
                                                                        this.toggle_theme(
                                                                            window, cx,
                                                                        );
                                                                    }
                                                                });
                                                            }),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .child(div().child("Interface and editor font size"))
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        Button::new("font-size-decrease")
                                                            .label("−")
                                                            .on_click(move |_, window, cx| {
                                                                decrease_view.update(
                                                                    cx,
                                                                    |this, cx| {
                                                                        typography::step_by(cx, -1);
                                                                        theme::sync_font_sizes(cx);
                                                                        this.refresh_dialog(cx);
                                                                        cx.notify();
                                                                        window.refresh();
                                                                    },
                                                                );
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(52.))
                                                            .text_center()
                                                            .child(format!("{font_size:.0} px")),
                                                    )
                                                    .child(
                                                        Button::new("font-size-increase")
                                                            .label("+")
                                                            .on_click(move |_, window, cx| {
                                                                increase_view.update(
                                                                    cx,
                                                                    |this, cx| {
                                                                        typography::step_by(cx, 1);
                                                                        theme::sync_font_sizes(cx);
                                                                        this.refresh_dialog(cx);
                                                                        cx.notify();
                                                                        window.refresh();
                                                                    },
                                                                );
                                                            }),
                                                    ),
                                            ),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_3()
                                    .child(div().text_sm().font_semibold().child("Layout"))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .child(div().child("File explorer"))
                                            .child(
                                                Button::new("settings-explorer-visibility")
                                                    .label(if explorer_visible {
                                                        "Visible"
                                                    } else {
                                                        "Hidden"
                                                    })
                                                    .on_click(move |_, _, cx| {
                                                        explorer_view.update(cx, |this, cx| {
                                                            this.toggle_explorer(cx)
                                                        });
                                                    }),
                                            ),
                                    )
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .child(div().child("Bottom panel"))
                                            .child(
                                                Button::new("settings-bottom-panel-visibility")
                                                    .label(if panel_visible {
                                                        "Visible"
                                                    } else {
                                                        "Hidden"
                                                    })
                                                    .on_click(move |_, _, cx| {
                                                        panel_view.update(cx, |this, cx| {
                                                            this.toggle_bottom_panel(cx)
                                                        });
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .child("Resize panels by dragging the dividers."),
                                    ),
                            ),
                    )
                },
                move |dialog, _, cx| {
                    owner.update(cx, |this, cx| {
                        this.dialog = Some(dialog);
                        cx.notify();
                    });
                },
            ))
    }
}
fn restore_expanded(items: Vec<TreeItem>, expanded: &[String]) -> Vec<TreeItem> {
    items
        .into_iter()
        .map(|item| {
            let children = restore_expanded(item.children, expanded);
            let id = item.id.to_string();
            TreeItem::new(id.clone(), item.label.clone())
                .children(children)
                .expanded(expanded.iter().any(|path| path == &id))
        })
        .collect()
}

fn find_tree_item<'a>(items: &'a [TreeItem], path: &Path) -> Option<&'a TreeItem> {
    items.iter().find_map(|item| {
        if Path::new(item.id.as_str()) == path {
            Some(item)
        } else {
            find_tree_item(&item.children, path)
        }
    })
}

fn tree_items<'a>(root: &Path, files: impl Iterator<Item = &'a Path>) -> Vec<TreeItem> {
    #[derive(Default)]
    struct Node {
        path: PathBuf,
        children: std::collections::BTreeMap<String, Node>,
        file: bool,
    }

    fn into_items(node: Node) -> Vec<TreeItem> {
        let mut children = node
            .children
            .into_iter()
            .map(|(label, child)| {
                let is_folder = !child.file;
                let item = if child.file {
                    TreeItem::new(child.path.to_string_lossy().to_string(), label.clone())
                } else {
                    TreeItem::new(child.path.to_string_lossy().to_string(), label.clone())
                        .children(into_items(child))
                };
                (is_folder, tree_sort_key(&label), label.to_lowercase(), item)
            })
            .collect::<Vec<_>>();
        children.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
        });
        children.into_iter().map(|(_, _, _, item)| item).collect()
    }

    let mut root_node = Node {
        path: root.to_path_buf(),
        ..Default::default()
    };
    for file in files {
        let Ok(relative) = file.strip_prefix(root) else {
            continue;
        };
        let mut node = &mut root_node;
        let mut current = root.to_path_buf();
        let parts = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        for (index, part) in parts.iter().enumerate() {
            current.push(part);
            node = node.children.entry(part.clone()).or_insert_with(|| Node {
                path: current.clone(),
                ..Default::default()
            });
            node.file = index + 1 == parts.len();
        }
    }
    into_items(root_node)
}

fn tree_sort_key(name: &str) -> String {
    name.chars().fold(String::new(), |mut key, character| {
        if let Some(pinyin) = character.to_pinyin() {
            key.push_str(pinyin.first_letter());
        } else {
            key.extend(character.to_lowercase());
        }
        key
    })
}

fn language_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
    {
        "rs" => "rust",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "vue" => "vue",
        "html" | "htm" => "html",
        "css" => "css",
        "json" => "json",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        "yaml" | "yml" => "yaml",
        _ => "text",
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
    #[cfg(target_os = "windows")]
    let _timer_resolution = WindowsTimerResolution::enable_for_window_drag();

    let (workspace, initial_file) = resolve_startup_target()?;
    let startup_state = SessionState::load(workspace.root());
    gpui_kit::application()
        .with_assets(AppAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(false), cx);
            cx.activate(true);
            cx.bind_keys([
                KeyBinding::new("ctrl-s", SaveDocument, Some("EditorShell")),
                KeyBinding::new("ctrl-shift-r", RefreshWorkspace, Some("EditorShell")),
                KeyBinding::new("ctrl-j", ToggleBottomPanel, Some("EditorShell")),
                KeyBinding::new("ctrl-alt-t", ToggleTheme, Some("EditorShell")),
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
            window_options.window_bounds = Some(WindowBounds::Windowed(bounds));
            // GPUI's default throttles animations in inactive windows to 30 FPS.
            // Leave frame scheduling uncapped; active frames follow the display refresh rate.
            window_options.inactive_frame_interval = None;
            cx.open_window(window_options, move |window, cx| {
                let view = cx.new(|cx| EditorApp::new(workspace, initial_file, window, cx));
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            })
            .expect("failed to open Me Editor window");
        });
    Ok(())
}

#[cfg(test)]
mod settings_dialog_tests {
    use crate::theme::{apply_theme, builtin_theme};
    use crate::{EditorApp, typography};
    use editor_core::Workspace;
    use gpui_kit::{AppContext as _, TestAppContext, component::Root, gpui, px, size};

    /// Clicking the real title-bar button must paint a dialog in the app window.
    #[gpui::test]
    fn settings_button_paints_dialog(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(false), cx);
            cx.set_reduce_motion(true);
        });
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
            Root::new(view, window, cx)
        });
        cx.simulate_resize(size(px(1000.), px(800.)));
        cx.update(|window, cx| window.draw(cx).clear(cx));

        let button = cx
            .debug_bounds("settings-trigger")
            .expect("settings button should be visible in the title bar");
        cx.simulate_click(button.center(), Default::default());
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));

        assert!(
            cx.debug_bounds("dialog-0").is_some(),
            "clicking Settings must paint a dialog layer"
        );
    }
}
