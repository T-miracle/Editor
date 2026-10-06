//! Semantic preview positions use live Base layout, with one versioned notification per frame.
//! This projection retains measurements and receipts, never another authoritative scroll state.
use super::*;
use gpui_kit::{Bounds, Pixels, Point, Size, point, px};
use plugin_runtime::plugin_protocol::api;

/// The host enables this only for its authorized split preview and current synchronization preference.
#[derive(Default)]
pub(super) struct ViewportState {
    enabled: bool,
    frame: u64,
    measured: BTreeSet<String>,
    last: Option<Snapshot>,
    pending: Option<Locate>,
    program: Option<Program>,
    moved: bool,
    queued: Option<Emission>,
    scheduled: bool,
    /// Manual ownership spans the whole native drag, including motion outside its original pane.
    manual_pointer: bool,
}

/// A locate belongs to an exact UI revision and is consumed by its target's next native measurement.
struct Locate {
    node: String,
    fraction: f32,
    origin: u64,
    revision: u64,
}

/// This expected offset identifies one receipt; Base remains the sole owner of the actual offset.
struct Program {
    origin: u64,
    offset: Point<Pixels>,
}

/// Content-relative geometry distinguishes layout changes from ordinary viewport translation.
struct Snapshot {
    size: Size<Pixels>,
    offset: Point<Pixels>,
    geometry: Vec<(String, Bounds<Pixels>)>,
    position: api::PreviewViewport,
}

/// The latest complete layout replaces older queued layouts before a single deferred guest callback.
struct Emission {
    revision: u64,
    scroll: String,
    snapshot: Snapshot,
}

impl ViewportState {
    /// Modal, source, tab or scene replacement revokes pending work without changing the host preference.
    pub(super) fn reset_scene(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.measured.clear();
        self.last = None;
        self.cancel_locate();
        self.moved = false;
        // Ordinary source/theme remeasurement does not end a native pointer gesture; release does.
    }

    /// Explicit navigation and subsequent manual input take ownership from a prior semantic locate.
    pub(super) fn cancel_locate(&mut self) {
        self.pending = None;
        self.program = None;
        self.queued = None;
    }

    /// A prepaint setter changes the next frame; the currently measured bounds must not emit yet.
    pub(super) fn scroll_changed(&mut self) {
        self.moved = true;
        self.queued = None;
    }
}

