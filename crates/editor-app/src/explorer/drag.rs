//! Typed GPUI gestures share the transfer worker and own only temporary tree navigation.
use super::transfer::Kind;
use crate::*;
use gpui_kit::{DragMoveEvent, ExternalPaths, KeyDownEvent, MouseUpEvent};

/// Stable path payload survives row virtualization while the user is dragging.
#[derive(Clone)]
pub(crate) struct TreeDrag {
    pub path: PathBuf,
}
impl Render for TreeDrag {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::controls::file_operation::drag_preview(
            self.path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            copy_modifier(window.modifiers()),
            cx,
        )
    }
}

/// File-drag copy uses the familiar modifier on the current host platform.
fn copy_modifier(modifiers: Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.alt
    } else {
        modifiers.control
    }
}

#[derive(Default)]
pub(crate) struct DragState {
    gesture: Option<Gesture>,
    generation: u64,
}
struct Gesture {
    sources: Vec<PathBuf>,
    internal: bool,
    target: Option<PathBuf>,
    bounds: Bounds<Pixels>,
    hover: Option<PathBuf>,
    hover_generation: u64,
    temporary: Vec<PathBuf>,
    edge: i8,
    copy: bool,
}
impl DragState {
    pub(crate) fn target(&self) -> Option<&Path> {
        self.gesture
            .as_ref()
            .and_then(|gesture| gesture.target.as_deref())
    }
    pub(crate) fn intent(&self) -> Option<bool> {
        self.gesture
            .as_ref()
            .filter(|gesture| gesture.target.is_some())
            .map(|gesture| gesture.copy)
    }
}

