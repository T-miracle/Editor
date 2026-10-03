//! Markdown parsing is isolated in this guest; native layout and document authority stay in the host.

use plugin_protocol::{
    Environment, Snapshot, api,
    bindings::{Guest, export},
    ui,
};
use std::cell::RefCell;

mod format;
mod formatting;
mod preview;
mod toolbar;

/// A host-supplied source snapshot is replaced atomically and is never edited or persisted here.
struct Source {
    version: api::DocumentVersion,
    text: String,
}

/// Only derived view state and task ownership are mutable; the editor owns IME, selection and undo.
#[derive(Default)]
struct State {
    environment: Environment,
    source: Option<Source>,
    blocks: Vec<ui::Node>,
    /// The pending intent stores request ownership only; source text remains a readonly host snapshot.
    formatting: formatting::Formatting,
    revision: u64,
}

thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

struct MarkdownPlugin;

impl Guest for MarkdownPlugin {
    /// Source snapshots arrive through Preview; formatting writes use separate version-checked editor tasks.
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
                        state.formatting.source_changed();
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

export!(MarkdownPlugin);

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
                let changed = match (&self.source, &document) {
                    (Some(current), Some(next)) => current.version != *next || current.text != text,
                    (None, None) => false,
                    _ => true,
                };
                if changed {
                    self.formatting.source_changed();
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
            api::Notification::Ui(event) if panel == Some("preview") => {
                if event.revision == self.revision && event.action == ui::Action::Click {
                    if let Some(command) = toolbar::command(&event.node) {
                        // Keeping the same revision for acceptance lets a later fast click replace this intent.
                        if self.formatting.start(command, self.source.as_ref()) {
                            self.revision = self.revision.saturating_add(1);
                        }
                    }
                }
            }
            event @ api::Notification::Request { .. } => {
                if self.formatting.request(
                    &event,
                    self.source.as_ref(),
                    self.environment.locale.starts_with("en"),
                ) {
                    self.revision = self.revision.saturating_add(1);
                }
            }
            // Other preview interactions remain readonly until separately scoped capabilities are implemented.
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
        if self.source.is_some() {
            let english = self.environment.locale.starts_with("en");
            document.editor_toolbar =
                Some(toolbar::node(english, self.formatting.message(english)));
        }
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
