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
    Preview(api::PreviewViewport),
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

impl Scrolling {
    /// A new source or UI projection cancels work instead of carrying old offsets into another scene.
    pub(super) fn reset(&mut self) {
        if let Some(pending) = self.pending.take() {
            let _ = pending.task.cancel(api::CancelMode::StopWaiting);
        }
        self.latest = None;
        self.driver = None;
    }

    /// Only manual source positions become drivers; reflow on the following side preserves its last driver.
    pub(super) fn source(
        &mut self,
        position: &api::SourceViewport,
        source: &Source,
        blocks: &[ui::Node],
        revision: u64,
    ) {
        if position.origin.is_some()
            || position.document != source.version
            || position.ui_revision != revision
            || position.validate().is_err()
        {
            return;
        }
        let intent = if position.layout && matches!(self.driver, Some(Driver::Preview(_))) {
            let Some(Driver::Preview(previous)) = &self.driver else {
                unreachable!()
            };
            preview_intent(previous, source, revision)
        } else {
            self.driver = Some(Driver::Source(position.clone()));
            source_intent(position, source, blocks, revision)
        };
        if let Some(intent) = intent {
            self.queue(intent);
        }
    }

    /// Actual preview geometry identifies its deepest rendered block, including images and wrapped tables.
    pub(super) fn preview(
        &mut self,
        position: &api::PreviewViewport,
        source: &Source,
        blocks: &[ui::Node],
        revision: u64,
    ) {
        if position.origin.is_some() || position.validate().is_err() {
            return;
        }
        let intent = if position.layout && matches!(self.driver, Some(Driver::Source(_))) {
            let Some(Driver::Source(previous)) = &self.driver else {
                unreachable!()
            };
            source_intent(previous, source, blocks, revision)
        } else {
            self.driver = Some(Driver::Preview(position.clone()));
            preview_intent(position, source, revision)
        };
        if let Some(intent) = intent {
            self.queue(intent);
        }
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
mod tests {
    use super::*;

    /// Returning to A while A is pending must discard B rather than scroll to it after A completes.
    #[test]
    fn scrolling_latest_manual_position_cancels_an_obsolete_deferred_intent() {
        let first = Intent {
            document: api::DocumentVersion {
                id: "native".into(),
                path: "notes.md".into(),
                revision: 7,
            },
            ui_revision: 4,
            target: api::ViewportTarget::Preview {
                node: "a".into(),
                fraction: 0.0,
            },
        };
        let mut second = first.clone();
        second.target = api::ViewportTarget::Preview {
            node: "b".into(),
            fraction: 0.5,
        };
        let mut latest = None;
        coalesce(&first, &mut latest, second.clone());
        assert!(latest.as_ref() == Some(&second));
        coalesce(&first, &mut latest, first.clone());
        assert!(latest.is_none(), "A→B→A retains A, not the obsolete B");
    }

    /// Unicode endpoints and blank gaps remain tied to the closest semantic block.
    #[test]
    fn scrolling_maps_blocks_and_unicode_boundaries_without_document_percentages() {
        let source = Source {
            version: api::DocumentVersion {
                id: "native".into(),
                path: "notes.md".into(),
                revision: 7,
            },
            text: "前段\n\n目标中文段落\n\n末段\n".into(),
        };
        let start = source.text.find("目标").unwrap();
        let end = source.text.find("\n\n末段").unwrap() + 1;
        let range = ui::SourceRange { start, end };
        let blocks = vec![ui::Node::text("target", "目标中文段落").source_range(start..end)];
        let anchor = api::SourceViewport {
            document: source.version.clone(),
            ui_revision: 4,
            offset: start,
            line_fraction: 0.0,
            origin: None,
            layout: false,
        };
        assert!(
            matches!(source_intent(&anchor, &source, &blocks, 4).unwrap().target,
            api::ViewportTarget::Preview { node, fraction } if node == "target" && fraction == 0.0)
        );
        for fraction in [0.0, 0.13, 0.5, 0.87, 1.0] {
            let preview = api::PreviewViewport {
                block: "target".into(),
                source_range: range,
                fraction,
                origin: None,
                layout: false,
            };
            let api::ViewportTarget::Source { offset, .. } =
                preview_intent(&preview, &source, 4).unwrap().target
            else {
                panic!("source target")
            };
            assert!((start..=end).contains(&offset) && source.text.is_char_boundary(offset));
        }
    }
}
