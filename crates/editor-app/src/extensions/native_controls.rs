//! Panel title commands use the same native popup as guest-supplied menus.
use super::*;
use crate::ui::controls::menu::{MenuStyle, PopupMenu};
impl ExtensionPanel {
    /// Release native focus/IME targets with the hidden surface, while the guest keeps its own view state.
    pub(super) fn hide(&mut self) {
        self.visible.set(false);
        self.native_ui = None;
        self.command_popup = None;
    }
    pub(super) fn command_popup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<PopupMenu>> {
        if !self.commands_open {
            self.command_popup = None;
            return None;
        }
        let items: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.enabled && Some(&entry.manifest.id) == self.active.as_ref())
            .flat_map(|entry| {
                entry
                    .manifest
                    .commands
                    .iter()
                    .filter(|command| command.menu)
            })
            .map(|command| protocol::ui::MenuItem {
                id: command.id.clone(),
                label: command.title.clone(),
                disabled: false,
                separator_before: false,
            })
            .collect();
        let style = MenuStyle::current(cx);
        if let Some(popup) = &self.command_popup {
            popup.update(cx, |popup, cx| {
                popup.style = style;
                popup.items = items;
                cx.notify();
            });
        } else {
            let owner = cx.entity().downgrade();
            // Retained popups belong to the instance which supplied their items.
            let epoch = self.instance_epoch;
            let position = point(
                (self.bounds.right() - px(230.)).max(px(8.)),
                self.bounds.top().max(px(8.)),
            );
            self.command_popup = Some(cx.new(|cx| {
                PopupMenu::new(
                    items,
                    style,
                    position,
                    move |action, _, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            if this.instance_epoch != epoch {
                                return;
                            }
                            if let protocol::ui::Action::Select(id) = action {
                                this.command(id);
                            }
                            this.commands_open = false;
                            cx.notify();
                        });
                    },
                    window,
                    cx,
                )
            }));
        }
        self.command_popup.clone()
    }
}