impl PluginView {
    /// Enable current-scene viewport reporting after host authorization and split/preference checks.
    /// Disabling immediately revokes queued notifications and locates; ordinary link reveals still work.
    pub(crate) fn set_viewport_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.viewport.enabled != enabled {
            self.viewport.enabled = enabled;
            self.viewport.reset_scene();
            if !enabled {
                self.viewport.manual_pointer = false;
            }
            cx.notify();
        }
    }

    /// Locate one active mapped block using its current height, without changing focus or editor text.
    /// `node` belongs to the registered Scroll; `fraction` is 0..=1 within that block's rendered height.
    /// The nonzero `origin` is echoed once and `revision` must equal the current UI scene revision.
    /// Reject stale scenes, inactive ownership, nonfinite fractions and zero receipt identities.
    /// A native pointer gesture holds manual ownership until release and rejects late locates as Cancelled.
    pub(crate) fn locate_viewport(
        &mut self,
        node: &str,
        fraction: f32,
        origin: u64,
        revision: u64,
        cx: &mut Context<Self>,
    ) -> Result<(), api::Failure> {
        if revision != self.document.revision {
            return Err(api::Failure::new(
                api::ErrorCode::StaleRevision,
                "Preview scene changed",
            ));
        }
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) || origin == 0 {
            return Err(api::Failure::new(
                api::ErrorCode::InvalidRequest,
                "Invalid preview viewport position",
            ));
        }
        let scroll = self.viewport_scroll().ok_or_else(inactive)?;
        if self.viewport.manual_pointer {
            return Err(api::Failure::new(
                api::ErrorCode::Cancelled,
                "Manual viewport drag is active",
            ));
        }
        if self
            .scene_layout
            .source_blocks
            .get(node)
            .is_none_or(|block| block.scroll != scroll)
        {
            return Err(inactive());
        }
        // Latest intent wins before measurement; a superseded request never emits its origin.
        self.viewport.cancel_locate();
        self.scene_layout.cancel_reveal();
        self.viewport.pending = Some(Locate {
            node: node.into(),
            fraction,
            origin,
            revision,
        });
        cx.notify();
        Ok(())
    }

    /// Modal ownership and inactive selected tabs cannot report the underlying source viewport.
    fn viewport_scroll(&self) -> Option<&str> {
        if !self.viewport.enabled
            || self.document.source.is_none()
            || self.document.dialog.is_some()
            || self.document.menu.is_some()
        {
            return None;
        }
        let id = self.document.editor_viewport.as_deref()?;
        self.document
            .active_node(id)
            .filter(|node| matches!(node.kind, Kind::Scroll { .. }))?;
        Some(id)
    }

    /// Every render collects only its own measured blocks; old or hidden geometry cannot win selection.
    pub(super) fn begin_viewport_frame(&mut self) -> u64 {
        self.viewport.frame = self.viewport.frame.wrapping_add(1);
        self.viewport.measured.clear();
        self.viewport.moved = false;
        self.viewport.frame
    }

    /// Native wheel input supersedes an unacknowledged program locate before Base applies its delta.
    pub(super) fn viewport_wheel(&mut self, position: Point<Pixels>, revision: u64) -> bool {
        if revision == self.document.revision
            && self
                .viewport_scroll()
                .and_then(|id| self.scrolls.get(id))
                .is_some_and(|scroll| scroll.bounds().contains(&position))
        {
            self.viewport.cancel_locate();
            self.viewport.queued = None;
            return true;
        }
        false
    }

    /// A native press retains gesture ownership until release; Base still performs the actual drag.
    pub(super) fn viewport_pointer_down(&mut self, position: Point<Pixels>, revision: u64) {
        if self.viewport_wheel(position, revision) {
            self.viewport.manual_pointer = true;
        }
    }

    /// Held motion revokes pending receipts even after the pointer leaves its original viewport.
    pub(super) fn viewport_pointer_move(&mut self) {
        if self.viewport.manual_pointer {
            self.viewport.cancel_locate();
        }
    }

    /// Global release ends the gesture without changing the user's final Base offset.
    pub(super) fn viewport_pointer_up(&mut self) {
        self.viewport.manual_pointer = false;
    }

    /// A locate changes the native handle once. A following complete frame reports its clamped result.
    pub(super) fn measure_viewport_block(
        &mut self,
        node: &str,
        revision: u64,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if revision != self.document.revision {
            return;
        }
        let Some(scroll_id) = self.viewport_scroll().map(str::to_owned) else {
            return;
        };
        if self
            .scene_layout
            .source_blocks
            .get(node)
            .is_none_or(|block| block.scroll != scroll_id)
        {
            return;
        }
        self.viewport.measured.insert(node.into());
        if self
            .viewport
            .pending
            .as_ref()
            .is_none_or(|locate| locate.node != node || locate.revision != revision)
        {
            return;
        }
        let Some(scroll) = self.scrolls.get(&scroll_id).cloned() else {
            return;
        };
        let locate = self.viewport.pending.take().unwrap();
        let offset = scroll.offset();
        let desired = offset.y
            - (bounds.top() + bounds.size.height * locate.fraction - scroll.bounds().top());
        let offset = point(offset.x, desired.max(-scroll.max_offset().y).min(px(0.)));
        self.viewport.program = Some(Program {
            origin: locate.origin,
            offset,
        });
        if scroll.offset() != offset {
            scroll.set_offset(offset);
            self.viewport.scroll_changed();
            cx.notify();
        }
    }

    /// Paint follows all block prepaint callbacks, so no partially measured scene enters a guest queue.
    pub(super) fn finish_viewport_frame(
        &mut self,
        frame: u64,
        revision: u64,
        cx: &mut Context<Self>,
    ) {
        if frame != self.viewport.frame || revision != self.document.revision {
            return;
        }
        let Some(scroll) = self.viewport_scroll().map(str::to_owned) else {
            return;
        };
        if self.viewport.moved {
            return;
        }
        let Some(mut snapshot) = self.viewport_snapshot(&scroll) else {
            return;
        };
        let layout = self
            .viewport
            .last
            .as_ref()
            .is_none_or(|old| !same_layout(old, &snapshot));
        snapshot.position.layout = layout;
        if let Some(program) = &self.viewport.program {
            // Unexpected translation is another scroll intent, not an indefinitely sticky origin.
            if program.offset == snapshot.offset {
                snapshot.position.origin = Some(program.origin);
            } else {
                self.viewport.program = None;
            }
        }
        let changed = snapshot.position.origin.is_some()
            || layout
            || self.viewport.last.as_ref().is_none_or(|old| {
                old.offset != snapshot.offset
                    || old.position.block != snapshot.position.block
                    || old.position.fraction != snapshot.position.fraction
            });
        if !changed {
            self.viewport.queued = None;
            return;
        }
        self.viewport.queued = Some(Emission {
            revision,
            scroll,
            snapshot,
        });
        if !self.viewport.scheduled {
            self.viewport.scheduled = true;
            let owner = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = owner.update(cx, |view, cx| view.flush_viewport(cx));
            });
        }
    }

    /// Notification authorization is checked again after defer; withdrawn scenes cannot emit receipts.
    fn flush_viewport(&mut self, cx: &mut Context<Self>) {
        self.viewport.scheduled = false;
        let Some(queued) = self.viewport.queued.take() else {
            return;
        };
        if self.viewport_scroll() != Some(queued.scroll.as_str())
            || queued.revision != self.document.revision
        {
            return;
        }
        if self.emit_version(
            &queued.scroll,
            queued.revision,
            Action::Viewport(queued.snapshot.position.clone()),
            cx,
        ) {
            self.viewport.program = None;
            self.viewport.last = Some(queued.snapshot);
        }
    }

    /// Bounds are expressed relative to the registered viewport with its Base offset removed.
    fn viewport_snapshot(&self, scroll: &str) -> Option<Snapshot> {
        let handle = self.scrolls.get(scroll)?;
        let viewport = handle.bounds();
        if viewport.size.height <= px(0.) || viewport.size.width <= px(0.) {
            return None;
        }
        let offset = handle.offset();
        let mut geometry = Vec::new();
        let mut candidates = Vec::new();
        for id in &self.viewport.measured {
            let Some(block) = self.scene_layout.source_blocks.get(id) else {
                continue;
            };
            let Some(bounds) = self.scene_layout.bounds.get(id).copied() else {
                continue;
            };
            if block.scroll != scroll || bounds.size.height <= px(0.) {
                continue;
            }
            geometry.push((
                id.clone(),
                Bounds::new(bounds.origin - viewport.origin - offset, bounds.size),
            ));
            if bounds.right() > viewport.left() && bounds.left() < viewport.right() {
                candidates.push((id, block, bounds));
            }
        }
        let (id, block, bounds) = select_block(&candidates, viewport)?;
        let fraction = ((f32::from(viewport.top()) - f32::from(bounds.top()))
            / f32::from(bounds.size.height))
        .clamp(0., 1.);
        Some(Snapshot {
            size: viewport.size,
            offset,
            geometry,
            position: api::PreviewViewport {
                block: id.clone(),
                source_range: block.range,
                fraction,
                origin: None,
                layout: false,
            },
        })
    }
}

