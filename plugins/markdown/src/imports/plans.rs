//! Pure filename and UTF-8 insertion plans for host-owned image inputs.

use crate::format::Edit;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::{collections::HashSet, ops::Range};

/// Allocate one basename from an actual format extension and a bounded collision cursor.
pub(super) fn candidate(extension: &str, index: usize) -> Option<String> {
    if index > 4096 || !matches!(extension, "png" | "jpg" | "gif" | "webp" | "svg") {
        return None;
    }
    Some(if index == 0 {
        format!("img.{extension}")
    } else {
        format!("img{index}.{extension}")
    })
}

/// Replace exactly the input selection once, retaining every byte outside it and collapsing the new caret.
/// Invalid UTF-8/CRLF boundaries, noncanonical names and references hidden by Markdown context return no edit.
pub(super) fn insertion(
    source: &str,
    selection: Range<usize>,
    names: &[String],
    english: bool,
) -> Option<Edit> {
    source.get(selection.clone())?;
    if names.is_empty()
        || names.len() > 8
        || !names.iter().all(|name| canonical(name))
        || [selection.start, selection.end].into_iter().any(|offset| {
            offset > 0
                && source.as_bytes().get(offset - 1) == Some(&b'\r')
                && source.as_bytes().get(offset) == Some(&b'\n')
        })
    {
        return None;
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let alt = if english { "Image" } else { "图片" };
    let references = names
        .iter()
        .map(|name| format!("![{alt}]({name})"))
        .collect::<Vec<_>>();
    let text = references.join(newline);
    let caret = selection.start.checked_add(text.len())?;
    // Parse the complete resulting source: a surrounding fence or backslash can hide a valid isolated template.
    // Every generated image must have its exact source span and intended local destination in the real preview grammar.
    let result = format!(
        "{}{}{}",
        &source[..selection.start],
        text,
        &source[selection.end..]
    );
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut rendered = HashSet::new();
    let mut image_depth = 0usize;
    for (event, range) in Parser::new_ext(&result, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => {
                // The native preview flattens nested images into their outer image's alt text.
                // Parser recognition inside that label must not count as a displayed imported image.
                if image_depth == 0 {
                    rendered.insert((range.start, range.end, dest_url.into_string()));
                }
                image_depth += 1;
            }
            Event::End(TagEnd::Image) => image_depth -= 1,
            _ => {}
        }
    }
    let mut start = selection.start;
    for (reference, name) in references.iter().zip(names) {
        let end = start + reference.len();
        if !rendered.contains(&(start, end, name.clone())) {
            return None;
        }
        start = end + newline.len();
    }
    Some(Edit {
        range: selection,
        text,
        selection: caret..caret,
    })
}

/// Only the fixed local naming scheme may enter generated destinations; a receipt cannot smuggle a path or URI.
fn canonical(name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    let index = if stem == "img" {
        Some(0)
    } else {
        stem.strip_prefix("img")
            .and_then(|index| index.parse().ok())
    };
    index
        .and_then(|index| candidate(extension, index))
        .as_deref()
        == Some(name)
}

#[cfg(test)]
mod tests {
    use super::{candidate, insertion};

    /// One batch can share its increasing index across actual formats, without directories or JPEG renaming.
    #[test]
    fn image_names_follow_the_actual_format_and_bounded_collision_order() {
        for (extension, index, expected) in [
            ("png", 0, "img.png"),
            ("png", 1, "img1.png"),
            ("png", 2, "img2.png"),
            ("jpg", 0, "img.jpg"),
            ("jpg", 1, "img1.jpg"),
            ("gif", 0, "img.gif"),
            ("webp", 4096, "img4096.webp"),
            ("svg", 0, "img.svg"),
        ] {
            assert_eq!(candidate(extension, index).as_deref(), Some(expected));
        }
        assert!(candidate("png", 4097).is_none());
        assert!(candidate("../png", 0).is_none());
        assert!(candidate("exe", 0).is_none());
    }

    /// Multiple actual formats share one UTF-8 edit and the existing CRLF convention, without expanding the selection.
    #[test]
    fn image_references_replace_chinese_selection_once_and_preserve_neighbors() {
        let source = "前中文后\r\n末";
        let names = vec!["img.png".into(), "img1.jpg".into()];
        let edit = insertion(source, 3..9, &names, false).unwrap();
        let references = "![图片](img.png)\r\n![图片](img1.jpg)";
        assert_eq!(edit.range, 3..9);
        assert_eq!(edit.text, references);
        let caret = 3 + references.len();
        assert_eq!(edit.selection, caret..caret);
        let result = format!(
            "{}{}{}",
            &source[..edit.range.start],
            edit.text,
            &source[edit.range.end..]
        );
        assert_eq!(result, format!("前{references}后\r\n末"));
        let destinations = pulldown_cmark::Parser::new(&result)
            .filter_map(|event| match event {
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { dest_url, .. }) => {
                    Some(dest_url.to_string())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(destinations, names);
    }

    /// Valid UTF-8 alone is insufficient: splitting CRLF or placing a reference in code would hide the imported image.
    #[test]
    fn image_insertion_rejects_split_crlf_and_non_rendering_contexts() {
        let names = vec!["img.png".into()];
        for (source, range) in [
            ("甲\r\n乙", 4..4),
            ("甲\r\n乙", 3..4),
            ("甲\r\n乙", 4..5),
            ("```\n中文\n```\n", 4..10),
            ("\\ 后文", 1..1),
            ("中文", 1..1),
            ("中文", 0..7),
        ] {
            assert!(
                insertion(source, range.clone(), &names, true).is_none(),
                "invalid image context {source:?} at {range:?}"
            );
        }
        assert!(insertion("", 0..0, &[], true).is_none());
        assert!(insertion("", 0..0, &["../img.png".into()], true).is_none());
        let edit = insertion("前后", 3..3, &names, true).unwrap();
        assert_eq!(edit.text, "![Image](img.png)");
        assert_eq!(edit.selection, 20..20);
    }

    /// An image nested in another image's alt text is not a displayed native image, even if the parser recognizes it.
    #[test]
    fn image_insertion_rejects_an_outer_image_alt_but_accepts_link_content() {
        let names = vec!["img.png".into()];
        assert!(insertion("![outer ](old.png)", 8..8, &names, true).is_none());
        let source = "[link ](path.md)";
        let edit = insertion(source, 6..6, &names, true).unwrap();
        let result = format!(
            "{}{}{}",
            &source[..edit.range.start],
            edit.text,
            &source[edit.range.end..]
        );
        /// Inspect the public native tree rather than treating every parser image tag as a visible image.
        fn has_imported_image(node: &plugin_protocol::ui::Node) -> bool {
            use plugin_protocol::ui::Kind;
            match &node.kind {
                Kind::Image { source, .. } => source == "img.png",
                Kind::Column { children } | Kind::Row { children } => {
                    children.iter().any(has_imported_image)
                }
                Kind::Scroll { content } => has_imported_image(content),
                _ => false,
            }
        }
        assert!(
            crate::preview::blocks(&result, "en")
                .unwrap()
                .iter()
                .any(has_imported_image)
        );
    }
}
