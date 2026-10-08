//! Semantic scroll intents retain only readonly source/block positions and bounded request ownership.
use super::Source;
use plugin_protocol::{api, ui};

#[derive(Default)]
pub(super) struct Scrolling {
    driver: Option<Driver>,
    pending: Option<Pending>,
    latest: Option<Intent>,
    origin: u64,
}

enum Driver {
    Source(api::SourceViewport),
    /// Explicit navigation owns the preview before its next complete native geometry is available.
    /// A missing position suppresses source reflow without guessing a frame count or heading alignment.
    Preview(Option<api::PreviewViewport>),
}

#[derive(Clone, PartialEq)]
struct Intent {
    document: api::DocumentVersion,
    ui_revision: u64,
    target: api::ViewportTarget,
}

struct Pending {
    task: api::guest::EditorTask,
    intent: Intent,
}

/// One validated position supplies both a possible native location and whether translation may replace navigation.
struct Projection {
    intent: Option<Intent>,
    translated: bool,
    /// Switching manual sides retires an older queued location that could otherwise move the new driver.
    takeover: bool,
}

impl Scrolling {
    /// A new source cancels work instead of carrying old offsets into another document version.
    pub(super) fn reset(&mut self) {
        self.cancel_pending();
        self.driver = None;
    }

    /// Another revision of the same document cancels queued requests but keeps the manual driver.
    /// The side the user scrolled last stays authoritative; its next real translation moves the other.
    pub(super) fn invalidate(&mut self) {
        self.cancel_pending();
    }

    /// An explicit heading intent retires earlier automatic locations before native navigation runs.
    /// Its first actual preview position, including an EOF-clamped position, will drive only the source.
    pub(super) fn navigate(&mut self) {
        self.cancel_pending();
        self.driver = Some(Driver::Preview(None));
    }

    /// Rebuilding an unchanged source retires old requests while retaining the user's driving viewport.
    /// Startup locale/reflow can arrive before its reverse locate finishes; clearing that driver would
    /// let a source layout at zero undo the user's preview wheel. Source changes reset it explicitly.
    pub(super) fn refresh(&mut self, navigation: bool) {
        self.cancel_pending();
        if navigation && !matches!(self.driver, Some(Driver::Preview(_))) {
            self.driver = Some(Driver::Preview(None));
        } else if !navigation && matches!(self.driver, Some(Driver::Preview(None))) {
            // An unmeasured heading hold has no user anchor to retain after navigation relinquishes it.
            self.driver = None;
        }
    }

    /// Cancellation prevents queued host work where possible; already-applied Base offsets are not rolled back.
    fn cancel_pending(&mut self) {
        if let Some(pending) = self.pending.take() {
            let _ = pending.task.cancel(api::CancelMode::StopWaiting);
        }
        self.latest = None;
    }

    /// Source translation may become the driver; reflow on the following side preserves preview priority.
    /// Return true only for a validated, nonprogram source translation so navigation can retire its old intent.
    pub(super) fn source(
        &mut self,
        position: &api::SourceViewport,
        source: &Source,
        blocks: &[ui::Node],
        revision: u64,
    ) -> bool {
        let projection = project_source(&mut self.driver, position, source, blocks, revision);
        self.apply(projection)
    }

    /// Actual preview geometry identifies its deepest rendered block, including images and wrapped tables.
    /// Return true for nonprogram translation; layout can still establish a navigation's first preview anchor.
    pub(super) fn preview(
        &mut self,
        position: &api::PreviewViewport,
        source: &Source,
        blocks: &[ui::Node],
        revision: u64,
    ) -> bool {
        let projection = project_preview(&mut self.driver, position, source, blocks, revision);
        self.apply(projection)
    }

    /// Geometry decisions are pure; only this boundary turns a validated projection into a bounded host request.
    fn apply(&mut self, projection: Option<Projection>) -> bool {
        let Some(projection) = projection else {
            return false;
        };
        if projection.takeover {
            self.cancel_pending();
        }
        if let Some(intent) = projection.intent {
            self.queue(intent);
        }
        projection.translated
    }

    /// A single accepted request and one replaceable latest intent coalesce fast wheel/frame notifications.
    fn queue(&mut self, intent: Intent) {
        if let Some(pending) = &self.pending {
            coalesce(&pending.intent, &mut self.latest, intent);
            return;
        }
        let Some(origin) = self.origin.checked_add(1) else {
            return;
        };
        self.origin = origin;
        if let Ok(task) = api::guest::EditorTask::start(
            api::EditorOperation::LocateViewport {
                document: intent.document.clone(),
                panel: "preview".into(),
                ui_revision: intent.ui_revision,
                target: intent.target.clone(),
                origin,
            },
            2_000,
        ) {
            self.pending = Some(Pending { task, intent });
        }
    }

