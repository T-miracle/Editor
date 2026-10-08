//! Stable toolbar identities and themed SVGs describe Markdown intents through public native nodes.

use super::format::Command;
use plugin_protocol::ui;

/// One definition supplies both routing and presentation, so changing a button cannot drift its command ID.
struct Tool {
    command: Command,
    id: &'static str,
    /// Inline geometry is compiled into the independent guest and validated through `ui.icons`.
    icon: &'static str,
    tips: (&'static str, &'static str),
}

const GROUPS: &[(&str, &[Tool])] = &[
    (
        "format-headings",
        &[
            Tool {
                command: Command::Heading,
                id: "format-heading",
                icon: include_str!("../icons/format-heading.svg"),
                tips: ("一级标题", "Heading 1"),
            },
            Tool {
                command: Command::HeadingLevel(2),
                id: "format-heading-2",
                icon: include_str!("../icons/format-heading-2.svg"),
                tips: ("二级标题", "Heading 2"),
            },
            Tool {
                command: Command::HeadingLevel(3),
                id: "format-heading-3",
                icon: include_str!("../icons/format-heading-3.svg"),
                tips: ("三级标题", "Heading 3"),
            },
            Tool {
                command: Command::HeadingLevel(4),
                id: "format-heading-4",
                icon: include_str!("../icons/format-heading-4.svg"),
                tips: ("四级标题", "Heading 4"),
            },
            Tool {
                command: Command::HeadingLevel(5),
                id: "format-heading-5",
                icon: include_str!("../icons/format-heading-5.svg"),
                tips: ("五级标题", "Heading 5"),
            },
            Tool {
                command: Command::HeadingLevel(6),
                id: "format-heading-6",
                icon: include_str!("../icons/format-heading-6.svg"),
                tips: ("六级标题", "Heading 6"),
            },
        ],
    ),
    (
        "format-emphasis",
        &[
            Tool {
                command: Command::Bold,
                id: "format-bold",
                icon: include_str!("../icons/format-bold.svg"),
                tips: ("粗体", "Bold"),
            },
            Tool {
                command: Command::Italic,
                id: "format-italic",
                icon: include_str!("../icons/format-italic.svg"),
                tips: ("斜体", "Italic"),
            },
            Tool {
                command: Command::Strike,
                id: "format-strike",
                icon: include_str!("../icons/format-strike.svg"),
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
                icon: include_str!("../icons/format-inline-code.svg"),
                tips: ("行内代码", "Inline code"),
            },
            Tool {
                command: Command::CodeBlock,
                id: "format-code-block",
                icon: include_str!("../icons/format-code-block.svg"),
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
                icon: include_str!("../icons/format-quote.svg"),
                tips: ("引用", "Block quote"),
            },
            Tool {
                command: Command::Unordered,
                id: "format-unordered",
                icon: include_str!("../icons/format-unordered.svg"),
                tips: ("无序列表", "Unordered list"),
            },
            Tool {
                command: Command::Ordered,
                id: "format-ordered",
                icon: include_str!("../icons/format-ordered.svg"),
                tips: ("有序列表", "Ordered list"),
            },
            Tool {
                command: Command::Task,
                id: "format-task",
                icon: include_str!("../icons/format-task.svg"),
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
                icon: include_str!("../icons/format-link.svg"),
                tips: ("链接", "Link"),
            },
            Tool {
                command: Command::Image,
                id: "format-image",
                icon: include_str!("../icons/format-image.svg"),
                tips: ("图片引用模板", "Image reference template"),
            },
            Tool {
                command: Command::Table,
                id: "format-table",
                icon: include_str!("../icons/format-table.svg"),
                tips: ("两列表格", "Two-column table"),
            },
        ],
    ),
];

/// Only declared toolbar nodes route to formatting; parsed task edits and preview links have separate routing.
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
                    let tip = if english { tool.tips.1 } else { tool.tips.0 };
                    ui::Node::button(tool.id, tip)
                        .icon(tool.icon)
                        .tooltip(tip)
                        .role("toolbar_button")
                })
                .collect();
            ui::Node::row(*id, buttons).wrap().gap(2.)
        })
        .collect();
    let mut children = vec![ui::Node::row("format-actions", groups).wrap().gap(8.)];
    if let Some(error) = error {
        // Buttons keep their natural wrapped height. Only feedback scrolls, so every saved filename remains readable.
        children.push(
            ui::Node::scroll("format-feedback", ui::Node::text("format-error", error)).height(56.),
        );
    }
    ui::Node::column("format-toolbar", children)
        .padding(2.)
        .gap(2.)
}
