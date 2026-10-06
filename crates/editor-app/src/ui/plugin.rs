//! Native plugin view lifecycle. GPUI entities stay here; guests receive typed events only.
mod atlas;
pub(crate) mod bitmap;
mod canvas;
mod code;
pub(crate) mod controls;
pub(crate) mod images;
mod layout;
#[cfg(test)]
mod link_tests;
mod links;
mod render;
mod svg;
#[cfg(test)]
mod tests;
mod textareas;
mod theme;
mod viewport;
#[cfg(test)]
mod viewport_tests;
mod widgets;

use gpui_base::input::{InputEvent, InputState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, IntoElement, Render, ScrollHandle,
    Subscription, Window,
};
use plugin_runtime::plugin_protocol::{
    Environment,
    ui::{Action, Document, Kind, UiEvent},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

type EventSink = Rc<dyn Fn(UiEvent, &mut App)>;

struct NativeInput {
    state: Entity<InputState>,
    value_revision: u64,
    last_value: String,
    _subscription: Subscription,
}

pub(crate) struct PluginView {
    plugin: String,
    document: Document,
    environment: Environment,
    sink: EventSink,
    inputs: BTreeMap<String, NativeInput>,
    textareas: BTreeMap<String, textareas::NativeTextarea>,
    scrolls: BTreeMap<String, ScrollHandle>,
    /// Read-only source block ownership and one revision-bound reveal; native scroll remains in Base.
    scene_layout: layout::SceneLayout,
    /// Coalesced source-block geometry and one locate receipt; Base owns the actual scroll offset.
    viewport: viewport::ViewportState,
    /// Base resolves the release target; the local adapter retains the real native press owner.
    link_press: Option<links::LinkPress>,
    /// Per-target Base focus survives source refresh; subscriptions reveal only the focused link cue.
    link_focus: BTreeMap<String, links::LinkFocus>,
    /// Versioned readonly capture cache and a cancellable, bounded native background batch.
    code_highlights: code::CodeHighlights,
    canvases: BTreeMap<String, Entity<canvas::CanvasView>>,
    /// Collection widgets retain native rename/drag state independently of canvas redraws.
    collections: BTreeMap<String, Entity<controls::CollectionView>>,
    popup: Option<Entity<controls::CollectionView>>,
    origin: gpui_kit::Point<gpui_kit::Pixels>,
    dialog_focus: FocusHandle,
    /// The surface identifies descendant control focus without adding a container tab stop.
    view_focus: FocusHandle,
    /// Control focus survives a source refresh; gesture identity is separately bound to its scene.
    checkbox_focus: BTreeMap<String, FocusHandle>,
    previous_focus: Option<FocusHandle>,
    dismissed_dialog: Option<String>,
    /// Editor-local toolbar projections fit their wrapped content instead of consuming the full pane.
    content_sized: bool,
    /// Decoded resources are accepted only for this tree's source version and URI.
    photos: BTreeMap<String, std::sync::Arc<images::Photo>>,
    /// GPU textures outlive pixel Arcs unless the last native projection explicitly evicts them.
    image_leases: atlas::ImageLeases,
}

impl PluginView {
    /// Image decoding belongs to the worker; native children only borrow the matching immutable raster.
    pub(crate) fn update_images(
        &mut self,
        panel_key: &str,
        images: &images::SceneImages,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut photos = BTreeMap::new();
        let mut visit = |node: &plugin_runtime::plugin_protocol::ui::Node| {
            if let Kind::Image { source, .. } = &node.kind
                && let Some(photo) = images.photos.get(&format!("{panel_key}/image/{}", node.id))
                && self.document.source.as_ref() == Some(&photo.resource.source)
                && source == &photo.resource.uri
            {
                photos.insert(node.id.clone(), photo.clone());
            }
        };
        self.document.root.visit(&mut visit);
        if let Some(toolbar) = &self.document.editor_toolbar {
            toolbar.visit(&mut visit);
        }
        if let Some(dialog) = &self.document.dialog {
            dialog.content.visit(&mut visit);
        }
        let changed = photos.len() != self.photos.len()
            || photos.iter().any(|(id, image)| {
                self.photos
                    .get(id)
                    .is_none_or(|old| !std::sync::Arc::ptr_eq(old, image))
            });
        self.photos = photos;
        if changed {
            self.link_press = None;
            cx.notify();
        }
        for (id, canvas) in &self.canvases {
            let images = images.get(&format!("{panel_key}/canvas/{id}")).cloned();
            canvas.update(cx, |view, cx| {
                if match (&view.images, &images) {
                    (Some(old), Some(new)) => !std::sync::Arc::ptr_eq(old, new),
                    (None, None) => false,
                    _ => true,
                } {
                    view.images = images;
                    cx.notify();
                }
            });
        }
        self.sync_image_leases(window, cx);
    }

    /// Reconcile all decoded native pixels once per projection, including shared dialog/canvas images.
    fn sync_image_leases(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut pixels = self
            .photos
            .values()
            .filter_map(|photo| {
                photo
                    .decoded
                    .as_ref()
                    .ok()
                    .and_then(Option::as_ref)
                    .map(|bitmap| bitmap.image.clone())
            })
            .collect::<Vec<_>>();
        for canvas in self.canvases.values() {
            if let Some(images) = &canvas.read(cx).images {
                pixels.extend(images.iter().flatten().map(|image| image.image.clone()));
            }
        }
        for image in self.image_leases.update(pixels, cx) {
            // GPUI removes the borrowed window from App.windows during rendering and updates.
            cx.drop_image(image, Some(window));
        }
    }
    pub(crate) fn new(
        plugin: String,
        document: Document,
        environment: Environment,
        sink: impl Fn(UiEvent, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // RenderImage Arcs do not evict GPUI's atlas. Entity retirement therefore owns
        // an explicit cleanup, including the current window while App temporarily lends it out.
        let image_window = window.window_handle();
        cx.on_release(move |this, cx| {
            let pixels = this.image_leases.clear(cx);
            if image_window
                .update(cx, |_, window, cx| {
                    for image in &pixels {
                        cx.drop_image(image.clone(), Some(window));
                    }
                })
                .is_err()
            {
                // A closed owner window is already gone; shared application atlases still need retirement.
                for image in pixels {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        let mut this = Self {
            plugin,
            document,
            environment,
            sink: Rc::new(sink),
            inputs: BTreeMap::new(),
            textareas: BTreeMap::new(),
            scrolls: BTreeMap::new(),
            scene_layout: Default::default(),
            viewport: Default::default(),
            link_press: None,
            link_focus: BTreeMap::new(),
            code_highlights: Default::default(),
            canvases: BTreeMap::new(),
            collections: BTreeMap::new(),
            popup: None,
            origin: Default::default(),
            dialog_focus: cx.focus_handle(),
            view_focus: cx.focus_handle().tab_stop(false),
            checkbox_focus: BTreeMap::new(),
            previous_focus: None,
            dismissed_dialog: None,
            content_sized: false,
            photos: BTreeMap::new(),
            image_leases: Default::default(),
        };
        this.sync_native(window, cx);
        if this.document.dialog.is_some() {
            this.focus_dialog(window, cx);
        }
        this
    }

    /// Retain the same native controls and event gate while letting a surrounding layout own height.
    pub(crate) fn content_sized(mut self) -> Self {
        self.content_sized = true;
        self
    }

    /// Check document identity, excluding revision, before retaining hidden native state during refresh.
    pub(crate) fn same_source_document(
        &self,
        version: &plugin_runtime::plugin_protocol::api::DocumentVersion,
    ) -> bool {
        self.document
            .source
            .as_ref()
            .is_some_and(|source| source.id == version.id && source.path == version.path)
    }

    /// A preview edit preserves its originating native control; toolbar edits still focus source.
    pub(crate) fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.view_focus.contains_focused(window, cx)
    }

    /// Reconcile keyed native state; ordinary guest renders must not recreate focused inputs.
    pub(crate) fn update_document(
        &mut self,
        document: Document,
        environment: Environment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document == document && self.environment == environment {
            return;
        }
        if self.document != document {
            self.invalidate_code_highlighting(cx);
        }
        let old_dialog = self.document.dialog.as_ref().map(|d| d.id.clone());
        // A new scene/theme cannot adopt an earlier pointer press, even when node IDs are reused.
        self.link_press = None;
        let next_dialog = document.dialog.as_ref().map(|d| d.id.clone());
        let menu_changed = self.document.menu.as_ref().map(|menu| &menu.id)
            != document.menu.as_ref().map(|menu| &menu.id);
        self.document = document;
        self.environment = environment;
        // Each popup opening owns a fresh dismiss acknowledgement, even when it reuses an ID.
        if menu_changed {
            self.dismissed_dialog = None;
        }
        if old_dialog != next_dialog {
            self.dismissed_dialog = None;
            if next_dialog.is_some() {
                self.focus_dialog(window, cx);
            } else if let Some(focus) = self.previous_focus.take() {
                focus.focus(window, cx);
            }
        }
        self.sync_native(window, cx);
        // A removed image or source replacement can retire this tree without another worker update.
        let mut keep = BTreeSet::new();
        let mut visit = |node: &plugin_runtime::plugin_protocol::ui::Node| {
            if let Kind::Image { source, .. } = &node.kind
                && let Some(photo) = self.photos.get(&node.id)
                && self.document.source.as_ref() == Some(&photo.resource.source)
                && source == &photo.resource.uri
            {
                keep.insert(node.id.clone());
            }
        };
        self.document.root.visit(&mut visit);
        if let Some(toolbar) = &self.document.editor_toolbar {
            toolbar.visit(&mut visit);
        }
        if let Some(dialog) = &self.document.dialog {
            dialog.content.visit(&mut visit);
        }
        self.photos.retain(|id, _| keep.contains(id));
        self.sync_image_leases(window, cx);
        // Closing a source-only overlay can retire this view before it is rendered again.
        // Reconcile popup removal here as well so its previous native focus is returned first.
        self.sync_widgets(window, cx);
        cx.notify();
    }

    fn focus_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.previous_focus.is_none() {
            self.previous_focus = window.focused(cx);
        }
        self.dialog_focus.focus(window, cx);
    }

    fn sync_native(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.scene_layout.reset(&self.document);
        self.viewport.reset_scene();
        self.sync_link_focus(window, cx);
        let mut nodes = vec![];
        self.document
            .root
            .visit(&mut |node| nodes.push(node.clone()));
        if let Some(dialog) = &self.document.dialog {
            dialog.content.visit(&mut |node| nodes.push(node.clone()));
        }
        let input_ids: BTreeSet<_> = nodes
            .iter()
            .filter(|n| matches!(n.kind, Kind::Input(_)))
            .map(|n| n.id.clone())
            .collect();
        self.inputs.retain(|id, _| input_ids.contains(id));
        self.sync_textareas(&nodes, window, cx);
        let scroll_ids: BTreeSet<_> = nodes
            .iter()
            .filter(|n| matches!(n.kind, Kind::Scroll { .. }))
            .map(|n| n.id.clone())
            .collect();
        self.scrolls.retain(|id, _| scroll_ids.contains(id));
        let canvas_ids: BTreeSet<_> = nodes
            .iter()
            .filter(|node| matches!(node.kind, Kind::Canvas(_)))
            .map(|node| node.id.clone())
            .collect();
        self.canvases.retain(|id, _| canvas_ids.contains(id));
        let checkbox_ids: BTreeSet<_> = nodes
            .iter()
            .filter(|node| matches!(node.kind, Kind::Checkbox { .. }))
            .map(|node| node.id.clone())
            .collect();
        self.checkbox_focus
            .retain(|id, _| checkbox_ids.contains(id));
        for id in checkbox_ids {
            self.checkbox_focus
                .entry(id)
                .or_insert_with(|| cx.focus_handle());
        }
        for node in nodes {
            if let Kind::Canvas(drawing) = &node.kind {
                if !self.canvases.contains_key(&node.id) {
                    let owner = cx.entity().downgrade();
                    let id = node.id.clone();
                    let view = cx.new(|cx| {
                        let canvas = cx.entity().downgrade();
                        canvas::CanvasView::new(
                            drawing.clone(),
                            move |event, revision, cx| {
                                let owner = owner.clone();
                                let id = id.clone();
                                let canvas = canvas.clone();
                                let measurement = matches!(
                                    event,
                                    plugin_runtime::plugin_protocol::ui::CanvasEvent::Resize { .. }
                                );
                                cx.defer(move |cx| {
                                    let accepted = owner.update(cx, |this, cx| {
                                        this.emit_version(&id, revision, Action::Canvas(event), cx)
                                    });
                                    if measurement && !matches!(accepted, Ok(true)) {
                                        let _ = canvas.update(cx, |view, cx| {
                                            view.invalidate_measurement();
                                            cx.notify();
                                        });
                                    }
                                });
                            },
                            window,
                            cx,
                        )
                    });
                    self.canvases.insert(node.id.clone(), view);
                }
                let active = self.document.active_node(&node.id).is_some();
                let font = drawing.font.clone().over(self.environment.font_style(
                    &self.plugin,
                    node.theme_role(),
                    drawing.grid,
                ));
                self.canvases[&node.id].update(cx, |view, cx| {
                    view.drawing = drawing.clone();
                    view.font = font;
                    if active && !view.enabled {
                        view.invalidate_measurement();
                    }
                    view.enabled = active;
                    view.foreground = self.environment.foreground;
                    if !active {
                        view.deactivate();
                    } else if !drawing.focusable {
                        view.cancel_composition();
                    }
                    view.revision = self.document.revision;
                    cx.notify();
                });
            }
            if let Kind::Scroll { .. } = &node.kind {
                self.scrolls.entry(node.id.clone()).or_default();
            }
            let Kind::Input(input) = &node.kind else {
                continue;
            };
            if !self.inputs.contains_key(&node.id) {
                let state = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(input.value.clone())
                        .placeholder(input.placeholder.clone())
                });
                let id = node.id.clone();
                let subscription = cx.subscribe_in(
                    &state,
                    window,
                    move |this, state, event: &InputEvent, _, cx| {
                        let value = state.read(cx).value().to_string();
                        let action = match event {
                            InputEvent::Change => {
                                let Some(entry) = this.inputs.get_mut(&id) else {
                                    return;
                                };
                                if entry.last_value == value {
                                    return;
                                }
                                entry.last_value = value.clone();
                                Action::Change(value)
                            }
                            InputEvent::PressEnter { .. } => Action::Submit(value),
                            _ => return,
                        };
                        this.emit(&id, action, cx);
                    },
                );
                self.inputs.insert(
                    node.id.clone(),
                    NativeInput {
                        state,
                        value_revision: input.value_revision,
                        last_value: input.value.clone(),
                        _subscription: subscription,
                    },
                );
            }
            let disabled = self.document.active_node(&node.id).is_none();
            let entry = self.inputs.get_mut(&node.id).unwrap();
            let replace = entry.value_revision != input.value_revision;
            if replace {
                entry.last_value = input.value.clone();
                entry.value_revision = input.value_revision;
            }
            entry.state.update(cx, |state, cx| {
                state.set_disabled(disabled, cx);
                state.set_placeholder(input.placeholder.clone(), window, cx);
                if replace {
                    state.set_value(input.value.clone(), window, cx);
                }
            });
        }
    }

    /// Old elements cannot activate removed/disabled controls or controls behind a modal.
    fn emit(&mut self, id: &str, action: Action, cx: &mut Context<Self>) {
        self.emit_version(id, self.document.revision, action, cx);
    }

    /// Deferred canvas measurements retain the revision at emission instead of borrowing a newer tree.
    fn emit_version(
        &mut self,
        id: &str,
        revision: u64,
        action: Action,
        cx: &mut Context<Self>,
    ) -> bool {
        let event = UiEvent {
            revision,
            node: id.into(),
            action,
        };
        if self.document.validate_event(&event).is_err()
            || (matches!(event.action, Action::Dismiss)
                && self.dismissed_dialog.as_deref() == Some(id))
        {
            return false;
        }
        if matches!(event.action, Action::Dismiss) {
            self.dismissed_dialog = Some(id.into());
            cx.notify();
        }
        (self.sink)(event, cx);
        true
    }
}

impl Render for PluginView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_document(window, cx)
    }
}
