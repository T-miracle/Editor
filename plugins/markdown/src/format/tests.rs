//! Planner regressions observe Markdown rendering, preserved source bytes and resulting UTF-8 selections.

/// Native preview must recognize selected emphasis even with boundary whitespace and paragraphs.
#[test]
fn emphasis_with_whitespace_and_blank_lines_renders_as_formatting() {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    for (command, marker) in [
        (Command::Bold, "**"),
        (Command::Italic, "*"),
        (Command::Strike, "~~"),
    ] {
        for (source, expected, count) in [
            (" 中文 ", format!(" {marker}中文{marker} "), 1),
            (
                "\u{3000}中文\t",
                format!("\u{3000}{marker}中文{marker}\t"),
                1,
            ),
            (
                "甲\n乙",
                format!("{marker}甲{marker}\n{marker}乙{marker}"),
                2,
            ),
            (
                "甲\r\n\r\n乙",
                format!("{marker}甲{marker}\r\n\r\n{marker}乙{marker}"),
                2,
            ),
        ] {
            let edit = plan(command, source, 0..source.len(), false).unwrap();
            assert_eq!(edit.text, expected);
            let rendered = Parser::new_ext(&edit.text, Options::ENABLE_STRIKETHROUGH)
                .filter(|event| {
                    matches!(
                        (command, event),
                        (Command::Bold, Event::Start(Tag::Strong))
                            | (Command::Italic, Event::Start(Tag::Emphasis))
                            | (Command::Strike, Event::Start(Tag::Strikethrough))
                    )
                })
                .count();
            assert_eq!(
                rendered, count,
                "selected format must reach the actual Markdown parser"
            );
            assert!(edit.text.is_char_boundary(edit.selection.start));
            assert!(edit.text.is_char_boundary(edit.selection.end));
        }
    }
}

use super::*;

/// The surrounding source can prevent punctuation-adjacent delimiters from rendering as emphasis.
#[test]
fn emphasis_rejects_unrepresentable_punctuation_in_the_full_document() {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    for (command, marker) in [
        (Command::Bold, "**"),
        (Command::Italic, "*"),
        (Command::Strike, "~~"),
    ] {
        for (source, range, candidate) in [
            ("中文，后文", 0..9, format!("{marker}中文，{marker}后文")),
            (
                "前（中文）后",
                3..15,
                format!("前{marker}（中文）{marker}后"),
            ),
        ] {
            // This verifies the complete document, including the unselected adjacent words.
            let rendered =
                Parser::new_ext(&candidate, Options::ENABLE_STRIKETHROUGH).any(|event| {
                    matches!(
                        (command, event),
                        (Command::Bold, Event::Start(Tag::Strong))
                            | (Command::Italic, Event::Start(Tag::Emphasis))
                            | (Command::Strike, Event::Start(Tag::Strikethrough))
                    )
                });
            assert!(
                !rendered,
                "the candidate's selected punctuation cannot form this emphasis"
            );
            assert!(plan(command, source, range, false).is_none());
        }
    }
}

/// Ordinary Chinese word adjacency remains valid; rejection is based on parser coverage, not any neighbor.
#[test]
fn emphasis_between_chinese_words_renders_in_the_full_document() {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    for command in [Command::Bold, Command::Italic, Command::Strike] {
        let source = "前中文后";
        let edit = plan(command, source, 3..9, false).unwrap();
        let mut document = source.to_owned();
        document.replace_range(edit.range.clone(), &edit.text);
        let rendered = Parser::new_ext(&document, Options::ENABLE_STRIKETHROUGH)
            .into_offset_iter()
            .any(|(event, range)| {
                matches!(
                    (command, event),
                    (Command::Bold, Event::Start(Tag::Strong))
                        | (Command::Italic, Event::Start(Tag::Emphasis))
                        | (Command::Strike, Event::Start(Tag::Strikethrough))
                ) && range == (3..3 + edit.text.len())
            });
        assert!(
            rendered,
            "the actual full-document parser must cover the inserted markers"
        );
        assert_eq!(&document[edit.selection.clone()], "中文");
    }
}

