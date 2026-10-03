//! Standalone SDK example: all layout is native, and all event handling lives in lib.rs.
use super::*;
use ui::{Dialog, Document, Input, Kind, Node, OptionItem, Tab};

impl State {
    /// Each declared panel owns a native tree, with no legacy drawing envelope.
    pub(super) fn view(&self, panel: &str) -> api::View {
        let root = if panel == "counter" {
            Node::column(
                "counter-root",
                vec![
                    Node::text("count", format!("点击次数：{}", self.count)).role("body"),
                    Node::row(
                        "actions",
                        vec![
                            Node::button("increment", "增加一次"),
                            Node::button("open-dialog", "打开弹窗"),
                        ],
                    )
                    .gap(8.),
                    Node::new(
                        "pages",
                        Kind::Tabs {
                            selected: if self.tab.is_empty() {
                                "form".into()
                            } else {
                                self.tab.clone()
                            },
                            tabs: vec![
                                Tab::new(
                                    "form",
                                    "表单",
                                    Node::column(
                                        "form-body",
                                        vec![
                                            Node::checkbox("enabled", "启用示例选项", self.checked),
                                            Node::new(
                                                "mode",
                                                Kind::Choice {
                                                    options: vec![
                                                        OptionItem::new("fast", "快速"),
                                                        OptionItem::new("full", "完整"),
                                                    ],
                                                    selected: self.selected.clone(),
                                                },
                                            ),
                                            Node::new(
                                                "progress",
                                                Kind::Progress {
                                                    label: "示例进度".into(),
                                                    value: (self.count % 101) as f32,
                                                },
                                            ),
                                        ],
                                    )
                                    .gap(12.),
                                ),
                                Tab::new(
                                    "data",
                                    "数据",
                                    Node::column(
                                        "data-body",
                                        vec![
                                            Node::new(
                                                "list",
                                                Kind::List {
                                                    items: vec![
                                                        "原生布局".into(),
                                                        "主题继承".into(),
                                                    ],
                                                },
                                            ),
                                            Node::new("divider", Kind::Separator),
                                            Node::new(
                                                "table",
                                                Kind::Table {
                                                    headers: vec!["项目".into(), "状态".into()],
                                                    rows: vec![vec![
                                                        "组件协议".into(),
                                                        "可用".into(),
                                                    ]],
                                                },
                                            ),
                                            Node::new("space", Kind::Spacer).height(8.),
                                        ],
                                    )
                                    .gap(8.),
                                ),
                            ],
                        },
                    ),
                ],
            )
            .gap(12.)
            .padding(12.)
        } else {
            Node::column(
                "notes-root",
                vec![
                    Node::text("notes-title", "笔记（支持中文输入法）"),
                    Node::input(
                        "note",
                        Input {
                            value: self.note.clone(),
                            placeholder: "输入笔记".into(),
                            ..Default::default()
                        },
                    ),
                    Node::text("notes-preview", self.note.clone()),
                ],
            )
            .padding(12.)
            .gap(8.)
        };
        let mut document =
            Document::new(Node::scroll("viewport", root).height(300.)).revision(self.revision);
        if panel == "counter" && self.editing {
            document = document.dialog(Dialog::new(
                "sample-dialog",
                "插件原生弹窗",
                Node::column(
                    "dialog-body",
                    vec![
                        Node::text("dialog-message", "窗口、焦点和主题由主程序管理。"),
                        Node::button("close-dialog", "关闭"),
                    ],
                )
                .gap(12.),
            ));
        }
        api::View {
            panel: panel.into(),
            document,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_example_views_satisfy_the_exported_contract() {
        let mut state = State::default();
        for tab in ["form", "data"] {
            state.tab = tab.into();
            state.editing = true;
            for panel in ["counter", "notes"] {
                state.view(panel).document.validate().unwrap();
            }
        }
        state.event(
            Some("counter"),
            api::Notification::Ui(ui::UiEvent {
                revision: 0,
                node: "increment".into(),
                action: ui::Action::Click,
            }),
        );
        assert_eq!(state.count, 1);
    }
}
