//! Read and locate the native source viewport without changing document editing state.
//!
//! The public Base geometry is available only for laid-out buffer rows. A host
//! owner must recheck its document/UI revision before each step, execute only
//! the returned scroll, and revisit the locator after the editor paints.

use gpui_base::input::{EditorState, RopeExt as _};
use gpui_kit::{App, Pixels, Point, Window, point, px};

/// The owner paints between steps, so this bounds both layout work and an unsuccessful search.
const MAX_STEPS: u8 = 48;
const TOLERANCE: f32 = 0.5;

/// The first visible UTF-8 caret and the fraction of its visual row above the viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Anchor {
    pub(crate) offset: usize,
    pub(crate) line_fraction: f32,
}

/// Sample actual source geometry; unavailable before layout or while geometry is settling.
pub(crate) fn sample(editor: &EditorState, window: &Window, cx: &App) -> Option<Anchor> {
    let layout = Layout::read(editor)?;
    let viewport = editor.input_bounds();
    let text = editor.text();
    let visible = editor.visible_row_range()?;
    let first = text.line_start_offset(visible.start);
    let first_bounds = editor.range_to_bounds(&(first..first))?;
    // The fixed gutter does not move with horizontal text scrolling. Let the
    // existing native hit test find the top visual row inside its text area.
    let content_left = first_bounds.left() - editor.scroll_offset().x;
    if content_left >= viewport.right() {
        return None;
    }
    let pointer = point(content_left.max(viewport.left()), viewport.top());
    let hit = super::caret_offset_at(editor, pointer, window, cx)?;
    let row = editor.range_to_bounds(&(hit..hit))?;
    if row.bottom() <= viewport.top() || row.top() > viewport.top() + px(TOLERANCE) {
        return None;
    }

    // Horizontal scrolling can hit a later caret on the same visual row. Use
    // actual row geometry to recover its first UTF-8 caret without re-shaping.
    let head = text.line_start_offset(text.offset_to_point(hit).row);
    let mut low = text.offset_to_char_index(head);
    let mut high = text.offset_to_char_index(hit);
    while low < high {
        let middle = low + (high - low) / 2;
        let offset = text.char_index_to_offset(middle);
        let bounds = editor.range_to_bounds(&(offset..offset))?;
        if bounds.top() < row.top() - px(TOLERANCE) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    let mut offset = text.char_index_to_offset(low);
    // A CRLF is one native caret boundary even though both bytes are UTF-8 boundaries.
    if offset > 0 && text.char_at(offset - 1) == Some('\r') && text.char_at(offset) == Some('\n') {
        offset -= 1;
    }
    let bounds = editor.range_to_bounds(&(offset..offset))?;
    Some(Anchor {
        offset,
        line_fraction: (f32::from(viewport.top() - bounds.top()) / layout.line_height)
            .clamp(0., 1.),
    })
}

/// Bounded, layout-driven vertical location; no text, selection, or editor entity is retained.
pub(crate) struct Locator {
    target: Anchor,
    lower: f32,
    upper: Option<f32>,
    distance: f32,
    layout: Option<Layout>,
    previous: Option<Proposal>,
    steps: u8,
    finished: Option<Step>,
}

/// A reflow invalidates pixel search brackets, but never the read-only target offset.
#[derive(Clone, Copy, PartialEq)]
struct Layout {
    width: f32,
    height: f32,
    line_height: f32,
}

impl Layout {
    /// Reject a clamp frame whose painted text still uses the old deferred offset.
    fn read(editor: &EditorState) -> Option<Self> {
        let viewport = editor.input_bounds();
        let content = editor.text_bounds()?;
        let line_height = f32::from(editor.line_height()?);
        let offset = editor.scroll_offset();
        if viewport.size.width <= px(0.)
            || viewport.size.height <= px(0.)
            || !line_height.is_finite()
            || line_height <= 0.
            || !f32::from(offset.y).is_finite()
            || (content.top() - viewport.top() - offset.y).abs() > px(TOLERANCE)
        {
            return None;
        }
        Some(Self {
            width: f32::from(viewport.size.width),
            height: f32::from(viewport.size.height),
            line_height,
        })
    }
}

/// Compare a proposed scroll with the following painted frame to observe the native clamp.
#[derive(Clone, Copy)]
struct Proposal {
    before: f32,
    requested: f32,
    exact: bool,
}

/// One native layout step. The owner executes `Scroll` with Base's public setter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Step {
    Scroll(Point<Pixels>),
    Settled,
    Failed,
}

