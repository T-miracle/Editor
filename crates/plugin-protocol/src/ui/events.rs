//! Shared event validation keeps deferred native callbacks and worker queues on the same view version.
use super::*;
use crate::api::{ErrorCode, Failure};

impl Document {
    pub fn validate_event(&self, event: &UiEvent) -> Result<(), Failure> {
        if event.revision != self.revision {
            return Err(Failure::new(
                ErrorCode::StaleRevision,
                "UI document was replaced",
            ));
        }
        if matches!(event.action, Action::Dismiss)
            && self
                .dialog
                .as_ref()
                .is_some_and(|dialog| dialog.id == event.node)
        {
            return Ok(());
        }
        let node = self.active_node(&event.node).ok_or_else(|| {
            Failure::new(
                ErrorCode::InvalidHandle,
                "UI node is absent, inactive or disabled",
            )
        })?;
        let valid = match (&node.kind, &event.action) {
            (Kind::Button { .. }, Action::Click) | (Kind::Checkbox { .. }, Action::Toggle(_)) => {
                true
            }
            (Kind::Input(_), Action::Change(value) | Action::Submit(value)) => value.len() <= 65536,
            (Kind::Choice { options, .. }, Action::Select(id)) => options
                .iter()
                .any(|option| option.id == *id && !option.disabled),
            (Kind::Tabs { tabs, .. }, Action::Select(id)) => tabs.iter().any(|tab| tab.id == *id),
            (Kind::Canvas(canvas), Action::Canvas(event)) => canvas.accepts(event),
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Event does not match this UI node or exceeds its limits",
            ))
        }
    }
}

impl Canvas {
    /// Host-created measurements remain bounded too, avoiding NaN/Infinity entering a guest layout loop.
    fn accepts(&self, event: &CanvasEvent) -> bool {
        let finite = |value: f32| value.is_finite() && value.abs() <= 1_000_000.;
        match event {
            CanvasEvent::Resize {
                width,
                height,
                grid,
            } => {
                finite(*width)
                    && finite(*height)
                    && *width >= 0.
                    && *height >= 0.
                    && grid.is_some() == self.grid
                    && grid.as_ref().is_none_or(|grid| {
                        finite(grid.cell_width)
                            && finite(grid.cell_height)
                            && grid.cell_width > 0.
                            && grid.cell_height > 0.
                    })
            }
            CanvasEvent::Focus { .. } => self.focusable,
            CanvasEvent::Key { key, .. } => self.focusable && key.len() <= 128,
            CanvasEvent::Text { text } => self.focusable && text.len() <= 65536,
            CanvasEvent::Pointer { x, y, button, .. } => finite(*x) && finite(*y) && *button <= 2,
            CanvasEvent::Wheel {
                x,
                y,
                delta_x,
                delta_y,
                ..
            } => [*x, *y, *delta_x, *delta_y].into_iter().all(finite),
        }
    }
}
