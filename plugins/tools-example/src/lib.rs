//! One independent guest supplies window controls and auxiliary functions without host business code.
use plugin_protocol::{
    api,
    bindings::{Guest, export},
    ui,
};
use std::cell::RefCell;

const WINDOWS: [&str; 5] = ["window-a", "window-b", "window-c", "window-d", "window-e"];

#[derive(Default)]
struct State {
    file: Option<api::FileContext>,
    revision: u64,
    file_count: u64,
    window_counts: [u64; 5],
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
struct Example;
impl Guest for Example {
    /// Lifecycle and event correlation are encoded by the public SDK.
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |input| {
            STATE.with(|state| state.borrow_mut().handle(input))
        })
    }
}
export!(Example);

impl State {
    /// Runtime-authorized events change only this guest's counters; no native document is mutated.
    fn handle(&mut self, input: api::Input) -> Result<api::Output, api::Failure> {
        match input {
            api::Input::Prepare { snapshot, .. } => {
                // Restored counters contain no document, window or subscription authority.
                if let Some(snapshot) = snapshot {
                    if snapshot.schema != 1 {
                        return Err(api::Failure::new(
                            api::ErrorCode::InvalidRequest,
                            "Unknown tools snapshot",
                        ));
                    }
                    (self.file_count, self.window_counts) = serde_json::from_str(&snapshot.data)
                        .map_err(|error| {
                            api::Failure::new(api::ErrorCode::InvalidRequest, error.to_string())
                        })?;
                }
                return Ok(api::Output::default());
            }
            api::Input::Snapshot => {
                return Ok(api::Output {
                    snapshot: Some(plugin_protocol::Snapshot {
                        schema: 1,
                        data: serde_json::to_string(&(self.file_count, self.window_counts))
                            .unwrap(),
                    }),
                    ..Default::default()
                });
            }
            api::Input::Event {
                event: api::Notification::FilePreview { file },
                ..
            } => self.file = file,
            api::Input::Event {
                panel,
                event: api::Notification::Tool(event),
            } => match event.target {
                ui::ToolTarget::File { .. } => self.file_count += 1,
                ui::ToolTarget::Window { panel: target } => {
                    if panel.as_deref() == Some(&target)
                        && let Some(index) = WINDOWS.iter().position(|id| *id == target)
                    {
                        self.window_counts[index] += 1;
                    }
                }
            },
            _ => {}
        }
        self.revision += 1;
        let mut auxiliary = ui::Document::new(ui::Node::text(
            "aux-count",
            format!("File count: {}", self.file_count),
        ))
        .revision(self.revision);
        auxiliary.source = self.file.as_ref().and_then(|file| file.text.clone());
        auxiliary.file = self.file.as_ref().map(|file| file.version.clone());
        if let Some(file) = &self.file {
            for index in 0..8 {
                auxiliary.tools.push(tool(
                    format!("aux-{index}"),
                    format!("辅助工具 {index}"),
                    format!("Auxiliary tool {index}"),
                    ui::ToolTarget::File {
                        version: file.version.clone(),
                    },
                    10 + index,
                    self.file_count % 2 == 1,
                    index == 2,
                    index != 7,
                ));
            }
        }
        let mut views = vec![api::View {
            panel: "auxiliary".into(),
            document: auxiliary,
        }];
        for (index, panel) in WINDOWS.into_iter().enumerate() {
            let mut document = ui::Document::new(ui::Node::column(
                "window-content",
                vec![
                    ui::Node::text(
                        "window-count",
                        format!("Window count: {}", self.window_counts[index]),
                    ),
                    ui::Node::button("window-focus", "Focus this window"),
                ],
            ))
            .revision(self.revision);
            document.tools.push(tool(
                "increment".into(),
                "增加窗口计数".into(),
                "Increment window count".into(),
                ui::ToolTarget::Window {
                    panel: panel.into(),
                },
                0,
                false,
                false,
                true,
            ));
            views.push(api::View {
                panel: panel.into(),
                document,
            });
        }
        Ok(api::Output {
            views,
            ..Default::default()
        })
    }
}

/// Both dark/light themes use owned, tintable geometric artwork; semantic state remains guest data.
fn tool(
    id: String,
    zh_cn: String,
    en: String,
    target: ui::ToolTarget,
    order: i32,
    selected: bool,
    disabled: bool,
    visible: bool,
) -> ui::ToolButton {
    let label = ui::LocalizedText { zh_cn, en };
    ui::ToolButton {
        id,
        label: label.clone(),
        tooltip: label,
        icon: ui::ToolIcon {
            light: "icons/tool.svg".into(),
            dark: "icons/tool.svg".into(),
        },
        target,
        order,
        selected,
        disabled,
        visible,
    }
}
