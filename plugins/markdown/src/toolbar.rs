//! Stable toolbar identities and localized labels describe Markdown intents through public native nodes.

use super::format::Command;
use plugin_protocol::ui;

/// One definition supplies both routing and presentation, so changing a button cannot drift its command ID.
struct Tool {
    command: Command,
    id: &'static str,
    labels: (&'static str, &'static str),
    tips: (&'static str, &'static str),
}

const GROUPS: &[(&str, &[Tool])] = &[
    (
        "format-headings",
        &[Tool {
            command: Command::Heading,
            id: "format-heading",
            labels: ("H", "H"),
            tips: ("一级标题", "Heading 1"),
        }],
    ),
    (
        "format-emphasis",
        &[
            Tool {
                command: Command::Bold,
                id: "format-bold",
                labels: ("B", "B"),
                tips: ("粗体", "Bold"),
            },
            Tool {
                command: Command::Italic,
                id: "format-italic",
                labels: ("I", "I"),
                tips: ("斜体", "Italic"),
            },
            Tool {
                command: Command::Strike,
                id: "format-strike",
                labels: ("S", "S"),
                tips: ("删除线", "Strikethrough"),
            },
        ],
    ),
    (
        "format-code",
        &[
            Tool {
                command: Command::InlineCode,
                id: "format-inline-code",
                labels: ("`", "`"),
                tips: ("行内代码", "Inline code"),
            },
            Tool {
                command: Command::CodeBlock,
                id: "format-code-block",
                labels: ("```", "```"),
                tips: ("代码块", "Code block"),
            },
        ],
    ),
    (
        "format-blocks",
        &[
            Tool {
                command: Command::Quote,
                id: "format-quote",
                labels: (">", ">"),
                tips: ("引用", "Block quote"),
            },
            Tool {
                command: Command::Unordered,
                id: "format-unordered",
                labels: ("•", "•"),
                tips: ("无序列表", "Unordered list"),
            },
            Tool {
                command: Command::Ordered,
                id: "format-ordered",
                labels: ("1.", "1."),
                tips: ("有序列表", "Ordered list"),
            },
            Tool {
                command: Command::Task,
                id: "format-task",
                labels: ("☑", "☑"),
                tips: ("任务列表", "Task list"),
            },
        ],
    ),
    (
        "format-inserts",
        &[
            Tool {
                command: Command::Link,
                id: "format-link",
                labels: ("链接", "Link"),
                tips: ("链接", "Link"),
            },
            Tool {
                command: Command::Image,
                id: "format-image",
                labels: ("图片", "Image"),
                tips: ("图片引用模板", "Image reference template"),
            },
            Tool {
                command: Command::Table,
                id: "format-table",
                labels: ("表格", "Table"),
                tips: ("两列表格", "Two-column table"),
            },
        ],
    ),
];

/// Only declared toolbar nodes route to formatting; preview text and readonly tasks have no edit action.
pub(super) fn command(node: &str) -> Option<Command> {
    GROUPS
        .iter()
        .flat_map(|(_, tools)| tools.iter())
        .find(|tool| tool.id == node)
        .map(|tool| tool.command)
}

/// Flexible rows let the host retain every button at narrow source widths and choose the native height.
/// Localized feedback stays source-bound; its short scroll viewport keeps long file receipts from hiding the editor.
pub(super) fn node(english: bool, error: Option<&str>) -> ui::Node {
    let groups = GROUPS
        .iter()
        .map(|(id, tools)| {
            let buttons = tools
                .iter()
                .map(|tool| {
                    let label = if english {
                        tool.labels.1
                    } else {
                        tool.labels.0
                    };
                    let tip = if english { tool.tips.1 } else { tool.tips.0 };
                    ui::Node::button(tool.id, label).tooltip(tip)
                })
                .collect();
            ui::Node::row(*id, buttons).wrap().gap(4.)
        })
        .collect();
    let mut children = vec![ui::Node::row("format-actions", groups).wrap().gap(12.)];
    if let Some(error) = error {
        // Buttons keep their natural wrapped height. Only feedback scrolls, so every saved filename remains readable.
        children.push(
            ui::Node::scroll("format-feedback", ui::Node::text("format-error", error)).height(56.),
        );
    }
    ui::Node::column("format-toolbar", children)
        .padding(4.)
        .gap(4.)
}