impl Locator {
    /// Locate a UTF-8 offset at the top, preserving the requested visual-row fraction.
    /// Invalid boundaries or non-finite/out-of-range fractions fail without mutation.
    pub(crate) fn new(offset: usize, line_fraction: f32) -> Self {
        Self {
            target: Anchor {
                offset,
                line_fraction,
            },
            lower: 0.,
            upper: None,
            distance: 0.,
            layout: None,
            previous: None,
            steps: 0,
            finished: None,
        }
    }

    /// Read the latest painted geometry and propose only a vertical scroll offset.
    /// Callers retain ownership of cancellation, source identity and origin suppression.
    pub(crate) fn step(&mut self, editor: &EditorState) -> Step {
        if let Some(finished) = self.finished {
            return finished;
        }
        let text = editor.text();
        if self.target.offset > text.len()
            || !text.is_char_boundary(self.target.offset)
            || !self.target.line_fraction.is_finite()
            || !(0.0..=1.0).contains(&self.target.line_fraction)
            || self.steps >= MAX_STEPS
        {
            return self.finish(Step::Failed);
        }
        self.steps += 1;
        let Some(layout) = Layout::read(editor) else {
            // A pending deferred/clamped frame must paint once more before its
            // visible buffer rows can become evidence for a search decision.
            return Step::Scroll(editor.scroll_offset());
        };
        if self.layout != Some(layout) {
            self.layout = Some(layout);
            self.lower = 0.;
            self.upper = None;
            self.distance = layout.height.max(layout.line_height);
            self.previous = None;
        }
        let depth = (-f32::from(editor.scroll_offset().y)).max(0.);
        let row = text.offset_to_point(self.target.offset).row;
        let Some(visible) = editor.visible_row_range().filter(|range| !range.is_empty()) else {
            return Step::Scroll(editor.scroll_offset());
        };
        if visible.contains(&row) {
            return self.align(editor, layout, depth);
        }

        // Base's range lookup can alias an offset before the first laid-out
        // buffer row to that row's start. Trust coordinates only after the
        // requested buffer row enters the actual layout window above.
        if self.previous.is_some_and(|previous| {
            !previous.exact
                && (previous.requested - previous.before).abs() > TOLERANCE
                && (depth - previous.before).abs() <= TOLERANCE
        }) {
            return self.finish(Step::Failed);
        }
        if row < visible.start {
            self.upper = Some(self.upper.map_or(depth, |upper| upper.min(depth)));
        } else {
            self.lower = self.lower.max(depth);
        }
        let requested = if let Some(upper) = self.upper {
            if upper - self.lower <= TOLERANCE {
                return self.finish(Step::Failed);
            }
            self.lower + (upper - self.lower) / 2.
        } else {
            // There is no public total display-row count. Expand the measured
            // search span until a real layout brackets the target, then bisect.
            let next = depth + self.distance;
            self.distance *= 2.;
            next
        };
        self.propose(editor, depth, requested, false)
    }

    /// A whole wrapped buffer line is shaped together, so its distant visual rows are precise here.
    fn align(&mut self, editor: &EditorState, layout: Layout, depth: f32) -> Step {
        let offset = self.target.offset;
        let Some(bounds) = editor.range_to_bounds(&(offset..offset)) else {
            // Hidden folded text is not unfolded and no caret is moved to manufacture geometry.
            return self.finish(Step::Failed);
        };
        let delta = f32::from(editor.input_bounds().top() - bounds.top())
            - layout.line_height * self.target.line_fraction;
        if delta.abs() <= TOLERANCE {
            return self.finish(Step::Settled);
        }
        if self.previous.is_some_and(|previous| {
            previous.exact
                && (previous.requested - previous.before).abs() > TOLERANCE
                && (depth - previous.before).abs() <= TOLERANCE
        }) {
            // A valid final row may not reach the very top in a short document.
            // A measured, unchanged native offset is success at its scroll clamp.
            return self.finish(Step::Settled);
        }
        let requested = (depth - delta).max(0.);
        if (requested - depth).abs() <= TOLERANCE {
            return self.finish(Step::Settled);
        }
        self.propose(editor, depth, requested, true)
    }

    /// Keep horizontal scrolling intact and remember only the proposed vertical measurement.
    fn propose(&mut self, editor: &EditorState, before: f32, requested: f32, exact: bool) -> Step {
        if !requested.is_finite() || (requested - before).abs() <= TOLERANCE {
            return self.finish(Step::Failed);
        }
        self.previous = Some(Proposal {
            before,
            requested,
            exact,
        });
        Step::Scroll(point(editor.scroll_offset().x, px(-requested)))
    }

    /// Terminal results remain stable while the owner retires the pending origin/request.
    fn finish(&mut self, result: Step) -> Step {
        self.finished = Some(result);
        result
    }
}

#[cfg(test)]
mod tests;
