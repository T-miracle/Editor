//! Native source positions are sampled after layout and routed only to the authorized active split.
use super::*;
use crate::editor::viewport::{self, Anchor, Locator, Step};
use gpui_kit::{AnyElement, Point, canvas};

/// Retain geometry and bounded location work, never a document, caret, or another scroll handle.
#[derive(Default)]
pub(crate) struct SourceTracking {
    last: Option<Measurement>,
    pending: Option<Pending>,
    /// Retain a completed origin until stable top-caret geometry becomes available.
    settled_origin: Option<u64>,
    /// Retained only for native pointer ownership; it never becomes an editor scroll state.
    manual_pointer: bool,
}

#[derive(PartialEq)]
struct Measurement {
    document: protocol::api::DocumentVersion,
    ui_revision: u64,
    anchor: Anchor,
    offset: Point<Pixels>,
    bounds: Bounds<Pixels>,
    line_height: Option<Pixels>,
}

struct Pending {
    document: protocol::api::DocumentVersion,
    ui_revision: u64,
    origin: u64,
    locator: Locator,
}

impl SourceTracking {
    /// A hidden side, disabled preference, source change or retired owner withdraws pending work.
    pub(crate) fn reset(&mut self) {
        self.last = None;
        self.pending = None;
        self.settled_origin = None;
        // A temporary scene withdrawal cannot end an ongoing Base drag; the global release ends it.
    }

    /// A removed observer cannot receive release; true hiding or retirement ends its gesture ownership.
    pub(crate) fn withdraw(&mut self) {
        self.reset();
        self.manual_pointer = false;
    }

    /// The editor operation acknowledges a queued locate; every later layout rechecks its scene.
    /// Native pointer ownership persists through release and rejects incoming locates as Cancelled.
    pub(crate) fn locate(
        &mut self,
        document: protocol::api::DocumentVersion,
        ui_revision: u64,
        origin: u64,
        offset: usize,
        fraction: f32,
    ) -> Result<(), protocol::api::Failure> {
        if self.manual_pointer {
            return Err(protocol::api::Failure::new(
                protocol::api::ErrorCode::Cancelled,
                "Manual source viewport drag is active",
            ));
        }
        self.settled_origin = None;
        self.pending = Some(Pending {
            document,
            ui_revision,
            origin,
            locator: Locator::new(offset, fraction),
        });
        Ok(())
    }
}

impl EditorApp {
    /// The guest opts into synchronization by mounting both its mapped Scroll and native source.
    pub(crate) fn editor_preview_sync_enabled(
        &self,
        preview: &Entity<ExtensionPanel>,
        cx: &App,
    ) -> bool {
        preview.read(cx).current_document().is_some_and(|scene| {
            scene.editor_viewport.is_some() && scene.active_native_editor().is_some()
        })
    }

