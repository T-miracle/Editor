//! One native highlighter adapter arbitrates structure and grammar folds, avoiding asynchronous candidate overwrites.
use crate::*;
use gpui_base::input::{FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, Rope};
use gpui_kit::{SharedString, component::highlighter::SyntaxHighlighter};
use std::{cell::RefCell, ops::Range};

/// Some candidates means structure owns folds (including an empty pending result); None restores grammar folds.
#[derive(Clone)]
pub(super) struct Controller(Rc<RefCell<Option<Vec<FoldRange>>>>);

impl Controller {
    /// Install only when an editor first receives structure. Unrelated language editors retain their original adapter.
    pub(super) fn attach(editor: &Entity<EditorState>, cx: &mut App) -> Self {
        let controller = Self(Rc::new(RefCell::new(Some(Vec::new()))));
        let folds = controller.0.clone();
        editor.update(cx, |editor, cx| {
            let language = editor.language_name();
            editor.set_highlighter_factory(
                Rc::new(move |language| {
                    Some(Box::new(Adapter {
                        language: language.to_owned().into(),
                        syntax: Rc::new(RefCell::new(plugin_syntax(language))),
                        folds: folds.clone(),
                        generation: Rc::new(Cell::new(0)),
                        pending: None,
                    }))
                }),
                cx,
            );
            // Discard the old adapter and its pending grammar writer before this factory takes ownership.
            editor.set_highlighter(language, cx);
        });
        controller
    }

    /// A revision change revokes old structure candidates before another native frame can fold them.
    pub(super) fn replace(
        &self,
        folds: Vec<FoldRange>,
        editor: &Entity<EditorState>,
        cx: &mut App,
    ) {
        *self.0.borrow_mut() = Some(folds.clone());
        editor.update(cx, |editor, cx| {
            editor.apply_highlighter_fold_candidates(folds, cx)
        });
    }

    /// Retiring this provider restores only this editor's grammar candidates, without clearing another document.
    pub(super) fn detach(&self, editor: &Entity<EditorState>, cx: &mut App) {
        *self.0.borrow_mut() = None;
        editor.update(cx, |editor, cx| {
            // Native refresh reparses/queries the same dynamic grammar while preserving its editor and Undo owner.
            let language = editor.language_name();
            editor.set_highlighter(language, cx);
        });
    }
}

/// Delegate dynamic grammar/styles to the public SyntaxHighlighter; only candidate arbitration is host-owned.
struct Adapter {
    language: SharedString,
    syntax: Rc<RefCell<SyntaxHighlighter>>,
    folds: Rc<RefCell<Option<Vec<FoldRange>>>>,
    generation: Rc<Cell<u64>>,
    pending: Option<gpui_kit::Task<()>>,
}

impl InputHighlighter for Adapter {
    fn language(&self) -> SharedString {
        self.language.clone()
    }

    /// Short incremental parses stay on the UI thread; slower immutable parses never publish grammar folds over structure.
    fn update(
        &mut self,
        edit: Option<InputEdit>,
        text: &Rope,
        _: bool,
        window: &mut Window,
        cx: &mut Context<EditorState>,
    ) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.pending.take();
        // Base also passes a zero-range edit while initializing/refreshing a highlighter, without changing the document.
        // Only actual replacements revoke the revision's candidates; a later native parse must preserve accepted structure.
        let source_changed = edit.as_ref().is_some_and(|edit| {
            edit.old_end_byte != edit.start_byte || edit.new_end_byte != edit.start_byte
        });
        if source_changed && self.folds.borrow().is_some() {
            *self.folds.borrow_mut() = Some(Vec::new());
        }
        let edit = edit.map(|edit| tree_sitter::InputEdit {
            start_byte: edit.start_byte,
            old_end_byte: edit.old_end_byte,
            new_end_byte: edit.new_end_byte,
            start_position: tree_sitter::Point::new(
                edit.start_position.row,
                edit.start_position.column,
            ),
            old_end_position: tree_sitter::Point::new(
                edit.old_end_position.row,
                edit.old_end_position.column,
            ),
            new_end_position: tree_sitter::Point::new(
                edit.new_end_position.row,
                edit.new_end_position.column,
            ),
        });
        if self
            .syntax
            .borrow_mut()
            .update(edit, text, Some(Duration::from_millis(2)))
        {
            return;
        }
        let language = self.language.clone();
        let syntax = self.syntax.clone();
        let folds = self.folds.clone();
        let generation = self.generation.clone();
        let requested = generation.get();
        let text = text.clone();
        let grammar_epoch = crate::language::code_highlighting::epoch();
        self.pending = Some(cx.spawn_in(window, async move |editor, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let source = text.clone();
            let parsed = cx
                .background_executor()
                .spawn(async move {
                    let mut parsed = plugin_syntax(&language);
                    parsed.update(None, &source, Some(Duration::from_millis(100)));
                    parsed
                })
                .await;
            // Editor incarnation, source, adapter generation and grammar selection all guard background completion.
            if generation.get() != requested
                || grammar_epoch != crate::language::code_highlighting::epoch()
            {
                return;
            }
            let _ = editor.update(cx, |editor, cx| {
                if editor.text() != &text {
                    return;
                }
                *syntax.borrow_mut() = parsed;
                if folds.borrow().is_none() {
                    editor.apply_highlighter_fold_candidates(grammar_folds(&syntax.borrow()), cx);
                } else {
                    cx.notify();
                }
            });
        }));
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        self.syntax.borrow().styles(range, resolver)
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        self.folds
            .borrow()
            .clone()
            .unwrap_or_else(|| grammar_folds(&self.syntax.borrow()))
    }
}

