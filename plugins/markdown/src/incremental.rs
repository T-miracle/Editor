//! Reuse immutable parsed blocks when an edit cannot change surrounding CommonMark boundaries.
use plugin_protocol::ui::{self, Node};

/// Cached text is a read-only parse input, replaced atomically; it never owns edits or undo.
#[derive(Default)]
pub(super) struct Parsed {
    text: String,
    locale: String,
    pub nodes: Vec<Node>,
    /// Parser-budget failures remain visible to the caller instead of escaping as an invalid view.
    pub limited: bool,
    /// Previous and replacement byte bounds for consumers of the same read-only parse cache.
    pub changed: Option<(usize, usize, usize)>,
    generation: u64,
}

impl Parsed {
    /// Conservative block invalidation: ordinary inline edits parse one enclosing block;
    /// structural edits and document-wide references expand to a complete correct parse.
    pub fn update(&mut self, text: &str, locale: &str) {
        self.changed = None;
        if self.text == text && self.locale == locale {
            self.changed = Some((0, 0, 0));
            return;
        }
        let local = self.locale == locale && self.replace_block(text, locale);
        if !local {
            self.nodes = match super::preview::blocks(text, locale) {
                Ok(nodes) => {
                    self.limited = false;
                    nodes
                }
                Err(()) => {
                    self.limited = true;
                    Vec::new()
                }
            };
        }
        self.text = text.to_owned();
        self.locale = locale.to_owned();
    }