impl EditorApp {
    /// Root capture observes typed movement underneath opaque row hitboxes.
    pub(crate) fn tree_drag_move(
        &mut self,
        event: &DragMoveEvent<TreeDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = event.drag(cx).path.clone();
        self.track_tree_drag(
            vec![source],
            true,
            event.bounds,
            event.event.position,
            window,
            cx,
        );
    }
    pub(crate) fn external_tree_drag_move(
        &mut self,
        event: &DragMoveEvent<ExternalPaths>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sources = event.drag(cx).paths().to_vec();
        self.track_tree_drag(
            sources,
            false,
            event.bounds,
            event.event.position,
            window,
            cx,
        );
    }
    fn track_tree_drag(
        &mut self,
        sources: Vec<PathBuf>,
        internal: bool,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_transfers.is_running() || self.explorer_menu.is_some() {
            return;
        }
        if self.explorer_drag.gesture.is_none() {
            self.explorer_drag.generation += 1;
            let generation = self.explorer_drag.generation;
            self.explorer_drag.gesture = Some(Gesture {
                sources,
                internal,
                bounds,
                target: None,
                hover: None,
                hover_generation: 0,
                temporary: Vec::new(),
                edge: 0,
                copy: !internal || copy_modifier(window.modifiers()),
            });
            // One gesture timer scrolls at the edge and releases state when the platform cancels.
            cx.spawn_in(window, async move |app, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(80))
                        .await;
                    let running = app
                        .update_in(cx, |app, window, cx| {
                            if app.explorer_drag.generation != generation {
                                return false;
                            }
                            if !cx.has_active_drag() {
                                app.finish_tree_drag(false, cx);
                                return false;
                            }
                            if let Some(gesture) = &mut app.explorer_drag.gesture {
                                gesture.copy =
                                    !gesture.internal || copy_modifier(window.modifiers());
                                let scroll = app.tree_state.read(cx).scroll_handle().clone();
                                let handle = scroll.0.borrow().base_handle.clone();
                                let mut offset = handle.offset();
                                offset.y -= px(gesture.edge as f32 * 18.);
                                handle.set_offset(offset);
                                // The virtual list is rendered by TreeState, so its entity must redraw too.
                                app.tree_state.update(cx, |_, cx| cx.notify());
                                cx.notify();
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false);
                    if !running {
                        break;
                    }
                }
            })
            .detach();
        }
        let gesture = self.explorer_drag.gesture.as_mut().unwrap();
        gesture.bounds = bounds;
        gesture.copy = !internal || copy_modifier(window.modifiers());
        let inside = bounds.contains(&position);
        gesture.target = inside.then(|| self.workspace.root().to_path_buf());
        gesture.edge = if !inside {
            0
        } else if position.y < bounds.top() + px(24.) {
            -1
        } else if position.y > bounds.bottom() - px(24.) {
            1
        } else {
            0
        };
        cx.notify();
    }

    /// A hit file resolves to its parent; the outline follows the actual destination directory.
    pub(crate) fn tree_drag_row(
        &mut self,
        path: &Path,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !bounds.contains(&position) {
            return;
        }
        let target = self.explorer_destination(Some(path));
        let Some(gesture) = &mut self.explorer_drag.gesture else {
            return;
        };
        gesture.target = Some(target.clone());
        if gesture.hover.as_ref() == Some(&target) {
            return;
        }
        gesture.hover = Some(target.clone());
        gesture.hover_generation += 1;
        let hover_generation = gesture.hover_generation;
        let generation = self.explorer_drag.generation;
        cx.spawn_in(window, async move |app, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(600))
                .await;
            let _ = app.update_in(cx, |app, window, cx| {
                if app.explorer_drag.generation != generation || !cx.has_active_drag() {
                    return;
                }
                let Some(gesture) = &mut app.explorer_drag.gesture else {
                    return;
                };
                if gesture.hover_generation != hover_generation
                    || gesture.target.as_ref() != Some(&target)
                    || !gesture.bounds.contains(&window.mouse_position())
                {
                    return;
                }
                let roots = explorer::tree::root_items(app.tree_state.read(cx));
                if let Some(item) = find_tree_item(&roots, &target)
                    && !item.is_expanded()
                {
                    item.clone().expanded(true);
                    gesture.temporary.push(target);
                    app.tree_state
                        .update(cx, |state, cx| state.set_items(roots, cx));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Cancel restores only this gesture's expansion, preserving folders that were already open.
    fn finish_tree_drag(&mut self, accepted: bool, cx: &mut Context<Self>) {
        let Some(gesture) = self.explorer_drag.gesture.take() else {
            return;
        };
        self.explorer_drag.generation += 1;
        if !accepted {
            let roots = explorer::tree::root_items(self.tree_state.read(cx));
            for path in gesture.temporary {
                if let Some(item) = find_tree_item(&roots, &path) {
                    item.clone().expanded(false);
                }
            }
            self.tree_state
                .update(cx, |state, cx| state.set_items(roots, cx));
        }
        cx.notify();
    }

    /// Capture releases before child hitboxes and route both platform and tree offers to the worker.
    pub(crate) fn cancel_explorer_drag(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "escape" && self.explorer_drag.gesture.is_some() {
            cx.stop_active_drag(window);
            self.finish_tree_drag(false, cx);
            cx.stop_propagation();
        }
    }

    /// Release capture is independent of which child currently owns keyboard focus.
    pub(crate) fn render_tree_drag_events(&self, cx: &Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        gpui_kit::canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                let releasing = owner.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if !phase.capture() || event.button != MouseButton::Left {
                        return;
                    }
                    let _ = releasing.update(cx, |app, cx| {
                        let Some(gesture) = &app.explorer_drag.gesture else {
                            return;
                        };
                        let target = gesture
                            .target
                            .clone()
                            .filter(|_| gesture.bounds.contains(&event.position));
                        let sources = gesture.sources.clone();
                        let kind = if !gesture.internal || copy_modifier(event.modifiers) {
                            Kind::Copy
                        } else {
                            Kind::Move
                        };
                        if cx.has_active_drag()
                            && let Some(target) = target
                        {
                            app.finish_tree_drag(true, cx);
                            cx.stop_active_drag(window);
                            cx.stop_propagation();
                            app.start_file_transfer(sources, target, kind, window, cx);
                        } else {
                            app.finish_tree_drag(false, cx);
                        }
                    });
                });
            },
        )
        .absolute()
        .size(px(0.))
    }
}