    /// Observe manual input without consuming it; Base continues to own keys, wheel and scrollbar behavior.
    pub(in crate::extensions) fn render_source_viewport_probe(
        &self,
        source: AnyElement,
        preview: Entity<ExtensionPanel>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // This renderer can run inside PluginView.render. Configure the view in the parent
        // before mounting it; updating that same entity here would violate GPUI borrowing.
        let owner = cx.entity().downgrade();
        div()
            .size_full()
            // The borrowed editor grows inside this probe just as it does in the native fallback.
            .flex()
            .flex_col()
            .min_h_0()
            .relative()
            .capture_key_down(cx.listener(|app, _, window, cx| {
                // Observe navigation at the editor's ancestor so nested dispatch nodes cannot hide it.
                if app.editor.focus_handle(cx).is_focused(window) {
                    app.cancel_source_viewport_location(cx);
                }
            }))
            .child(source)
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        // Base records its final source geometry during paint, before this transparent overlay.
                        let _ = owner.update(cx, |app, cx| {
                            app.measure_source_viewport(&preview, window, cx)
                        });
                        let wheel_owner = owner.clone();
                        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, cx| {
                            if phase == gpui_kit::DispatchPhase::Capture {
                                let _ = wheel_owner.update(cx, |app, cx| {
                                    if app.editor.read(cx).input_bounds().contains(&event.position)
                                    {
                                        app.cancel_source_viewport_location(cx);
                                    }
                                });
                            }
                        });
                        // A thumb drag begins with a source-pane press, including the gutter beyond input_bounds.
                        let pointer_owner = owner.clone();
                        window.on_mouse_event(
                            move |event: &gpui_kit::MouseDownEvent, phase, _, cx| {
                                if phase == gpui_kit::DispatchPhase::Capture
                                    && bounds.contains(&event.position)
                                {
                                    let _ = pointer_owner.update(cx, |app, cx| {
                                        app.set_source_viewport_pointer(true, cx)
                                    });
                                }
                            },
                        );
                        let drag_owner = owner.clone();
                        window.on_mouse_event(
                            move |event: &gpui_kit::MouseMoveEvent, phase, _, cx| {
                                if phase == gpui_kit::DispatchPhase::Capture
                                    && event.pressed_button.is_some()
                                {
                                    // Late reverse requests must not retake ownership during a thumb drag.
                                    let _ = drag_owner.update(cx, |app, cx| {
                                        if app.active_editor_preview(cx).is_some_and(|panel| {
                                            panel.read(cx).source_viewport.manual_pointer
                                        }) {
                                            app.cancel_source_viewport_location(cx);
                                        }
                                    });
                                }
                            },
                        );
                        let release_owner = owner.clone();
                        window.on_mouse_event(move |_: &gpui_kit::MouseUpEvent, phase, _, cx| {
                            if phase == gpui_kit::DispatchPhase::Capture {
                                let _ = release_owner.update(cx, |app, cx| {
                                    app.set_source_viewport_pointer(false, cx)
                                });
                            }
                        });
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
            .into_any_element()
    }

    /// Manual source movement supersedes a queued reverse locate before its next layout step.
    pub(crate) fn cancel_source_viewport_location(&self, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_preview(cx) {
            panel.update(cx, |panel, _| {
                panel.source_viewport.pending = None;
                panel.source_viewport.settled_origin = None;
            });
        }
    }

    /// Retain a source press through global release so late reverse requests cannot interrupt Base dragging.
    fn set_source_viewport_pointer(&self, held: bool, cx: &mut Context<Self>) {
        self.cancel_source_viewport_location(cx);
        if let Some(panel) = self.active_editor_preview(cx) {
            panel.update(cx, |panel, _| panel.source_viewport.manual_pointer = held);
        }
    }

    fn measure_source_viewport(
        &mut self,
        panel: &Entity<ExtensionPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.session_state.workspace_trusted
            || self.active_editor_preview(cx).as_ref() != Some(panel)
            || !self.editor_preview_sync_enabled(panel, cx)
        {
            panel.update(cx, |panel, _| panel.source_viewport.reset());
            return;
        }
        let Some(document) = self
            .active_tab_index()
            .and_then(|index| self.plugin_document_version(index).ok())
        else {
            return;
        };
        let Some(ui_revision) = panel
            .read(cx)
            .current_document()
            .filter(|scene| {
                scene.source.as_ref() == Some(&document)
                    && scene.dialog.is_none()
                    && scene.menu.is_none()
            })
            .map(|scene| scene.revision)
        else {
            panel.update(cx, |panel, _| panel.source_viewport.reset());
            return;
        };
        let editor = self.editor.clone();
        let mut scroll = None;
        panel.update(cx, |panel, cx| {
            let tracking = &mut panel.source_viewport;
            if let Some(pending) = &mut tracking.pending {
                if pending.document != document || pending.ui_revision != ui_revision {
                    tracking.pending = None;
                } else {
                    match pending.locator.step(editor.read(cx)) {
                        Step::Scroll(offset) => {
                            scroll = Some(offset);
                            return;
                        }
                        Step::Failed => {
                            tracking.pending = None;
                            return;
                        }
                        Step::Settled => {
                            tracking.settled_origin = Some(pending.origin);
                            tracking.pending = None;
                        }
                    }
                }
            }
            let state = editor.read(cx);
            let Some(anchor) = viewport::sample(state, window, cx) else {
                return;
            };
            let origin = tracking.settled_origin.take();
            let next = Measurement {
                document: document.clone(),
                ui_revision,
                anchor,
                offset: state.scroll_offset(),
                bounds: state.input_bounds(),
                line_height: state.line_height(),
            };
            if tracking.last.as_ref() == Some(&next) && origin.is_none() {
                return;
            }
            // Native reflow can clamp Y while changing width/height/font metrics; it is still layout input.
            let layout = tracking.last.as_ref().is_none_or(|old| {
                old.offset.y == next.offset.y
                    || old.bounds.size != next.bounds.size
                    || old.line_height != next.line_height
            });
            tracking.last = Some(next);
            panel.send(protocol::api::Notification::SourceViewport(
                protocol::api::SourceViewport {
                    document: document.clone(),
                    ui_revision,
                    offset: anchor.offset,
                    line_fraction: anchor.line_fraction,
                    origin,
                    layout,
                },
            ));
        });
        if let Some(offset) = scroll {
            editor.update(cx, |editor, cx| {
                editor.set_scroll_offset(offset, cx);
                cx.notify();
            });
            // Base's setter applies during layout. Wake only the next finite location step, never a permanent loop.
            let owner = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = owner.update(cx, |app, cx| {
                    app.editor_panel.update(cx, |_, cx| cx.notify());
                    cx.notify();
                });
            });
        }
    }
}
