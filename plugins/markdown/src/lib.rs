//! Markdown parsing is isolated in this guest; native layout and document authority stay in the host.

use plugin_protocol::{
    Environment, Snapshot, api,
    bindings::{Guest, export},
    ui,
};
use std::cell::RefCell;

mod preview;

/// A host-supplied source snapshot is replaced atomically and is never edited or persisted here.
struct Source {
    version: api::DocumentVersion,
    text: String,
}

/// Only derived preview state is mutable; the editor owns IME, selections and undo history.
#[derive(Default)]
struct State {
    environment: Environment,
    source: Option<Source>,
    blocks: Vec<ui::Node>,
    revision: u64,
}

thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

struct MarkdownPreview;

impl Guest for MarkdownPreview {
    /// The public lifecycle delivers unsaved source without filesystem, clipboard or editor writes.
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |message| {
            STATE.with(|cell| {
                let mut state = cell.borrow_mut();
                match message {
                    api::Input::Prepare {
                        environment,
                        snapshot,
                        ..
                    } => {
                        if snapshot.is_some_and(|snapshot| snapshot.schema != 1) {
                            return Err(api::Failure::new(
                                api::ErrorCode::InvalidRequest,
                                "Unsupported Markdown preview snapshot",
                            ));
                        }
                        *state = State {
                            environment,
                            ..Default::default()
                        };
                    }
                    api::Input::Event { panel, event } => state.event(panel.as_deref(), event),
                    api::Input::Snapshot => {
                        // Source text and derived trees are transient and cannot revive a closed document.
                        return Ok(api::Output {
                            snapshot: Some(Snapshot {
                                schema: 1,
                                data: "{}".into(),
                            }),
                            ..Default::default()
                        });
                    }
                    api::Input::Activate => {}
                }
                Ok(api::Output {
                    views: vec![state.view()],
                    ..Default::default()
                })
            })
        })
    }
}

export!(MarkdownPreview);

impl State {
    /// Only the file-scoped preview notification may replace this readonly source snapshot.
    fn event(&mut self, panel: Option<&str>, event: api::Notification) {
        match event {
            api::Notification::Preview { document, text } if panel == Some("preview") => {
                if matches!((&self.source, &document), (Some(current), Some(next))
                    if current.version.id == next.id && next.revision < current.version.revision)
                {
                    return;
                }
                self.source = document.map(|version| Source { version, text });
                self.refresh();
            }
            api::Notification::Theme(environment) => {
                let locale_changed = self.environment.locale != environment.locale;
                self.environment = environment;
                if locale_changed {
                    self.refresh();
                }
            }
            // Preview interactions are readonly in this stage; future writes need revision-checked APIs.
            _ => {}
        }
    }

    /// Rebuild only derived nodes; offsets remain UTF-8 bytes in exactly the echoed source version.
    fn refresh(&mut self) {
        self.blocks = self.source.as_ref().map_or_else(Vec::new, |source| {
            preview::blocks(&source.text, &self.environment.locale)
        });
        self.revision = self.revision.saturating_add(1);
    }

    /// Stable container IDs retain native scroll state while every content block carries its source range.
    fn view(&self) -> api::View {
        let body = ui::Node::column("preview-body", self.blocks.clone())
            .padding(12.)
            .gap(8.);
        let mut document = ui::Document::new(ui::Node::scroll("preview-scroll", body).grow())
            .revision(self.revision);
        document.source = self.source.as_ref().map(|source| source.version.clone());
        // A large or deeply nested document should leave the guest alive and preserve its source authority.
        // The same public quotas apply to this preview and every other native plugin view.
        if document.validate().is_err() {
            let message = if self.environment.locale.starts_with("en") {
                "This document exceeds the native preview limits."
            } else {
                "此文档超出原生预览限制。"
            };
            document.root = ui::Node::scroll(
                "preview-scroll",
                ui::Node::column(
                    "preview-body",
                    vec![ui::Node::text("preview-limit", message)],
                )
                .padding(12.),
            )
            .grow();
        }
        api::View {
            panel: "preview".into(),
            document,
        }
    }
}
