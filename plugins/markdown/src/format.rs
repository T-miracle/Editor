//! Plan one Markdown edit from an immutable source snapshot and a UTF-8 selection.

use pulldown_cmark::{Event, Options, Parser, Tag};
use std::{collections::HashSet, ops::Range};

/// A toolbar intent becomes a single host document transaction.
#[derive(Clone, Copy)]
pub(super) enum Command {
    Heading,
    /// Levels two through six share the current-line heading transaction with level one.
    HeadingLevel(u8),
    Bold,
    Italic,
    Strike,
    InlineCode,
    Quote,
    Unordered,
    Ordered,
    Task,
    Link,
    Image,
    Table,
    CodeBlock,
}

/// Replacement bytes and the selection in the resulting full document, never a mutable guest document.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Edit {
    pub range: Range<usize>,
    pub text: String,
    pub selection: Range<usize>,
}

/// Reject invalid UTF-8 boundaries instead of guessing offsets from character counts.
/// Blank-only or unrenderable emphasis and block endpoints inside CRLF also produce no edit.
pub(super) fn plan(
    command: Command,
    source: &str,
    selection: Range<usize>,
    english: bool,
) -> Option<Edit> {
    let selected = source.get(selection.clone())?;
    // UTF-8 permits the byte between CR and LF, but a block edit must retain their shared line boundary.
    if matches!(
        command,
        Command::Heading
            | Command::HeadingLevel(_)
            | Command::CodeBlock
            | Command::Quote
            | Command::Unordered
            | Command::Ordered
            | Command::Task
            | Command::Table
    ) && (divides_crlf(source, selection.start) || divides_crlf(source, selection.end))
    {
        return None;
    }
    if let Command::Heading | Command::HeadingLevel(_) = command {
        let level = match command {
            Command::Heading => 1,
            Command::HeadingLevel(level) => level,
            _ => unreachable!(),
        };
        return heading(source, selection, level, english);
    }
    if selection.is_empty() {
        let edit = template(command, source, selection.start, english);
        if matches!(
            command,
            Command::Bold | Command::Italic | Command::Strike | Command::Link | Command::Image
        ) {
            // A neighboring backslash or code delimiter can also invalidate an inserted reference template.
            let span = (edit.range.start, edit.range.start + edit.text.len());
            return rendered(source, edit, command, HashSet::from([span]));
        }
        return Some(edit);
    }
    Some(match command {
        Command::Bold => emphasis(source, selection, selected, "**", command)?,
        Command::Italic => emphasis(source, selection, selected, "*", command)?,
        Command::Strike => emphasis(source, selection, selected, "~~", command)?,
        Command::InlineCode => code_span(selection, selected),
        Command::Link | Command::Image => reference(source, selection, selected, command)?,
        Command::CodeBlock => {
            let range = whole_lines(source, &selection);
            fenced(range.clone(), &source[range], newline(source))
        }
        Command::Table => {
            let range = whole_lines(source, &selection);
            let text = table(&source[range.clone()], english, newline(source));
            Edit {
                selection: range.start..range.start + text.len(),
                range,
                text,
            }
        }
        Command::Heading
        | Command::HeadingLevel(_)
        | Command::Quote
        | Command::Unordered
        | Command::Ordered
        | Command::Task => {
            let range = whole_lines(source, &selection);
            let text = source[range.clone()]
                .split('\n')
                .enumerate()
                .map(|(index, line)| format!("{}{line}", prefix(command, index)))
                .collect::<Vec<_>>()
                .join("\n");
            Edit {
                selection: range.start..range.start + text.len(),
                range,
                text,
            }
        }
    })
}

