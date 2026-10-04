//! Markdown parsing is isolated in this guest; native layout and document authority stay in the host.

use plugin_protocol::{
    Environment, Snapshot, api,
    bindings::{Guest, export},
    ui,
};
use std::cell::RefCell;

mod format;
mod formatting;
mod imports;
mod preview;
mod tasks;
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
    /// Image imports preserve complete-file receipts independently of the current source snapshot.
    imports: imports::Imports,
    revision: u64,
}

thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

struct MarkdownPlugin;

impl Guest for MarkdownPlugin {
    /// Source snapshots arrive through Preview; image saves and text writes use correlated host editor tasks.
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
                        state.imports.superseded();
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
                    self.imports.source_changed();
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
            api::Notification::Ui(event) if panel == Some("preview") => self.ui_event(event),
            api::Notification::ImageInput {
                document,
                selection,
                images,
            } if panel == Some("preview") => {
                // Only a current native offer supersedes formatting; late offers cannot cancel a newer document's edit.
                if self
                    .source
                    .as_ref()
                    .is_some_and(|source| source.version == document)
                {
                    self.formatting.source_changed();
                }
                self.imports.start(
                    document,
                    selection,
                    images,
                    self.source.as_ref(),
                    self.environment.locale.starts_with("en"),
                );
                self.revision = self.revision.saturating_add(1);
            }
            event @ api::Notification::Request { .. } => {
                let english = self.environment.locale.starts_with("en");
                let formatting_changed =
                    self.formatting
                        .request(&event, self.source.as_ref(), english);
                // Both owners inspect correlation; short-circuiting would strand an import's file receipt.
                let imports_changed = self.imports.request(&event, self.source.as_ref(), english);
                if formatting_changed || imports_changed {
                    self.revision = self.revision.saturating_add(1);
                }
            }
            // Only parsed tasks and declared toolbar actions can edit; other preview content remains readonly.
            _ => {}
        }
    }

    /// File-scoped actions bind to the current UI revision before resolving a parsed task or toolbar intent.
    /// A valid new text intent stops later image insertion while accepted image saves retain their receipts.
    fn ui_event(&mut self, event: ui::UiEvent) {
        if event.revision != self.revision {
            return;
        }
        let changed = match event.action {
            ui::Action::Click => {
                let Some(command) = toolbar::command(&event.node) else {
                    return;
                };
                let imports_changed = self.imports.superseded();
                self.formatting.start(command, self.source.as_ref()) || imports_changed
            }
            ui::Action::Toggle(checked) => {
                let Some(source) = self.source.as_ref() else {
                    return;
                };
                let Some(change) = tasks::change(&self.blocks, &source.text, &event.node, checked)
                else {
                    return;
                };
                let imports_changed = self.imports.superseded();
                self.formatting.start_task(change, Some(source)) || imports_changed
            }
            _ => false,
        };
        // Acceptance alone keeps the revision stable so a newer fast action can replace the pending intent.
        if changed {
            self.revision = self.revision.saturating_add(1);
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
        document.editor_image_input = self.source.is_some();
        if self.source.is_some() {
            let english = self.environment.locale.starts_with("en");
            let import_message = self.imports.message(self.source.as_ref(), english);
            // A format failure and an external-file receipt are independent outcomes; show both when needed.
            let messages = [self.formatting.message(english), import_message.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n");
            document.editor_toolbar = Some(toolbar::node(
                english,
                (!messages.is_empty()).then_some(messages.as_str()),
            ));
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
