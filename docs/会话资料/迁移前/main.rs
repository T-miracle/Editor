mod assets;
mod icons;
mod session_state;
mod theme;
mod typography;

use editor_core::{CommandId, CommandRegistry, DocumentSession, Workspace};
use gpui_kit::{
    InteractiveElement as _,
    component::{
        ActiveTheme, Icon, IconName, Root, Sizable, StyledExt, TitleBar,
        button::{Button, ButtonVariants as _},
        h_flex,
        input::{Editor, EditorState, InputEvent, TabSize},
        list::ListItem,
        resizable::{h_resizable, resizable_panel, v_resizable},
        status_bar::StatusBar,
        tree::{TreeEvent, TreeItem, TreeState, tree},
        v_flex,
    },
    prelude::FluentBuilder,
    *,
};
use platform_windows::{LocalHistory, NativeFileStore};
use std::path::{Path, PathBuf};

use assets::AppAssets;
use icons::file_icon;
use pinyin::ToPinyin;
use session_state::SessionState;
use theme::apply_jetbrains_theme;

const EXPLORER_INITIAL_WIDTH: f32 = 280.;
const EXPLORER_MIN_WIDTH: f32 = 220.;
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

struct EditorApp {
    workspace: Workspace,
    file_store: NativeFileStore,
    history: Option<LocalHistory>,
    commands: CommandRegistry,
    editor: Entity<EditorState>,
    tree_state: Entity<TreeState>,
    tabs_scroll: ScrollHandle,
    tabs: Vec<OpenTab>,
    active_path: Option<PathBuf>,
    status: String,
    panel_visible: bool,
    soft_wrap: bool,
    dark_theme: bool,
    session_state: SessionState,
    _tree_subscription: Subscription,
    _bounds_subscription: Option<Subscription>,
}

struct OpenTab {
    session: DocumentSession,
    editor: Entity<EditorState>,
    _subscription: Subscription,
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
        let mut this = Self {
            workspace,
            file_store: NativeFileStore,
            history: LocalHistory::for_current_user().ok(),
            commands: CommandRegistry::editor_defaults(),
            editor,
            tree_state,
            tabs_scroll: ScrollHandle::new(),
            tabs: Vec::new(),
            active_path: None,
            status: "Ready".into(),
            panel_visible: session_state.panel_visible,
            soft_wrap: false,
            dark_theme: false,
            session_state,
            _tree_subscription: tree_subscription,
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

    fn toggle_bottom_panel(&mut self, cx: &mut Context<Self>) {
        self.panel_visible = !self.panel_visible;
        self.persist_session();
        cx.notify();
    }

    fn toggle_soft_wrap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.soft_wrap = !self.soft_wrap;
        self.editor.update(cx, |editor, cx| {
            editor.set_soft_wrap(self.soft_wrap, window, cx);
        });
        cx.notify();
    }

    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dark_theme = !self.dark_theme;
        apply_jetbrains_theme(self.dark_theme, cx);
        self.status = if self.dark_theme {
            "JetBrains 2023 Dark"
        } else {
            "JetBrains 2023 Light"
        }
        .into();
        window.refresh();
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