/// Change ATX markers on existing target lines; never split a line around the caret.
/// Repeating its current level removes the marker. CRLF and up to three indentation spaces survive.
fn heading(source: &str, selection: Range<usize>, level: u8, english: bool) -> Option<Edit> {
    if !(1..=6).contains(&level) {
        return None;
    }
    let range = whole_lines(source, &selection);
    if selection.is_empty() && source[range.clone()].is_empty() {
        let word = if english { "heading" } else { "标题" };
        return Some(inline(
            range,
            word,
            &format!("{} ", "#".repeat(level as usize)),
            "",
        ));
    }
    let mut text = String::new();
    let mut caret = None;
    for line in source[range.clone()].split_inclusive('\n') {
        let indent = line.bytes().take_while(|byte| *byte == b' ').count().min(3);
        let body = &line[indent..];
        let hashes = body.bytes().take_while(|byte| *byte == b'#').count();
        let existing = if (1..=6).contains(&hashes)
            && body
                .as_bytes()
                .get(hashes)
                .is_none_or(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        {
            hashes
                + body[hashes..]
                    .bytes()
                    .take_while(|byte| matches!(byte, b' ' | b'\t'))
                    .count()
        } else {
            0
        };
        let marker = if existing > 0 && hashes == level as usize {
            String::new()
        } else {
            format!("{} ", "#".repeat(level as usize))
        };
        // Keep an empty caret at the same logical content column after prefix replacement.
        if selection.is_empty() {
            let column = selection.start.saturating_sub(range.start);
            caret = Some(
                range.start + indent + marker.len() + column.saturating_sub(indent + existing),
            );
        }
        text.push_str(&line[..indent]);
        text.push_str(&marker);
        text.push_str(&body[existing..]);
    }
    let end = range.start + text.len();
    let resulting_selection =
        caret.map_or(range.start..end, |caret| caret.min(end)..caret.min(end));
    Some(Edit {
        range,
        text,
        selection: resulting_selection,
    })
}

/// CommonMark emphasis cannot enclose boundary whitespace or blank paragraphs.
/// Wrap each line's nonblank body while preserving every whitespace byte outside its delimiters.
/// The contiguous selection spans the first and last bodies; unrenderable input is not an edit.
fn emphasis(
    source: &str,
    range: Range<usize>,
    content: &str,
    marker: &str,
    command: Command,
) -> Option<Edit> {
    let mut text = String::new();
    let mut selection: Option<Range<usize>> = None;
    let mut spans = HashSet::new();
    for line in content.split_inclusive('\n') {
        let body = line.trim();
        if body.is_empty() {
            text.push_str(line);
            continue;
        }
        let leading = line.len() - line.trim_start().len();
        text.push_str(&line[..leading]);
        let marker_start = range.start + text.len();
        text.push_str(marker);
        let start = range.start + text.len();
        text.push_str(body);
        let end = range.start + text.len();
        if let Some(selection) = selection.as_mut() {
            selection.end = end;
        } else {
            selection = Some(start..end);
        }
        text.push_str(marker);
        spans.insert((marker_start, range.start + text.len()));
        text.push_str(&line[leading + body.len()..]);
    }
    let edit = Edit {
        range,
        text,
        selection: selection?,
    };
    rendered(source, edit, command, spans)
}

/// Escape Markdown label syntax without losing its literal characters, and select the generated label bytes.
/// A reference must cover its complete label in the surrounding source rather than just a nested fragment.
fn reference(source: &str, range: Range<usize>, label: &str, command: Command) -> Option<Edit> {
    let (opening, closing) = match command {
        Command::Link => ("[", "](https://example.com)"),
        Command::Image => ("![", "](image.png)"),
        _ => unreachable!("Only reference commands supply link labels"),
    };
    let escaped = label
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    let edit = inline(range, &escaped, opening, closing);
    let span = (edit.range.start, edit.range.start + edit.text.len());
    rendered(source, edit, command, HashSet::from([span]))
}

/// Validate emphasis and references against one readonly full-document candidate using the preview's grammar.
/// Exact element ranges include both delimiters, so unrelated or partial existing elements cannot qualify.
fn rendered(
    source: &str,
    edit: Edit,
    command: Command,
    mut spans: HashSet<(usize, usize)>,
) -> Option<Edit> {
    // Delimiter flanking, escapes and paragraph boundaries depend on the unselected surrounding source.
    let mut document = source.to_owned();
    document.replace_range(edit.range.clone(), &edit.text);
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    for (event, range) in Parser::new_ext(&document, options).into_offset_iter() {
        if matches!(
            (command, event),
            (Command::Bold, Event::Start(Tag::Strong))
                | (Command::Italic, Event::Start(Tag::Emphasis))
                | (Command::Strike, Event::Start(Tag::Strikethrough))
                | (Command::Link, Event::Start(Tag::Link { .. }))
                | (Command::Image, Event::Start(Tag::Image { .. }))
        ) {
            spans.remove(&(range.start, range.end));
        }
    }
    spans.is_empty().then_some(edit)
}

/// Inline operations preserve selected bytes and keep the content, rather than markers, selected.
fn inline(range: Range<usize>, content: &str, before: &str, after: &str) -> Edit {
    let start = range.start + before.len();
    Edit {
        range,
        text: format!("{before}{content}{after}"),
        selection: start..start + content.len(),
    }
}

/// Padding prevents backtick content from merging with its delimiter or losing significant outer spaces.
fn code_span(range: Range<usize>, content: &str) -> Edit {
    let fence = "`".repeat(longest_backticks(content).saturating_add(1));
    let padding = content.starts_with('`')
        || content.ends_with('`')
        || (content.starts_with(' ')
            && content.ends_with(' ')
            && content.chars().any(|c| c != ' '));
    let marker = if padding {
        format!("{fence} ")
    } else {
        fence.clone()
    };
    let closing = if padding { format!(" {fence}") } else { fence };
    inline(range, content, &marker, &closing)
}

/// Fences retain the literal code and its selected body, with a delimiter longer than every content run.
fn fenced(range: Range<usize>, body: &str, newline: &str) -> Edit {
    let fence = "`".repeat(longest_backticks(body).saturating_add(1).max(3));
    let closing_line = if body.ends_with('\n') { "" } else { newline };
    inline(
        range,
        body,
        &format!("{fence}{newline}"),
        &format!("{closing_line}{fence}"),
    )
}

/// Whole-line prefixes share one numbering and marker policy for both selections and templates.
fn prefix(command: Command, index: usize) -> String {
    match command {
        Command::Heading => "# ".into(),
        Command::HeadingLevel(level) => format!("{} ", "#".repeat(level as usize)),
        Command::Quote => "> ".into(),
        Command::Unordered => "- ".into(),
        Command::Ordered => format!("{}. ", index + 1),
        Command::Task => "- [ ] ".into(),
        _ => unreachable!("Only whole-line commands request prefixes"),
    }
}

/// Every empty-selection command offers a localized editable placeholder; block templates stay on their own lines.
fn template(command: Command, source: &str, cursor: usize, english: bool) -> Edit {
    let words = match command {
        Command::Heading | Command::HeadingLevel(_) => ("标题", "heading"),
        Command::Bold => ("粗体", "bold text"),
        Command::Italic => ("斜体", "italic text"),
        Command::Strike => ("删除线", "deleted text"),
        Command::InlineCode | Command::CodeBlock => ("代码", "code"),
        Command::Quote => ("引用", "quote"),
        Command::Unordered | Command::Ordered => ("列表项", "item"),
        Command::Task => ("任务", "task"),
        Command::Link => ("链接文字", "link text"),
        Command::Image => ("图片说明", "image description"),
        Command::Table => ("内容", "content"),
    };
    let word = if english { words.1 } else { words.0 };
    let range = cursor..cursor;
    match command {
        Command::Bold => inline(range, word, "**", "**"),
        Command::Italic => inline(range, word, "*", "*"),
        Command::Strike => inline(range, word, "~~", "~~"),
        Command::InlineCode => code_span(range, word),
        Command::Link => inline(range, word, "[", "](https://example.com)"),
        Command::Image => inline(range, word, "![", "](image.png)"),
        Command::CodeBlock => {
            let edit = fenced(0..0, word, newline(source));
            insert_block(source, cursor, edit.text, edit.selection)
        }
        Command::Table => {
            let headers = headers(english);
            let newline = newline(source);
            let text = format!(
                "{}{}| {word} | {word} |",
                headers.0.replace('\n', newline),
                newline
            );
            insert_block(source, cursor, text, 2..2 + headers.1.len())
        }
        Command::Heading
        | Command::HeadingLevel(_)
        | Command::Quote
        | Command::Unordered
        | Command::Ordered
        | Command::Task => {
            let prefix = prefix(command, 0);
            let start = prefix.len();
            insert_block(
                source,
                cursor,
                format!("{prefix}{word}"),
                start..start + word.len(),
            )
        }
    }
}

/// Selected source lines become first-column rows; escaped pipes cannot silently add columns.
fn table(body: &str, english: bool, newline: &str) -> String {
    let mut text = headers(english).0.replace('\n', newline);
    for line in body.split('\n') {
        let cell = line
            .strip_suffix('\r')
            .unwrap_or(line)
            .replace('\\', "\\\\")
            .replace('|', "\\|");
        text.push_str(&format!("{newline}| {cell} |  |"));
    }
    text
}

/// Both table paths use the same two-column GFM header and choose the first heading as the template placeholder.
fn headers(english: bool) -> (&'static str, &'static str) {
    if english {
        ("| Header 1 | Header 2 |\n| --- | --- |", "Header 1")
    } else {
        ("| 列1 | 列2 |\n| --- | --- |", "列1")
    }
}

/// A selection ending immediately after a newline does not include the following line.
/// The final line terminator remains outside the replacement, preserving both LF and CRLF.
fn whole_lines(source: &str, selection: &Range<usize>) -> Range<usize> {
    let start = source[..selection.start]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let last = if selection.end > selection.start && source.as_bytes()[selection.end - 1] == b'\n' {
        selection.end - 1
    } else {
        selection.end
    };
    let mut end = source[last..]
        .find('\n')
        .map_or(source.len(), |newline| last + newline);
    if end < source.len() && end > start && source.as_bytes()[end - 1] == b'\r' {
        end -= 1;
    }
    start..end
}

/// Treat CRLF as one block boundary even though both bytes independently satisfy the UTF-8 contract.
fn divides_crlf(source: &str, offset: usize) -> bool {
    let bytes = source.as_bytes();
    offset > 0 && bytes.get(offset - 1) == Some(&b'\r') && bytes.get(offset) == Some(&b'\n')
}

/// New markers follow the document's existing CRLF convention without normalizing its content.
fn newline(source: &str) -> &str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Longer delimiters keep every existing run literal, including pasted Markdown examples.
fn longest_backticks(content: &str) -> usize {
    content
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
}

/// Insert a standalone block at the cursor without replacing neighboring text on that line.
/// Its local placeholder is translated to the resulting full document after any added separators.
fn insert_block(source: &str, cursor: usize, text: String, placeholder: Range<usize>) -> Edit {
    let newline = newline(source);
    let leading = if cursor > 0 && !source[..cursor].ends_with('\n') {
        newline
    } else {
        ""
    };
    let after = &source[cursor..];
    let trailing = if after.is_empty() || after.starts_with('\n') || after.starts_with("\r\n") {
        ""
    } else {
        newline
    };
    let start = cursor + leading.len();
    Edit {
        range: cursor..cursor,
        text: format!("{leading}{text}{trailing}"),
        selection: start + placeholder.start..start + placeholder.end,
    }
}

#[cfg(test)]
mod tests;
