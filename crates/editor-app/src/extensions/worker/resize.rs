//! Replace queued absolute canvas geometry without reordering input or lifecycle boundaries.
use super::*;

/// Keep the latest consecutive resize for one node/revision/incarnation before executing WASM.
/// The first nonmatching message is retained for the next actor turn, including shutdown/revocation.
pub(super) fn coalesce(
    mut work: Work,
    rx: &mpsc::Receiver<Work>,
    next_received: &mut Option<Work>,
) -> Work {
    if resize_target(&work).is_none() {
        return work;
    }
    // Bound channel draining even when a producer continuously emits geometry changes.
    for _ in 0..1024 {
        let Some(next) = next_received.take().or_else(|| rx.try_recv().ok()) else {
            break;
        };
        if resize_target(&work) == resize_target(&next) {
            work = next;
        } else {
            *next_received = Some(next);
            break;
        }
    }
    work
}

/// Only absolute Resize events can replace each other; deltas and release gestures are never dropped.
fn resize_target(work: &Work) -> Option<(&str, u64, &Option<String>, u64, &str)> {
    match work {
        Work::Event(plugin, epoch, panel, api::Notification::Ui(event))
            if matches!(
                event.action,
                ui::Action::Canvas(ui::CanvasEvent::Resize { .. })
                    | ui::Action::ViewportInput(ui::ViewportInput {
                        event: ui::CanvasEvent::Resize { .. },
                        ..
                    })
            ) =>
        {
            Some((plugin, *epoch, panel, event.revision, &event.node))
        }
        _ => None,
    }
}