/// Floating-point cancellation of a translated block must not turn manual scrolling into reflow.
/// Subpixel noise is below native layout resolution; cumulative real changes still differ from last emission.
fn same_layout(left: &Snapshot, right: &Snapshot) -> bool {
    let near = |left: Pixels, right: Pixels| (left - right).abs() <= px(0.1);
    near(left.size.width, right.size.width)
        && near(left.size.height, right.size.height)
        && left.geometry.len() == right.geometry.len()
        && left
            .geometry
            .iter()
            .zip(&right.geometry)
            .all(|((left_id, left), (right_id, right))| {
                left_id == right_id
                    && near(left.origin.x, right.origin.x)
                    && near(left.origin.y, right.origin.y)
                    && near(left.size.width, right.size.width)
                    && near(left.size.height, right.size.height)
            })
}

/// Prefer the deepest leaf crossing the top. Padding gaps use the nearest visible leaf before containers.
/// Equal-height table cells use source order, which stays deterministic independently of node spelling.
fn select_block<'a>(
    blocks: &'a [(&'a String, &'a layout::SourceBlock, Bounds<Pixels>)],
    viewport: Bounds<Pixels>,
) -> Option<(&'a String, &'a layout::SourceBlock, Bounds<Pixels>)> {
    let crosses_top =
        |bounds: Bounds<Pixels>| bounds.top() <= viewport.top() && bounds.bottom() > viewport.top();
    let visible = |bounds: Bounds<Pixels>| {
        bounds.bottom() > viewport.top() && bounds.top() < viewport.bottom()
    };
    let deepest = |left: &(&String, &layout::SourceBlock, Bounds<Pixels>),
                   right: &(&String, &layout::SourceBlock, Bounds<Pixels>)| {
        left.1
            .depth
            .cmp(&right.1.depth)
            .then_with(|| right.1.range.start.cmp(&left.1.range.start))
    };
    blocks
        .iter()
        .copied()
        .filter(|(_, block, bounds)| !block.has_children && crosses_top(*bounds))
        .max_by(deepest)
        .or_else(|| {
            blocks
                .iter()
                .copied()
                .filter(|(_, block, bounds)| !block.has_children && visible(*bounds))
                .min_by(|left, right| {
                    (left.2.top() - viewport.top())
                        .abs()
                        .partial_cmp(&(right.2.top() - viewport.top()).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| deepest(right, left))
                })
        })
        .or_else(|| {
            blocks
                .iter()
                .copied()
                .filter(|(_, _, bounds)| crosses_top(*bounds))
                .max_by(deepest)
        })
}

/// Native failures preserve the typed platform boundary without exposing plugin-specific assumptions.
fn inactive() -> api::Failure {
    api::Failure::new(
        api::ErrorCode::InvalidState,
        "Block has no enabled source viewport owner",
    )
}