    /// Prefix/suffix comparison finds the changed bytes without a second mutable editor document.
    fn replace_block(&mut self, text: &str, locale: &str) -> bool {
        let old = self.text.as_str();
        let mut start = old
            .bytes()
            .zip(text.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !old.is_char_boundary(start) || !text.is_char_boundary(start) {
            start -= 1;
        }
        let mut suffix = old[start..]
            .bytes()
            .rev()
            .zip(text[start..].bytes().rev())
            .take_while(|(a, b)| a == b)
            .count();
        while !old.is_char_boundary(old.len() - suffix)
            || !text.is_char_boundary(text.len() - suffix)
        {
            suffix -= 1;
        }
        let end = old.len() - suffix;
        let next_end = text.len() - suffix;
        // Line boundaries, reference definitions and fences may alter arbitrarily distant blocks.
        if old[start..end].contains(['\n', '\r'])
            || text[start..next_end].contains(['\n', '\r'])
            || old
                .lines()
                .chain(text.lines())
                .any(|line| line.trim_start().starts_with('[') && line.contains("]:"))
        {
            return false;
        }
        let Some(index) = self.nodes.iter().position(|node| {
            node.source_range
                .is_some_and(|range| range.start <= start && end < range.end)
        }) else {
            return false;
        };
        let range = self.nodes[index].source_range.unwrap();
        let delta = text.len() as i64 - old.len() as i64;
        let new_end = (range.end as i64 + delta) as usize;
        let piece = &text[range.start..new_end];
        let prior = &old[range.start..range.end];
        if prior.contains("```")
            || prior.contains("~~~")
            || piece.contains("```")
            || piece.contains("~~~")
            || prior.trim_start().starts_with('<')
            || piece.trim_start().starts_with('<')
        {
            return false;
        }
        let Ok(mut replacement) = super::preview::blocks(piece, locale) else {
            return false;
        };
        if replacement.len() != 1 {
            return false;
        }
        // A heading turned into prose can merge with the next block without inserting a newline.
        // Keep the fast path for unchanged block semantics; punctuation/indent changes need context.
        if old[start..end]
            .chars()
            .chain(text[start..next_end].chars())
            .any(|ch| ch.is_ascii_punctuation() || ch == '\t')
            || (start == range.start
                && prior.starts_with([' ', '\t', '#', '>', '-', '*', '+', '=']))
        {
            return false;
        }
        // Preserve identities even when following byte coordinates move, avoiding native state churn.
        let mut node = replacement.remove(0);
        if ui::incremental::shift_source(&mut node, range.start as i64).is_err() {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        rename(&mut node, self.generation);
        node.id = self.nodes[index].id.clone();
        self.nodes[index] = node;
        for following in &mut self.nodes[index + 1..] {
            if ui::incremental::shift_source(following, delta).is_err() {
                return false;
            }
        }
        self.changed = Some((range.start, range.end, new_end));
        true
    }
}

/// Generated descendant IDs are document-relative, while retained siblings keep stable identities.
fn rename(node: &mut Node, generation: u64) {
    // A replacement namespace cannot collide with retained siblings after arbitrarily large insertions.
    node.id = format!("edit-{generation}-{}", node.id);
    match &mut node.kind {
        ui::Kind::Column { children } | ui::Kind::Row { children } => {
            for child in children {
                rename(child, generation);
            }
        }
        ui::Kind::Scroll { content } => rename(content, generation),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// IDs are intentionally stable rather than regenerated offsets; compare the rendered semantics separately.
    fn semantic(mut nodes: Vec<Node>) -> Vec<Node> {
        fn scrub(node: &mut Node) {
            node.id.clear();
            match &mut node.kind {
                ui::Kind::Column { children } | ui::Kind::Row { children } => {
                    for child in children {
                        scrub(child);
                    }
                }
                ui::Kind::Scroll { content } => scrub(content),
                _ => {}
            }
        }
        for node in &mut nodes {
            scrub(node);
        }
        nodes
    }

    /// Local prose, nested lists and table cells match a fresh parse; structural edits safely widen invalidation.
    #[test]
    fn incremental_output_matches_full_parse_across_markdown_boundaries() {
        for (old, needle, replacement) in [
            ("甲。\n\n乙。\n\n丙。\n", "乙", "乙，中文"),
            ("# 标题\n\n正文\n", "标题", "新标题"),
            ("# 标题\n正文\n", "#", "普通"),
            ("- [ ] 第一项\n- 第二项\n\n尾部\n", "第一", "更新第一"),
            ("| 列 |\n| --- |\n| 原文 |\n\n尾部\n", "原文", "修改正文"),
            ("段落\n\n[链接][ref]\n\n[ref]: old.md\n", "old", "new"),
            ("段落\n\n```rs\ncode\n```\n\n尾部\n", "code", "changed"),
            ("段落\n\n尾部\n", "段落", "两段\n\n新增"),
        ] {
            let mut cache = Parsed::default();
            cache.update(old, "zh-CN");
            let next = old.replacen(needle, replacement, 1);
            cache.update(&next, "zh-CN");
            assert_eq!(
                semantic(cache.nodes),
                semantic(super::super::preview::blocks(&next, "zh-CN").unwrap()),
                "{old:?} -> {next:?}"
            );
        }
    }

    /// A hundred-block document transmits one changed paragraph and reconstructs the same full scene.
    #[test]
    fn local_edit_reuses_unmodified_blocks_across_the_public_delta() {
        let text = (0..100)
            .map(|index| format!("段落 {index} 正文内容。\n\n"))
            .collect::<String>();
        let mut cache = Parsed::default();
        cache.update(&text, "zh-CN");
        let old = ui::Document::new(Node::column("body", cache.nodes.clone()));
        let next = text.replacen("段落 50", "修改段落 50", 1);
        cache.update(&next, "zh-CN");
        assert!(
            cache.changed.is_some(),
            "ordinary paragraph input should use the bounded parse path"
        );
        let full = ui::Document::new(Node::column("body", cache.nodes));
        let mut wire = full.clone();
        let reused = ui::incremental::compact(&old, &mut wire);
        assert_eq!(reused.len(), 99);
        let mut retained_text = false;
        wire.root.visit(&mut |node| {
            retained_text |= matches!(&node.kind, ui::Kind::RichText { html } if html.contains("段落 99 正文内容"));
        });
        assert!(!retained_text);
        ui::incremental::restore(&old, &mut wire, &reused).unwrap();
        assert_eq!(wire, full);
    }
    /// Local Chinese edits retain later node identity and exactly match full parsing content/ranges.
    #[test]
    fn paragraph_edit_reuses_following_identity_and_handles_structural_fallback() {
        let mut cache = Parsed::default();
        let old = "第一段。\n\n第二段。\n\n第三段。\n";
        cache.update(old, "zh-CN");
        let id = cache.nodes[2].id.clone();
        cache.update("第一段。\n\n第二段中文。\n\n第三段。\n", "zh-CN");
        assert_eq!(cache.nodes[2].id, id);
        assert_eq!(
            cache.nodes[2].source_range.unwrap().start,
            "第一段。\n\n第二段中文。\n\n".len()
        );
        let next = "第一段。\n\n```\n第二段中文。\n\n第三段。\n";
        cache.update(next, "zh-CN");
        assert_eq!(
            cache.nodes,
            super::super::preview::blocks(next, "zh-CN").unwrap()
        );
    }
}