    fn render_toolbar(&self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let save_title = self
            .commands
            .get(CommandId::SaveDocument)
            .map_or("Save", |command| command.title);
        h_flex()
            .w_full()
            .h(px(42.))
            .px_2()
            .gap_1()
            .bg(cx.theme().sidebar)
            .child(
                Button::new("save")
                    .label(save_title)
                    .small()
                    .primary()
                    .on_click(cx.listener(|this, _, _, cx| this.save_current(cx))),
            )
            .child(
                Button::new("refresh")
                    .label("Refresh")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.refresh_files(cx))),
            )
            .child(
                Button::new("soft-wrap")
                    .label(if self.soft_wrap {
                        "Wrap: on"
                    } else {
                        "Wrap: off"
                    })
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_soft_wrap(window, cx))),
            )
            .child(
                Button::new("panel")
                    .label("Panel")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_bottom_panel(cx))),
            )
            .child(
                Button::new("theme")
                    .label(if self.dark_theme { "Light" } else { "Dark" })
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_theme(window, cx))),
            )
            .child(
                h_flex()
                    .ml_auto()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.workspace.root().display().to_string()),
            )
    }

    fn render_file_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let dark_theme = self.dark_theme;
        tree(&self.tree_state, move |index, entry, _, _window, cx| {
            view.update(cx, |_, cx| {
                let item = entry.item();
                let is_folder = item.is_folder();
                let icon = file_icon(Path::new(item.id.as_str()), is_folder, dark_theme);
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
                    .py_0p5()
                    .px_1()
                    .pl(px(14.) * entry.depth() + px(8.))
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
        })
        .p_1()
        .text_size(cx.theme().mono_font_size)
        .font_family(cx.theme().mono_font_family.clone())
        .size_full()
        .bg(cx.theme().sidebar)
        .text_color(cx.theme().sidebar_foreground)
    }

    fn render_tabs(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = self.tabs.iter().map(|tab| {
            let path = tab.session.path().to_path_buf();
            let is_active = self.active_path.as_ref() == Some(&path);
            let is_dirty = tab.session.is_dirty();
            let name = tab.session.file_name().unwrap_or("Untitled").to_string();
            let icon = file_icon(&path, false, self.dark_theme);
            let activate_path = path.clone();
            let close_path = path.clone();
            let middle_close_path = path.clone();
            h_flex()
                .id(format!("editor-tab:{}", path.to_string_lossy()))
                .h_full()
                .w(px(190.))
                .flex_shrink_0()
                .gap_2()
                .px_2()
                .border_r_1()
                .border_color(cx.theme().border)
                .bg(if is_active {
                    cx.theme().background
                } else {
                    cx.theme().tab_bar
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
                        .rounded(px(3.))
                        .p_1()
                        .hover(|style| style.bg(cx.theme().list_hover))
                        .child(Icon::new(IconName::Close).xsmall())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_tab(close_path.clone(), window, cx);
                        })),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_file(activate_path.clone(), window, cx);
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
            .h(px(36.))
            .bg(cx.theme().tab_bar)
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
            .when(max_scroll > px(0.), |this| {
                this.child(
                    div()
                        .absolute()
                        .left(thumb_left)
                        .bottom_0()
                        .w(thumb_width)
                        .h(px(2.))
                        .bg(cx.theme().primary),
                )
            })
    }

    fn render_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .size_full()
            .min_h_0()
            .p_3()
            .gap_2()
            .bg(cx.theme().muted)
            .child(h_flex().text_sm().child("OUTPUT"))
            .child(
                h_flex()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.status.clone()),
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

        v_flex()
            .id("editor-shell")
            .key_context("EditorShell")
            .on_action(cx.listener(Self::on_save_action))
            .on_action(cx.listener(Self::on_refresh_action))
            .on_action(cx.listener(Self::on_toggle_panel_action))
            .on_action(cx.listener(Self::on_toggle_theme_action))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .h_full()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .child(
                            h_flex()
                                .size(px(20.))
                                .rounded(px(5.))
                                .bg(cx.theme().primary)
                                .text_color(cx.theme().primary_foreground)
                                .text_xs()
                                .font_semibold()
                                .justify_center()
                                .items_center()
                                .child(project_initial),
                        )
                        .child(div().text_sm().font_semibold().child("Me Editor")),
                ),
            )
            .child(self.render_toolbar(window, cx))
            .child(
                div().flex_1().child(
                    h_resizable("main-split")
                        .on_resize({
                            let view = cx.entity();
                            move |state, _, cx| {
                                let width =
                                    state.read(cx).sizes().first().map(|size| *size / px(1.));
                                if let Some(width) = width {
                                    view.update(cx, |this, _| {
                                        this.session_state.explorer_width = width;
                                        this.persist_session();
                                    });
                                }
                            }
                        })
                        .child(
                            resizable_panel()
                                .size(px(self.session_state.explorer_width))
                                .size_range(px(EXPLORER_MIN_WIDTH)..px(520.))
                                .child(self.render_file_tree(cx)),
                        )
                        .child(
                            resizable_panel().child(
                                v_flex()
                                    .size_full()
                                    .child(
                                        div().flex_1().child(
                                            v_resizable("editor-output-split")
                                                .on_resize({
                                                    let view = cx.entity();
                                                    move |state, _, cx| {
                                                        if let Some(size) =
                                                            state.read(cx).sizes().get(1)
                                                        {
                                                            let height = *size / px(1.);
                                                            view.update(cx, |this, _| {
                                                                this.session_state.output_height =
                                                                    height;
                                                                this.persist_session();
                                                            });
                                                        }
                                                    }
                                                })
                                                .child(
                                                    resizable_panel().child(
                                                        v_flex()
                                                            .size_full()
                                                            .child(self.render_tabs(window, cx))
                                                            .child(
                                                                Editor::new(&self.editor)
                                                                    .bordered(false)
                                                                    .p_0()
                                                                    .flex_1()
                                                                    .min_h_0()
                                                                    .font_family(
                                                                        cx.theme()
                                                                            .mono_font_family
                                                                            .clone(),
                                                                    )
                                                                    .text_size(
                                                                        cx.theme().mono_font_size,
                                                                    )
                                                                    .into_any_element(),
                                                            ),
                                                    ),
                                                )
                                                .child(
                                                    resizable_panel()
                                                        .size(px(self.session_state.output_height))
                                                        .size_range(px(100.)..px(420.))
                                                        .visible(self.panel_visible)
                                                        .child(self.render_panel(cx)),
                                                ),
                                        ),
                                    )
                                    .child(
                                        StatusBar::new().left(format!("{}", self.status)).right(
                                            format!(
                                                "Ln {}, Col {}",
                                                cursor.line + 1,
                                                cursor.character + 1
                                            ),
                                        ),
                                    ),
                            ),
                        ),
                ),
            )
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
            apply_jetbrains_theme(false, cx);
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
