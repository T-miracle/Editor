//! Independent whole-file layout example, borrowing native text without retaining an editable copy.
use plugin_protocol::{
    api,
    bindings::{Guest, export},
    ui,
};
use std::cell::RefCell;

/// Only view intent and immutable capability tokens are retained; the host owns native text/Undo.
#[derive(Default)]
struct State {
    source: Option<api::DocumentVersion>,
    file: Option<api::FileContext>,
    revision: u64,
    mode: u8,
    locale: String,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
struct Example;

impl Guest for Example {
    /// Public SDK dispatch provides correlation and typed failure encoding for each lifecycle call.
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |input| {
            STATE.with(|state| state.borrow_mut().handle(input))
        })
    }
}
export!(Example);

impl State {
    /// File/source authority arrives from the selected host context; other lifecycle events add none.
    fn handle(&mut self, input: api::Input) -> Result<api::Output, api::Failure> {
        match input {
            api::Input::Prepare {
                snapshot,
                environment,
                ..
            } => {
                self.locale = environment.locale;
                if let Some(snapshot) = snapshot {
                    if snapshot.schema != 1 {
                        return Err(api::Failure::new(
                            api::ErrorCode::InvalidRequest,
                            "Unsupported layout snapshot",
                        ));
                    }
                    self.mode = serde_json::from_str::<u8>(&snapshot.data).map_err(|error| {
                        api::Failure::new(api::ErrorCode::InvalidRequest, error.to_string())
                    })?;
                    if self.mode > 2 {
                        return Err(api::Failure::new(
                            api::ErrorCode::InvalidRequest,
                            "Unknown layout choice",
                        ));
                    }
                }
                return Ok(api::Output::default());
            }
            api::Input::Snapshot => {
                return Ok(api::Output {
                    snapshot: Some(plugin_protocol::Snapshot {
                        schema: 1,
                        data: self.mode.to_string(),
                    }),
                    ..Default::default()
                });
            }
            api::Input::Event {
                event: api::Notification::Preview { document, .. },
                ..
            } => {
                self.source = document;
                self.file = None;
            }
            api::Input::Event {
                event: api::Notification::FilePreview { file },
                ..
            } => {
                self.source = None;
                self.file = file;
            }
            api::Input::Event {
                event: api::Notification::Ui(event),
                ..
            } => {
                if matches!(event.action, ui::Action::Click) {
                    self.mode = match event.node.as_str() {
                        "layout-row" => 0,
                        "layout-column" => 1,
                        "layout-content" => 2,
                        _ => self.mode,
                    };
                }
            }
            _ => {}
        }
        self.revision = self.revision.saturating_add(1);
        let mut controls = Vec::new();
        let chinese = self.locale.starts_with("zh");
        for (id, label) in [
            (
                "layout-row",
                if chinese {
                    "并排显示"
                } else {
                    "Side by side"
                },
            ),
            (
                "layout-column",
                if chinese { "上下显示" } else { "Stacked" },
            ),
            (
                "layout-content",
                if chinese {
                    "仅插件内容"
                } else {
                    "Content only"
                },
            ),
        ] {
            controls.push(ui::Node::button(id, label));
        }
        let mut children = Vec::new();
        if self.mode != 2
            && let Some(source) = &self.source
        {
            children.push(
                ui::Node::new(
                    "borrowed-editor",
                    ui::Kind::NativeEditor {
                        document: source.clone(),
                    },
                )
                .grow(),
            );
        }
        children.push(if self.file.is_some() {
            ui::Node::new(
                "file-image",
                ui::Kind::FileImage {
                    alt: "Image".into(),
                    sizing: ui::ImageSizing::OriginalContain,
                },
            )
            .grow()
        } else {
            ui::Node::text(
                "plugin-content",
                if chinese {
                    "插件布局内容"
                } else {
                    "Plugin layout content"
                },
            )
            .grow()
        });
        let layout = if self.mode == 1 {
            ui::Node::column("work-area", children).grow()
        } else {
            ui::Node::row("work-area", children).grow()
        };
        let mut document = ui::Document::new(
            ui::Node::column(
                "file-layout",
                vec![
                    ui::Node::row("layout-controls", controls).height(32.),
                    layout,
                ],
            )
            .grow(),
        )
        .revision(self.revision);
        document.editor_layout = self.source.is_some() || self.file.is_some();
        document.source = self.source.clone();
        document.file = self.file.as_ref().map(|file| file.version.clone());
        Ok(api::Output {
            views: vec![api::View {
                panel: "layout".into(),
                document,
            }],
            ..Default::default()
        })
    }
}
