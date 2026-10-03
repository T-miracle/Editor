//! Collections reuse the editor's native tab/menu behavior at arbitrary composed-tree positions.
use super::controls::CollectionModel;
use super::*;

impl PluginView {
    /// Reconcile stable widget identities; drawing-only revisions never recreate rename inputs or menus.
    pub(super) fn sync_widgets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut models = Vec::new();
        self.document.root.visit(&mut |node| {
            if let Kind::SideTabs(tabs) = &node.kind {
                models.push(tabs.clone());
            }
        });
        if let Some(dialog) = &self.document.dialog {
            dialog.content.visit(&mut |node| {
                if let Kind::SideTabs(tabs) = &node.kind {
                    models.push(tabs.clone());
                }
            });
        }
        self.collections
            .retain(|id, _| models.iter().any(|tabs| &tabs.id == id));
        for tabs in models {
            let id = tabs.id.clone();
            if !self.collections.contains_key(&id) {
                let view = self.widget(cx);
                self.collections.insert(id.clone(), view);
            }
            let model = CollectionModel {
                revision: self.document.revision,
                sidebar: Some(tabs),
                menu: None,
            };
            self.collections[&id].update(cx, |view, cx| {
                view.update(model, self.environment.clone(), self.origin, window, cx)
            });
        }
        // The ordinary dialog has precedence over a popup; neither borrows another panel's native state.
        if self.document.dialog.is_none() && self.document.menu.is_some() {
            if self.popup.is_none() {
                self.popup = Some(self.widget(cx));
            }
            let model = CollectionModel {
                revision: self.document.revision,
                menu: self.document.menu.clone(),
                sidebar: None,
            };
            self.popup.as_ref().unwrap().update(cx, |view, cx| {
                view.update(model, self.environment.clone(), self.origin, window, cx)
            });
        } else if let Some(popup) = self.popup.take() {
            // Let the shared menu adapter return focus before its keyed entity is retired.
            popup.update(cx, |view, cx| {
                view.update(
                    CollectionModel::default(),
                    self.environment.clone(),
                    self.origin,
                    window,
                    cx,
                )
            });
        }
    }

    /// Every callback carries the widget's published revision through the same document gate.
    fn widget(&self, cx: &mut Context<Self>) -> Entity<controls::CollectionView> {
        let owner = cx.entity().downgrade();
        let focus = self
            .canvases
            .values()
            .find(|canvas| canvas.read(cx).drawing.focusable)
            .map(|canvas| canvas.read(cx).focus_handle())
            .unwrap_or_else(|| self.dialog_focus.clone());
        cx.new(|_| {
            controls::CollectionView::new(self.plugin.clone(), focus, move |event, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.emit_version(&event.node, event.revision, event.action, cx);
                });
            })
        })
    }
}
