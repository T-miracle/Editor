//! Independent whole-file layout example, borrowing native text without retaining an editable copy.
use plugin_protocol::{
    api,
    bindings::{Guest, export},
    ui,
};
use std::cell::RefCell;
mod intent;

/// Only view intent and immutable capability tokens are retained; the host owns native text/Undo.
#[derive(Default)]
struct State {
    source: Option<api::DocumentVersion>,
    file: Option<api::FileContext>,
    revision: u64,
    mode: u8,
    locale: String,
    intent: intent::Intent,
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
                event: api::Notification::Command { id, .. },
                ..
            } if id == "diagnostic-trap" => {
                // An explicit SDK diagnostic exercises real fault retirement without a host fixture branch.
                panic!("Independent layout diagnostic trap");
            }
            api::Input::Event {
                event: api::Notification::Preview { document, .. },
                ..
            } => {
                self.source = document;
                if let Some(file) = &mut self.file {
                    file.text = self.source.clone();
                }
            }
            api::Input::Event {
                event: api::Notification::FilePreview { file },
                ..
            } => {
                self.source = file.as_ref().and_then(|file| file.text.clone());
                if let Some(mode) = self.intent.bind(file.as_ref())? {
                    self.mode = mode;
                }
                self.file = file;
            }
            api::Input::Event {
                event: api::Notification::Ui(event),
                ..
            } => {
                if matches!(event.action, ui::Action::Click) {
                    self.select(&event.node)?;
                }
            }
            api::Input::Event {
                event: api::Notification::Tool(event),
                ..
            } => self.select(&event.tool)?,
            api::Input::Event {
                event:
                    api::Notification::PreferenceChanged {
                        subscription,
                        key,
                        value,
                    },
                ..
            } => {
                if let Some(mode) = self.intent.changed(&subscription, &key, &value)? {
                    self.mode = mode;
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
        children.push(
            if self.file.as_ref().is_some_and(|file| file.text.is_none()) {
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
            },
        );
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
        if let Some(file) = &self.file {
            for (index, (id, zh, en, icon)) in [
                ("row", "并排显示", "Side by side", "row"),
                ("column", "上下显示", "Stacked", "column"),
                ("content", "仅插件内容", "Content only", "content"),
            ]
            .into_iter()
            .enumerate()
            {
                let label = ui::LocalizedText {
                    zh_cn: zh.into(),
                    en: en.into(),
                };
                document.tools.push(ui::ToolButton {
                    id: format!("tool-{id}"),
                    label: label.clone(),
                    tooltip: label,
                    icon: ui::ToolIcon {
                        light: format!("icons/{icon}.svg"),
                        dark: format!("icons/{icon}.svg"),
                    },
                    target: ui::ToolTarget::File {
                        version: file.version.clone(),
                    },
                    visible: true,
                    selected: self.mode == index as u8,
                    disabled: false,
                    order: index as i32,
                });
            }
        }
        Ok(api::Output {
            views: vec![api::View {
                panel: "layout".into(),
                document,
            }],
            ..Default::default()
        })
    }

    /// Both native content buttons and bottom-bar contributions invoke this guest-owned display choice.
    fn select(&mut self, id: &str) -> Result<(), api::Failure> {
        let selected = match id {
            "layout-row" | "tool-row" => 0,
            "layout-column" | "tool-column" => 1,
            "layout-content" | "tool-content" => 2,
            _ => return Ok(()),
        };
        self.mode = self.intent.select(selected)?;
        Ok(())
    }
}
