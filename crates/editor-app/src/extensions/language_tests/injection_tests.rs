//! Injection declarations must not expose upstream native parsers through installed resource packages.

use super::*;

/// Undeclared, dynamic, or repeated injection targets must fail before a grammar is published.
#[gpui::test]
fn installed_injection_queries_cannot_borrow_undeclared_native_grammars(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.novel-injection");
    let source = "payload = \"json\"\n";
    std::fs::write(&path, source).unwrap();
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
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(path, window, cx)));
    cx.run_until_parked();
    let registry = gpui_kit::component::highlighter::LanguageRegistry::singleton();
    // JSON has an upstream native grammar; rejecting publication must precede any injection lookup.
    assert!(registry.language("json").unwrap().language.is_some());
    for (id, query, allowed_languages, expected_error) in [
        (
            "novel-static-injection",
            "((string) @injection.content (#set! injection.language \"json\"))",
            "[]",
            "injection target must be explicitly declared",
        ),
        (
            "novel-dynamic-injection",
            "(string) @injection.content @injection.language",
            "[]",
            "dynamic injection language captures are unsupported",
        ),
        // An allowed first property must not conceal the undeclared target in a later property.
        (
            "novel-repeated-injection",
            r#"((string) @injection.content
                (#set! injection.language "novel-injection")
                (#set! injection.language "json"))"#,
            "[\"novel-injection\"]",
            "injection target must be explicitly declared",
        ),
    ] {
        let mut files = language_package(id).files;
        let declaration = String::from_utf8(files["plugin.toml"].clone())
            .unwrap()
            .replace("\"novel\"", "\"novel-injection\"");
        files.insert(
            "plugin.toml".into(),
            format!(
                "{declaration}\ninjections = \"injections.scm\"\ninjection_languages = {allowed_languages}\n"
            )
            .into_bytes(),
        );
        files.insert("injections.scm".into(), query.as_bytes().to_vec());
        // ZIP inspection and Manager installation succeed; the production grammar loader rejects it.
        let package = repack(files).unwrap();
        manager.install(&package, Default::default()).unwrap();
        publish_languages(&app, &manager, cx);
        cx.update(|_, cx| {
            let app = app.read(cx);
            assert_eq!(
                app.editor.read(cx).language_name().as_ref(),
                "novel-injection"
            );
            assert_eq!(app.plugin_count(PluginPopupKind::Error, cx), 1);
            assert!(
                app.dynamic_language_status(PluginPopupKind::Error)
                    .iter()
                    .any(|(_, error)| error.contains(expected_error))
            );
        });
        let config = registry.language("novel-injection").unwrap();
        assert!(config.highlights.is_empty() && config.injections.is_empty());
        let mut parser =
            gpui_kit::component::highlighter::SyntaxHighlighter::new("novel-injection");
        parser.update(None, &gpui_base::input::Rope::from(source), None);
        assert!(
            parser.tree().is_none(),
            "unsafe query published a usable parser: {id}"
        );
        manager.uninstall(id, false).unwrap();
        publish_languages(&app, &manager, cx);
    }
}