/// Literal brackets and backslashes remain one complete rendered label in the actual surrounding document.
#[test]
fn references_escape_labels_and_render_the_complete_selected_text() {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    for command in [Command::Link, Command::Image] {
        for (label, escaped) in [
            (r"甲[乙", r"甲\[乙"),
            (r"甲]乙", r"甲\]乙"),
            (r"甲\", r"甲\\"),
            (r"甲\[乙]", r"甲\\\[乙\]"),
        ] {
            let source = format!("前 {label} 后");
            let edit = plan(command, &source, 4..4 + label.len(), false).unwrap();
            let mut document = source.clone();
            document.replace_range(edit.range.clone(), &edit.text);
            let events: Vec<_> = Parser::new_ext(&document, Options::ENABLE_STRIKETHROUGH)
                .into_offset_iter()
                .collect();
            let complete = events.iter().any(|(event, range)| {
                matches!(
                    (command, event),
                    (Command::Link, Event::Start(Tag::Link { .. }))
                        | (Command::Image, Event::Start(Tag::Image { .. }))
                ) && *range == (4..4 + edit.text.len())
            });
            assert!(
                complete,
                "the full document must render the entire inserted reference"
            );
            let rendered_label: String = events
                .iter()
                .filter_map(|(event, range)| match event {
                    Event::Text(text)
                        if range.start >= edit.selection.start
                            && range.end <= edit.selection.end =>
                    {
                        Some(text.as_ref())
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                rendered_label, label,
                "escaping must retain all literal label characters"
            );
            assert_eq!(&document[edit.selection.clone()], escaped);
        }
    }
}

/// References cannot cross paragraph boundaries; accepting one would leave literal or partial markup.
#[test]
fn references_reject_paragraph_crossing_selection_in_the_full_document() {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    for (command, opening, closing) in [
        (Command::Link, "[", "](https://example.com)"),
        (Command::Image, "![", "](image.png)"),
    ] {
        let source = "前 甲\n\n乙 后";
        let candidate = format!("前 {opening}甲\n\n乙{closing} 后");
        let complete = Parser::new_ext(&candidate, Options::ENABLE_STRIKETHROUGH).any(|event| {
            matches!(
                (command, event),
                (Command::Link, Event::Start(Tag::Link { .. }))
                    | (Command::Image, Event::Start(Tag::Image { .. }))
            )
        });
        assert!(
            !complete,
            "the surrounding paragraphs cannot form one reference"
        );
        assert!(plan(command, source, 4..12, false).is_none());
    }
}

/// Edge whitespace remains outside the markers and outside the selected editable Chinese body.
#[test]
fn emphasis_keeps_body_selected_in_the_resulting_full_document() {
    for (command, text, selection) in [
        (Command::Bold, " **中文** ", 6..12),
        (Command::Italic, " *中文* ", 5..11),
        (Command::Strike, " ~~中文~~ ", 6..12),
    ] {
        assert_eq!(
            plan(command, "前 中文 后", 3..11, false).unwrap(),
            Edit {
                range: 3..11,
                text: text.into(),
                selection
            }
        );
    }
}

/// Blank-only selections produce no replacement, avoiding literal markers and meaningless undo entries.
#[test]
fn emphasis_does_not_format_a_whitespace_only_selection() {
    for command in [Command::Bold, Command::Italic, Command::Strike] {
        for source in [" ", "\t", "\r\n\r\n", " \t\r\n\u{3000} "] {
            assert!(plan(command, source, 0..source.len(), false).is_none());
        }
    }
}

/// The user keeps the selected Chinese word inside the markers after the single edit.
#[test]
fn bold_wraps_a_chinese_selection_and_keeps_its_byte_range() {
    assert_eq!(
        plan(Command::Bold, "甲乙丙", 3..6, false).unwrap(),
        Edit {
            range: 3..6,
            text: "**乙**".into(),
            selection: 5..8,
        }
    );
}

/// Empty selections insert a localized template and make its content immediately replaceable.
#[test]
fn bold_empty_selection_selects_the_localized_placeholder() {
    for (english, text, selection) in [(false, "**粗体**", 5..11), (true, "**bold text**", 5..14)]
    {
        assert_eq!(
            plan(Command::Bold, "前后", 3..3, english).unwrap(),
            Edit {
                range: 3..3,
                text: text.into(),
                selection,
            }
        );
    }
}

/// A multiline selection expands to complete CRLF lines and excludes an unselected next line.
#[test]
fn ordered_lines_keep_crlf_and_exclude_the_next_line_start() {
    assert_eq!(
        plan(Command::Ordered, "前一\r\n二行\r\n后三", 3..16, false).unwrap(),
        Edit {
            range: 0..14,
            text: "1. 前一\r\n2. 二行".into(),
            selection: 0..20,
        }
    );
}

/// Literal backticks remain code because the inserted fence is longer than every content run.
#[test]
fn code_fence_preserves_chinese_and_backticks_without_closing_early() {
    assert_eq!(
        plan(Command::CodeBlock, "`甲```\n乙", 0..11, false).unwrap(),
        Edit {
            range: 0..11,
            text: "````\n`甲```\n乙\n````".into(),
            selection: 5..16,
        }
    );
}

/// A block template inserted inside a CRLF line separates existing text and selects only its placeholder.
#[test]
fn empty_code_block_keeps_neighboring_text_and_selects_its_placeholder() {
    assert_eq!(
        plan(Command::CodeBlock, "甲乙\r\n后", 3..3, false).unwrap(),
        Edit {
            range: 3..3,
            text: "\r\n```\r\n代码\r\n```\r\n".into(),
            selection: 10..16,
        }
    );
}

/// Every approved toolbar command formats existing text and inserts a replaceable empty-selection template.
#[test]
fn all_toolbar_commands_preserve_selection_or_select_their_template() {
    for (command, selected, selected_range, template, template_range) in [
        (Command::Heading, "# 中文", 0..8, "# 标题", 2..8),
        (Command::Bold, "**中文**", 2..8, "**粗体**", 2..8),
        (Command::Italic, "*中文*", 1..7, "*斜体*", 1..7),
        (Command::Strike, "~~中文~~", 2..8, "~~删除线~~", 2..11),
        (Command::InlineCode, "`中文`", 1..7, "`代码`", 1..7),
        (
            Command::CodeBlock,
            "```\n中文\n```",
            4..10,
            "```\n代码\n```",
            4..10,
        ),
        (Command::Quote, "> 中文", 0..8, "> 引用", 2..8),
        (Command::Unordered, "- 中文", 0..8, "- 列表项", 2..11),
        (Command::Ordered, "1. 中文", 0..9, "1. 列表项", 3..12),
        (Command::Task, "- [ ] 中文", 0..12, "- [ ] 任务", 6..12),
        (
            Command::Link,
            "[中文](https://example.com)",
            1..7,
            "[链接文字](https://example.com)",
            1..13,
        ),
        (
            Command::Image,
            "![中文](image.png)",
            2..8,
            "![图片说明](image.png)",
            2..14,
        ),
        (
            Command::Table,
            "| 列1 | 列2 |\n| --- | --- |\n| 中文 |  |",
            0..43,
            "| 列1 | 列2 |\n| --- | --- |\n| 内容 | 内容 |",
            2..6,
        ),
    ] {
        assert_eq!(
            plan(command, "中文", 0..6, false).unwrap(),
            Edit {
                range: 0..6,
                text: selected.into(),
                selection: selected_range
            }
        );
        assert_eq!(
            plan(command, "", 0..0, false).unwrap(),
            Edit {
                range: 0..0,
                text: template.into(),
                selection: template_range
            }
        );
    }
}

/// Inline code retains literal delimiters instead of allowing them to terminate the generated span.
#[test]
fn inline_code_preserves_backticks_and_significant_outer_spaces() {
    for (source, text, selection) in [("`中`", "`` `中` ``", 3..8), (" 中 ", "`  中  `", 2..7)]
    {
        assert_eq!(
            plan(Command::InlineCode, source, 0..5, false).unwrap(),
            Edit {
                range: 0..5,
                text: text.into(),
                selection
            }
        );
    }
}

/// Literal pipes remain in a cell instead of becoming extra GFM columns.
#[test]
fn table_selected_pipes_remain_in_the_first_column() {
    assert_eq!(
        plan(Command::Table, "中|文", 0..7, false).unwrap(),
        Edit {
            range: 0..7,
            text: "| 列1 | 列2 |\n| --- | --- |\n| 中\\|文 |  |".into(),
            selection: 0..45,
        }
    );
}

/// Host validation is also reflected in the planner so byte offsets are never silently rounded.
#[test]
fn invalid_utf8_reversed_and_out_of_bounds_ranges_are_rejected() {
    for range in [1..6, 0..4, 4..3, 0..9] {
        assert!(plan(Command::Bold, "中文", range, false).is_none());
    }
    for range in [1..4, 0..3, 7..8] {
        assert!(plan(Command::Bold, "😀甲", range, false).is_none());
    }
}

/// Block operations reject an endpoint inside CRLF instead of splitting the document's line terminator.
#[test]
fn block_commands_reject_selection_endpoints_inside_crlf() {
    for command in [
        Command::Heading,
        Command::CodeBlock,
        Command::Quote,
        Command::Unordered,
        Command::Ordered,
        Command::Task,
        Command::Table,
    ] {
        for range in [4..4, 4..8, 0..4] {
            assert!(plan(command, "甲\r\n乙", range, false).is_none());
        }
        // Complete CRLF boundaries still support both insertion and whole-line formatting.
        assert!(plan(command, "甲\r\n乙", 5..5, false).is_some());
        assert!(plan(command, "甲\r\n乙", 0..5, false).is_some());
    }
}