    /// Completion releases ownership; these readonly results never publish another UI tree or change its revision.
    pub(super) fn request(&mut self, event: &api::Notification) -> bool {
        let Some(update) = self
            .pending
            .as_mut()
            .and_then(|pending| pending.task.update(event))
        else {
            return false;
        };
        if update.is_terminal() {
            self.pending = None;
            if let Some(intent) = self.latest.take() {
                self.queue(intent);
            }
        }
        true
    }
}

/// Initial source measurement cannot override a navigation waiting for its first native preview geometry.
/// Later source reflow follows that measured preview, while genuine translation returns source ownership.
fn project_source(
    driver: &mut Option<Driver>,
    position: &api::SourceViewport,
    source: &Source,
    blocks: &[ui::Node],
    revision: u64,
) -> Option<Projection> {
    if position.origin.is_some()
        || position.document != source.version
        || position.ui_revision != revision
        || position.validate().is_err()
    {
        return None;
    }
    let takeover = !position.layout && matches!(driver, Some(Driver::Preview(_)));
    let intent =
        if let Some(Driver::Preview(previous)) = driver.as_ref().filter(|_| position.layout) {
            previous
                .as_ref()
                .and_then(|previous| preview_intent(previous, source, revision))
        } else {
            *driver = Some(Driver::Source(position.clone()));
            source_intent(position, source, blocks, revision)
        };
    Some(Projection {
        intent,
        translated: !position.layout,
        takeover,
    })
}

/// The preview can drive before or after a reveal receipt; its actual top block also handles EOF clamping.
/// Source ownership wins preview reflow only after a genuine source translation has replaced navigation.
fn project_preview(
    driver: &mut Option<Driver>,
    position: &api::PreviewViewport,
    source: &Source,
    blocks: &[ui::Node],
    revision: u64,
) -> Option<Projection> {
    if position.origin.is_some() || position.validate().is_err() {
        return None;
    }
    let takeover = !position.layout && matches!(driver, Some(Driver::Source(_)));
    let intent = if let Some(Driver::Source(previous)) = driver.as_ref().filter(|_| position.layout)
    {
        source_intent(previous, source, blocks, revision)
    } else {
        *driver = Some(Driver::Preview(Some(position.clone())));
        preview_intent(position, source, revision)
    };
    Some(Projection {
        intent,
        translated: !position.layout,
        takeover,
    })
}

/// One deferred intent must describe the latest manual position even when it returns to the in-flight one.
fn coalesce(pending: &Intent, latest: &mut Option<Intent>, intent: Intent) {
    *latest = (pending != &intent).then_some(intent);
}

/// The closest mapped block is the primary anchor; narrower descendants win ties over list/flow wrappers.
fn source_intent(
    position: &api::SourceViewport,
    source: &Source,
    blocks: &[ui::Node],
    revision: u64,
) -> Option<Intent> {
    let mut selected: Option<(String, ui::SourceRange, (usize, usize))> = None;
    for block in blocks {
        block.visit(&mut |node| {
            let Some(range) = node.source_range.filter(|range| {
                range.end > range.start && source.text.get(range.start..range.end).is_some()
            }) else {
                return;
            };
            let distance = if position.offset < range.start {
                range.start - position.offset
            } else {
                position.offset.saturating_sub(range.end.saturating_sub(1))
            };
            let score = (distance, range.end - range.start);
            if selected.as_ref().is_none_or(|(_, _, old)| score <= *old) {
                selected = Some((node.id.clone(), range, score));
            }
        });
    }
    let (node, range, _) = selected?;
    // Interpolate only within this semantic block. Native geometry supplies its current rendered height.
    let fraction = ((position.offset.saturating_sub(range.start) as f32 + position.line_fraction)
        / (range.end - range.start) as f32)
        .clamp(0.0, 1.0);
    Some(Intent {
        document: source.version.clone(),
        ui_revision: revision,
        target: api::ViewportTarget::Preview { node, fraction },
    })
}

/// Preview progression chooses a valid UTF-8 source caret inside its block, never an article-wide percentage.
fn preview_intent(
    position: &api::PreviewViewport,
    source: &Source,
    revision: u64,
) -> Option<Intent> {
    let range = position.source_range;
    let text = source.text.get(range.start..range.end)?;
    let mut relative =
        ((text.len() as f64 * position.fraction as f64).floor() as usize).min(text.len());
    while !text.is_char_boundary(relative) {
        relative = relative.saturating_sub(1);
    }
    Some(Intent {
        document: source.version.clone(),
        ui_revision: revision,
        target: api::ViewportTarget::Source {
            offset: range.start + relative,
            line_fraction: position.fraction,
        },
    })
}

#[cfg(test)]
mod tests;
