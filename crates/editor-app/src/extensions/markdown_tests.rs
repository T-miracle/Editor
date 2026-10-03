//! Markdown packages enter through the public ZIP/Manager boundary and affect native documents.

use super::*;
use gpui_kit::{TestAppContext, gpui};

/// Inspect the editable package resources without inventing a special host API for this plugin.
fn language_package() -> Package {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/markdown");
    let mut files = BTreeMap::new();
    for path in [
        "manifest.json",
        "plugin.toml",
        "grammar/markdown.wasm",
        "grammar/markdown_inline.wasm",
        "queries/highlights.scm",
        "queries/injections.scm",
        "queries/inline.scm",
    ] {
        files.insert(path.into(), std::fs::read(root.join(path)).unwrap());
    }
    language_tests::packages::repack(files).unwrap()
}

/// Installing a real resource package restores a previously plain document without reopening it.
#[gpui::test]
fn markdown_language_package_restores_open_document_and_retires_highlighting(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.MD");
    std::fs::write(
        &path,
        "# 中文标题\n\n- [ ] task\n\n```rust\nfn main() {}\n```\n",
    )
    .unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join(".runtime-plugin-test"),
        protocol::Environment {
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    cx.run_until_parked();
    let language = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_, cx| app.read(cx).editor.read(cx).language_name().to_string())
    };
    assert_eq!(language(cx), "text");
    manager
        .install(&language_package(), Default::default())
        .unwrap();
    language_tests::publish_languages(&app, &manager, cx);
    assert_eq!(language(cx), "markdown");
    cx.update(|_, cx| {
        let app = app.read(cx);
        assert!(
            app.dynamic_languages
                .entries
                .iter()
                .all(|(_, state)| matches!(state, Ok(true))),
            "{:?}",
            app.dynamic_languages
                .entries
                .iter()
                .map(|(p, state)| (&p.declaration.language, state))
                .collect::<Vec<_>>()
        );
    });
    // Rendered highlight spans must include prose formatting, not just a recognized language label.
    let source = "# title\n\n**bold** *italic* `code` ~~gone~~ [link](next.md)\n";
    let highlight = |language: &str| {
        let mut parser = gpui_kit::component::highlighter::SyntaxHighlighter::new(language);
        parser.update(None, &gpui_base::input::Rope::from(source), None);
        parser
            .styles(
                &(0..source.len()),
                gpui_kit::component::highlighter::HighlightTheme::default_dark().as_ref(),
            )
            .into_iter()
            .filter(|(_, style)| *style != gpui_kit::HighlightStyle::default())
            .collect::<Vec<_>>()
    };
    let spans = highlight("markdown");
    for word in ["title", "bold", "italic", "code", "gone", "link"] {
        let offset = source.find(word).unwrap();
        assert!(
            spans.iter().any(|(range, _)| range.contains(&offset)),
            "missing visible highlight for {word}"
        );
    }
    manager.disable("markdown").unwrap();
    language_tests::publish_languages(&app, &manager, cx);
    assert_eq!(language(cx), "text");
    assert!(highlight("text").is_empty());
    assert!(highlight("markdown").is_empty());
    assert!(highlight("markdown_inline").is_empty());
    manager.enable("markdown").unwrap();
    language_tests::publish_languages(&app, &manager, cx);
    assert_eq!(language(cx), "markdown");
    assert!(!highlight("markdown").is_empty());
    manager.uninstall("markdown", false).unwrap();
    language_tests::publish_languages(&app, &manager, cx);
    assert_eq!(language(cx), "text");
    assert!(highlight("text").is_empty());
    assert!(highlight("markdown").is_empty());
    assert!(highlight("markdown_inline").is_empty());
}