/// Structure alone never authorizes a built-in parser; only the selected dynamic grammar may produce styles.
fn plugin_syntax(language: &str) -> SyntaxHighlighter {
    let selected = crate::language::providers::grammars()
        .iter()
        .any(|provider| provider.declaration.language == language);
    SyntaxHighlighter::new(if selected { language } else { "text" })
}

/// Preserve Base's generic named-node fallback for editors whose independent structure provider is removed.
fn grammar_folds(syntax: &SyntaxHighlighter) -> Vec<FoldRange> {
    let mut ranges = Vec::new();
    if let Some(tree) = syntax.tree() {
        let root = tree.root_node();
        let mut cursor = root.walk();
        if cursor.goto_first_child() {
            'walk: loop {
                let node = cursor.node();
                let start = node.start_position().row;
                let end = node.end_position().row;
                if node.is_named() && end.saturating_sub(start) >= 2 {
                    ranges.push(FoldRange::new(start, end));
                    if cursor.goto_first_child() {
                        continue;
                    }
                }
                // Source trees are not bounded by the structure reply's 128 levels. Cursor navigation needs no Rust recursion.
                // Skip anonymous/short subtrees exactly as Base's named-child collector does.
                loop {
                    if cursor.goto_next_sibling() {
                        break;
                    }
                    if !cursor.goto_parent() || cursor.node() == root {
                        break 'walk;
                    }
                }
            }
        }
    }
    ranges.sort_by_key(|range| range.start_line);
    ranges.dedup_by_key(|range| range.start_line);
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real dynamically loaded grammar may produce a tree much deeper than the independent structure contract.
    #[test]
    fn grammar_folds_walk_deep_source_without_recursive_stack_growth() {
        std::thread::Builder::new()
            .name("bounded-grammar-fold-walk".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/rust");
                crate::language::plugins::register_plugin(&root).unwrap();
                let depth = 1000;
                // Rust blocks produce genuinely nested syntax here; very deep XML recovers into a shallow error tree.
                let source = "fn main() {\n".to_owned() + &"{\n".repeat(depth) + &"}\n".repeat(depth) + "}\n";
                let mut syntax = SyntaxHighlighter::new("rust");
                assert_eq!(syntax.language().as_ref(), "rust");
                assert!(syntax.update(None, &Rope::from(source), None));
                let ranges = grammar_folds(&syntax);
                // Inspect depth through TreeCursor too, so the actual fixture proves this boundary without recursive printing.
                let tree = syntax.tree().expect("the dynamic parser must produce a tree");
                let root = tree.root_node();
                let mut cursor = root.walk();
                let mut maximum_depth = 0;
                let mut first_nodes = Vec::new();
                loop {
                    maximum_depth = maximum_depth.max(cursor.depth());
                    let node = cursor.node();
                    if first_nodes.len() < 8 {
                        first_nodes.push((node.kind(), node.is_named(), node.start_position().row, node.end_position().row));
                    }
                    if cursor.goto_first_child() {
                        continue;
                    }
                    loop {
                        if cursor.goto_next_sibling() {
                            break;
                        }
                        if !cursor.goto_parent() {
                            break;
                        }
                    }
                    if cursor.node() == root {
                        break;
                    }
                }
                assert!(maximum_depth > 128, "actual tree depth={maximum_depth}; first nodes={first_nodes:?}");
                assert!(
                    ranges.len() > 128,
                    "the actual tree must exceed the structure budget; folds={}; depth={maximum_depth}; first nodes={first_nodes:?}", ranges.len()
                );
                assert_eq!(ranges[0].start_line, 0);
                assert!(ranges[0].end_line >= depth * 2 - 1);
                assert!(
                    ranges
                        .windows(2)
                        .all(|pair| pair[0].start_line < pair[1].start_line)
                );
                // Cursors never enter owned recursive Rust nodes; dropping the public grammar tree remains bounded too.
                drop(cursor);
                drop(syntax);
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
