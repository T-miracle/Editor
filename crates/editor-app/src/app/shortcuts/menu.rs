//! Application-menu entry preserves the active panel before popup focus is transferred.

use super::ShortcutOrigin;
use crate::ui::controls::menu::{MenuStyle, PopupMenu};
use crate::*;
use plugin_runtime::plugin_protocol::ui::{Action as MenuAction, MenuItem};

/// Retain one menu and its original dispatch context independently of shortcut bindings.
#[derive(Default)]
pub(crate) struct MenuState {
    popup: Option<Entity<PopupMenu>>,
    origin: Option<ShortcutOrigin>,
    pointer_origin: Option<ShortcutOrigin>,
    anchor: Rc<Cell<Point<Pixels>>>,
    generation: u64,
}

impl MenuState {
    /// Remove a superseded menu and invalidate deferred callbacks without moving keyboard focus.
    pub(crate) fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.popup = None;
        self.origin = None;
        self.pointer_origin = None;
    }
}

impl EditorApp {
    /// Draw the persistent application-menu trigger using the local Base-backed button.
    ///
    /// Capture pointer context before Base focuses its trigger. Keyboard activation captures
    /// the currently focused context rather than reusing an abandoned pointer gesture.
    pub(crate) fn render_shortcuts_menu_trigger(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let anchor = self.shortcut_menu.anchor.clone();
        div()
            .relative()
            .capture_any_mouse_down(cx.listener(|app, event: &MouseDownEvent, window, cx| {
                if event.button == MouseButton::Left {
                    app.shortcut_menu.pointer_origin =
                        Some(ShortcutOrigin::capture(app, window, cx));
                }
            }))
            .child(
                Button::new("shortcuts-menu-trigger")
                    .debug_selector(|| "shortcuts-menu-trigger".into())
                    .label(t!("shortcut_menu.application").to_string())
                    .accessibility_label(t!("shortcut_menu.application").to_string())
                    .expanded(self.shortcut_menu.popup.is_some())
                    .small()
                    .ghost()
                    .on_click(cx.listener(|app, event: &ClickEvent, window, cx| {
                        let origin = if matches!(event, ClickEvent::Mouse(_)) {
                            app.shortcut_menu.pointer_origin.take()
                        } else {
                            app.shortcut_menu.pointer_origin = None;
                            None
                        }
                        .unwrap_or_else(|| ShortcutOrigin::capture(app, window, cx));
                        let anchor = app.shortcut_menu.anchor.get();
                        app.open_shortcuts_menu_from(origin, anchor, window, cx);
                    })),
            )
            // Anchor to measured geometry so window scaling or label translation cannot move
            // the popup over its trigger. The canvas adds no independent hit target.
            .child(
                gpui_kit::canvas(
                    move |bounds, _, _| anchor.set(bounds.bottom_left()),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }

    /// Return the retained popup for the shell's overlay layer; rendering never opens a menu.
    pub(crate) fn render_shortcuts_menu(&self) -> Option<Entity<PopupMenu>> {
        self.shortcut_menu.popup.clone()
    }

    /// Open a local native menu with the original panel retained across both popup transitions.
    fn open_shortcuts_menu_from(
        &mut self,
        origin: ShortcutOrigin,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.shortcut_menu.clear();
        // PopupMenu restores its previous focus before delivering a selection or dismissal.
        // Seed that previous handle with the actual panel, not the pointer-focused trigger.
        origin.focus.focus(window, cx);
        self.shortcut_menu.origin = Some(origin);
        let generation = self.shortcut_menu.generation;
        let owner = cx.entity().downgrade();
        let items = vec![MenuItem {
            id: "item-shortcuts".into(),
            label: t!("shortcuts.title").to_string(),
            disabled: false,
            separator_before: false,
        }];
        self.shortcut_menu.popup = Some(cx.new(|cx| {
            PopupMenu::new(
                items,
                MenuStyle::current(cx),
                position,
                move |action, window, cx| {
                    let owner = owner.clone();
                    let open = matches!(action, MenuAction::Select(id) if id == "item-shortcuts");
                    // Removing an entity from inside its own menu event can cause a reentrant
                    // borrow; finish the menu's focus restoration before changing shell state.
                    window.defer(cx, move |window, cx| {
                        let _ = owner.update(cx, |app, cx| {
                            if app.shortcut_menu.generation != generation {
                                return;
                            }
                            let origin = app.shortcut_menu.origin.take();
                            app.shortcut_menu.clear();
                            if open && let Some(origin) = origin {
                                app.open_shortcuts_from(origin, window, cx);
                            }
                            cx.notify();
                        });
                    });
                },
                window,
                cx,
            )
            .width(230.)
            .below_anchor()
        }));
        cx.notify();
    }
}
