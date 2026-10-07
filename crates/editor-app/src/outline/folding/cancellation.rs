//! Real XML parser cancellation must preserve native paste, history and drawing.
//!
//! This tight loop uses the existing dynamic grammar loader and public input-highlighter
//! factory. Package installation and outline navigation remain covered by the actual ZIP tests.
use super::*;
use gpui_base::input::Editor as NativeEditor;
use gpui_kit::{ClipboardItem, TestAppContext, gpui};
use plugin_schema::PluginManifest;

const INITIAL_SOURCE: &str = "<root>\n  <!-- 多行🙂\n       comment body\n       end -->\n  <item id=\"中文🙂\">\n    <leaf name=\"child\"/>\n  </item>\n</root>";

/// Own only temporary preferences; the selected parser still comes from shipped WASM bytes.
struct Fixture {
    store: tempfile::TempDir,
    workspace: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            store: tempfile::tempdir().unwrap(),
            workspace: tempfile::tempdir().unwrap(),
        };
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/xml")
            .canonicalize()
            .unwrap();
        let declaration =
            PluginManifest::parse(&std::fs::read_to_string(root.join("plugin.toml")).unwrap())
                .unwrap();
        crate::language::providers::configure(fixture.store.path(), fixture.workspace.path());
        crate::language::providers::refresh(
            fixture.store.path(),
            vec![(
                declaration.plugin.id,
                root.clone(),
                declaration.language_definitions,
                declaration.highlighters,
            )],
            Vec::new(),
        );
        crate::language::plugins::register_plugin(&root).unwrap();
        fixture
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Withdraw the role before its temporary preferences disappear, including on RED panic.
        crate::language::providers::refresh(self.store.path(), Vec::new(), Vec::new());
        crate::language::plugins::mask_language("xml");
    }
}

/// Render the same Base editor used by documents without asynchronous package discovery.
struct View {
    editor: Entity<EditorState>,
}

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("cancelled-parser-native-source")
            .debug_selector(|| "cancelled-parser-native-source".into())
            .size_full()
            .text_size(px(14.))
            .child(NativeEditor::new(&self.editor))
    }
}

/// Cancel the actual XML foreground parse, then replay native Undo before debounce can replace it.
#[gpui::test]
fn native_xml_cancelled_parser_preserves_paste_undo_redo(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let fixture = Fixture::new();
    let path = fixture.workspace.path().join("history.xml");
    std::fs::write(&path, INITIAL_SOURCE).unwrap();
    // More than 100 parser operations reach its genuine progress callback; wall-clock speed is irrelevant.
    let pasted = format!(
        "<root>\n{}</root>",
        (0..90)
            .map(|index| format!("  <node id=\"{index}\"/>\n"))
            .collect::<String>()
    );
    let mut cancelled = plugin_syntax("xml");
    assert_eq!(cancelled.language().as_ref(), "xml");
    assert!(cancelled.update(None, &Rope::from(INITIAL_SOURCE), None));
    assert!(
        !cancelled.update(None, &Rope::from(pasted.clone()), Some(Duration::ZERO)),
        "the real XML grammar must hit cancellation before the native history regression"
    );
    // Do not reuse the probe: the production Adapter is independently driven by the native clipboard.
    drop(cancelled);

    let (view, visual) = cx.add_window_view(|window, cx| {
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("text")
                .line_number(true)
        });
        editor.update(cx, |editor, cx| {
            editor.set_value(INITIAL_SOURCE, window, cx);
            editor.set_highlighter_factory(
                Rc::new(|language| {
                    let mut syntax = plugin_syntax(language);
                    assert_eq!(syntax.language().as_ref(), "xml");
                    // Complete the short opening document before any controlled cancellation.
                    assert!(syntax.update(None, &Rope::from(INITIAL_SOURCE), None));
                    Some(Box::new(Adapter {
                        language: language.to_owned().into(),
                        syntax: Rc::new(RefCell::new(syntax)),
                        parser_cancelled: Rc::new(Cell::new(false)),
                        folds: Rc::new(RefCell::new(Some(Vec::new()))),
                        generation: Rc::new(Cell::new(0)),
                        pending: None,
                    }))
                }),
                cx,
            );
            editor.set_highlighter("xml", cx);
        });
        View { editor }
    });
    visual.simulate_resize(size(px(480.), px(240.)));
    let editor = visual.update(|window, cx| {
        window.activate_window();
        let editor = view.read(cx).editor.clone();
        editor.update(cx, |editor, cx| {
            editor.set_selected_range(0..editor.text().len(), cx);
            editor.focus(window, cx);
        });
        window.draw(cx).clear(cx);
        cx.write_to_clipboard(ClipboardItem::new_string(pasted.clone()));
        editor
    });
    visual.simulate_keystrokes("ctrl-v");
    visual.update(|window, cx| {
        assert_eq!(editor.read(cx).text().to_string(), pasted);
        // Paint the cancelled tree before Undo, matching the original outline interaction's native frame.
        window.draw(cx).clear(cx);
    });
    // No clock advance: this Undo reaches the cancelled foreground parser before its 150 ms timer.
    visual.simulate_keystrokes("ctrl-z");
    visual.update(|window, cx| {
        assert_eq!(editor.read(cx).text().to_string(), INITIAL_SOURCE);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), INITIAL_SOURCE);
        window.draw(cx).clear(cx);
    });
    visual.simulate_keystrokes("ctrl-y");
    visual.update(|window, cx| {
        assert_eq!(editor.read(cx).text().to_string(), pasted);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), INITIAL_SOURCE);
        assert!(editor.read(cx).range_to_bounds(&(0..0)).is_some());
        window.draw(cx).clear(cx);
    });
    assert!(
        visual
            .debug_bounds("cancelled-parser-native-source")
            .is_some()
    );
}
