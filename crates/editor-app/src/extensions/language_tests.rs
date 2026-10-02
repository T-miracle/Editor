//! Real resource packages must change an already-open editor through the installed contribution path.
use super::*;
use gpui_kit::{TestAppContext, gpui};
pub(super) mod packages;
use packages::{language_package, legacy_rust_package, repack};

/// A delayed legacy load must not overwrite a dynamic choice using the same public language ID.
#[gpui::test]
fn dynamic_selection_survives_legacy_load_and_legacy_returns_after_removal(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("sample.rs"), "answer = 42\n").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let root = workspace.root().join(".runtime-plugin-test");
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let mut manager = plugin_runtime::Manager::open(
        root,
        protocol::Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let legacy = legacy_rust_package();
    manager.install(&legacy, Default::default()).unwrap();
    queue_languages(&app, &manager, cx);
    let mut files = language_package("rust-alternative").files;
    let source = String::from_utf8(files["plugin.toml"].clone())
        .unwrap()
        .replace("\"novel\"", "\"rust\"");
    // Contribute only highlighting; recognition must continue to come from the legacy package.
    let source = format!(
        "{}[[highlighters]]{}",
        source.split("[[language_definitions]]").next().unwrap(),
        source.split("[[highlighters]]").nth(1).unwrap()
    );
    files.insert("plugin.toml".into(), source.into_bytes());
    let alternate = repack(files).unwrap();
    manager.install(&alternate, Default::default()).unwrap();
    queue_languages(&app, &manager, cx);
    cx.run_until_parked();
    let registry = gpui_kit::component::highlighter::LanguageRegistry::singleton();
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .tabs
            .last()
            .unwrap()
            .editor
            .read(cx)
            .language_name()
            .to_string()),
        "rust"
    );
    assert_eq!(
        registry.language("rust").unwrap().highlights.as_ref(),
        std::str::from_utf8(&alternate.files["highlights.scm"]).unwrap()
    );
    // Removing a dynamic owner revalidates the legacy parser instead of leaving its stale loaded flag.
    manager.uninstall("rust-alternative", false).unwrap();
    publish_languages(&app, &manager, cx);
    let manifest = plugin_schema::PluginManifest::parse(
        std::str::from_utf8(&legacy.files["plugin.toml"]).unwrap(),
    )
    .unwrap();
    let query_path = manifest.languages[0].highlights.to_string_lossy();
    assert_eq!(
        registry.language("rust").unwrap().highlights.as_ref(),
        std::str::from_utf8(&legacy.files[query_path.as_ref()]).unwrap()
    );
    // Retiring the old package while a dynamic override remains must not mask the surviving grammar.
    manager.install(&alternate, Default::default()).unwrap();
    publish_languages(&app, &manager, cx);
    manager.uninstall("rust", false).unwrap();
    publish_languages(&app, &manager, cx);
    assert_eq!(
        registry.language("rust").unwrap().highlights.as_ref(),
        std::str::from_utf8(&alternate.files["highlights.scm"]).unwrap()
    );
    manager.uninstall("rust-alternative", false).unwrap();
    publish_languages(&app, &manager, cx);
}

#[gpui::test]
fn unknown_declarative_language_highlights_open_document_without_restart(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.novel");
    std::fs::write(&path, "answer = 42\n").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let root = workspace.root().join(".runtime-plugin-test");
    let mut manager = plugin_runtime::Manager::open(
        root,
        protocol::Environment {
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(path, window, cx)));
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .tabs
            .last()
            .unwrap()
            .editor
            .read(cx)
            .language_name()
            .to_string()),
        "text"
    );
    let package = language_package("novel-primary");
    assert!(package.manifest.component.is_none());
    manager.install(&package, Default::default()).unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let owner = app.extensions.clone();
            owner.update(cx, |panel, cx| {
                panel.worker.state.lock().unwrap().entries = manager.published_entries();
                panel.poll(cx);
            });
            app.sync_runtime_contributions(window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .tabs
            .last()
            .unwrap()
            .editor
            .read(cx)
            .language_name()
            .to_string()),
        "novel"
    );
    // A language label alone is insufficient: the plugin query and WASM parser must be usable.
    let config = gpui_kit::component::highlighter::LanguageRegistry::singleton()
        .language("novel")
        .unwrap();
    assert!(config.language.is_none() && !config.highlights.is_empty());
    let mut parser = gpui_kit::component::highlighter::SyntaxHighlighter::new("novel");
    parser.update(None, &gpui_base::input::Rope::from("answer = 42\n"), None);
    assert!(!parser.tree().unwrap().root_node().has_error());
    // Installing competition must retain the currently working recognition and highlighter.
    manager
        .install(&language_package("novel-alternative"), Default::default())
        .unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.extensions.update(cx, |panel, cx| {
                panel.worker.state.lock().unwrap().entries = manager.published_entries();
                panel.poll(cx);
            });
            app.sync_runtime_contributions(window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .tabs
            .last()
            .unwrap()
            .editor
            .read(cx)
            .language_name()
            .to_string()),
        "novel"
    );
    assert!(
        !gpui_kit::component::highlighter::LanguageRegistry::singleton()
            .language("novel")
            .unwrap()
            .highlights
            .is_empty()
    );
    // The user can choose a different grammar without changing the recognition provider.
    let editor_window = cx.update(|window, _| window.window_handle());
    let trigger = cx.debug_bounds("settings-trigger").unwrap();
    cx.simulate_click(trigger.center(), Default::default());
    let dialog = cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != editor_window)
        .unwrap();
    let form = gpui_kit::VisualTestContext::from_window(dialog, cx).into_mut();
    form.run_until_parked();
    let nav = form.debug_bounds("settings-nav-languages").unwrap();
    form.simulate_click(nav.center(), Default::default());
    form.simulate_resize(size(px(1150.), px(900.)));
    form.run_until_parked();
    let choice = form
        .debug_bounds("provider-highlight:novel-novel-alternative/syntax")
        .unwrap();
    form.simulate_click(choice.center(), Default::default());
    form.run_until_parked();
    assert!(
        form.debug_bounds("provider-selected-highlight:novel-novel-alternative/syntax")
            .is_some()
    );
    assert!(
        form.debug_bounds("provider-selected-recognition:ext:novel-novel-primary/novel")
            .is_some()
    );
    // Project selection overrides the user choice and survives leaving and reopening that workspace.
    let scope = form.debug_bounds("provider-scope").unwrap();
    form.simulate_click(
        point(scope.right() - px(12.), scope.center().y),
        Default::default(),
    );
    form.run_until_parked();
    let primary = form
        .debug_bounds("provider-highlight:novel-novel-primary/syntax")
        .unwrap();
    form.simulate_click(primary.center(), Default::default());
    form.run_until_parked();
    assert!(
        form.debug_bounds("provider-source-project-highlight:novel")
            .is_some()
    );
    let root = directory.path().join(".runtime-plugin-test");
    let another = tempfile::tempdir().unwrap();
    contributions::refresh_for_workspace(&root, another.path()).unwrap();
    form.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.sync_dynamic_languages(cx);
            app.refresh_dialog(cx);
        })
    });
    form.run_until_parked();
    assert!(
        form.debug_bounds("provider-selected-highlight:novel-novel-alternative/syntax")
            .is_some()
    );
    contributions::refresh_for_workspace(&root, directory.path()).unwrap();
    form.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.sync_dynamic_languages(cx);
            app.refresh_dialog(cx);
        })
    });
    form.run_until_parked();
    assert!(
        form.debug_bounds("provider-source-project-highlight:novel")
            .is_some()
    );
    assert!(
        form.debug_bounds("provider-selected-highlight:novel-novel-primary/syntax")
            .is_some()
    );
    // Losing the chosen project provider cannot silently choose between two remaining candidates.
    manager
        .install(&language_package("novel-third"), Default::default())
        .unwrap();
    publish_languages(&app, &manager, form);
    manager.uninstall("novel-primary", false).unwrap();
    publish_languages(&app, &manager, form);
    assert!(
        form.debug_bounds("provider-choice-needed-highlight:novel")
            .is_some()
    );
    assert!(
        form.debug_bounds("provider-choice-needed-recognition:ext:novel")
            .is_some()
    );
    assert_eq!(
        form.update(|_, cx| app
            .read(cx)
            .tabs
            .last()
            .unwrap()
            .editor
            .read(cx)
            .language_name()
            .to_string()),
        "text"
    );
    manager.uninstall("novel-alternative", false).unwrap();
    publish_languages(&app, &manager, form);
    assert!(
        form.debug_bounds("provider-selected-highlight:novel-novel-third/syntax")
            .is_some()
    );
    assert_eq!(
        form.update(|_, cx| app
            .read(cx)
            .tabs
            .last()
            .unwrap()
            .editor
            .read(cx)
            .language_name()
            .to_string()),
        "novel"
    );
    manager.uninstall("novel-third", false).unwrap();
    publish_languages(&app, &manager, form);
    assert!(
        form.debug_bounds("provider-unavailable-highlight:novel")
            .is_some()
    );
    assert_eq!(
        form.update(|_, cx| app
            .read(cx)
            .tabs
            .last()
            .unwrap()
            .editor
            .read(cx)
            .language_name()
            .to_string()),
        "text"
    );
}

/// Publish real Manager output through the same boundary used by the production worker.
fn publish_languages(
    app: &Entity<EditorApp>,
    manager: &plugin_runtime::Manager,
    cx: &mut gpui_kit::VisualTestContext,
) {
    queue_languages(app, manager, cx);
    cx.run_until_parked();
}

/// Leave scheduled grammar work in flight when a lifecycle regression needs a late completion.
fn queue_languages(
    app: &Entity<EditorApp>,
    manager: &plugin_runtime::Manager,
    cx: &mut gpui_kit::VisualTestContext,
) {
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.extensions.update(cx, |panel, cx| {
                panel.worker.state.lock().unwrap().entries = manager.published_entries();
                panel.poll(cx);
            });
            app.sync_runtime_contributions(window, cx);
            app.refresh_dialog(cx);
        })
    });
}

/// Multiple language declarations load independently; failure and late success never revive retired contributions.
#[gpui::test]
fn multiple_languages_isolate_failed_grammars_and_discard_late_uninstalled_loads(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    for name in ["sample.novel", "sample.other"] {
        std::fs::write(directory.path().join(name), "answer = 42\n").unwrap();
    }
    let workspace = Workspace::open(directory.path()).unwrap();
    let root = workspace.root().join(".runtime-plugin-test");
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let mut manager = plugin_runtime::Manager::open(
        root,
        protocol::Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            for name in ["sample.novel", "sample.other"] {
                app.open_file(directory.path().join(name), window, cx);
            }
        })
    });
    cx.run_until_parked();
    let mut files = language_package("novel-multi").files;
    files.get_mut("plugin.toml").unwrap().extend_from_slice(
        br#"
[[language_definitions]]
id = "another-language"
name = "Another language"
extensions = ["other"]
[[highlighters]]
id = "other-syntax"
language = "another-language"
grammar_name = "toml"
grammar = "grammar.wasm"
highlights = "bad.scm"
tree_sitter_abi = 15
"#,
    );
    files.insert("bad.scm".into(), b"(unknown_node) @keyword".to_vec());
    let package = repack(files.clone()).unwrap();
    manager.install(&package, Default::default()).unwrap();
    publish_languages(&app, &manager, cx);
    cx.update(|_, cx| {
        let app = app.read(cx);
        for (extension, expected) in [("novel", "novel"), ("other", "another-language")] {
            let tab = app
                .tabs
                .iter()
                .find(|tab| {
                    tab.session
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == extension)
                })
                .unwrap();
            assert_eq!(tab.editor.read(cx).language_name().as_ref(), expected);
        }
        assert_eq!(app.plugin_count(PluginPopupKind::Error, cx), 1);
    });
    let registry = gpui_kit::component::highlighter::LanguageRegistry::singleton();
    assert!(!registry.language("novel").unwrap().highlights.is_empty());
    assert!(
        registry
            .language("another-language")
            .unwrap()
            .highlights
            .is_empty()
    );
    // Disable and re-enable withdraw and reload both declarations without closing their editors.
    manager.disable("novel-multi").unwrap();
    publish_languages(&app, &manager, cx);
    assert!(registry.language("novel").unwrap().highlights.is_empty());
    manager.enable("novel-multi").unwrap();
    publish_languages(&app, &manager, cx);
    assert!(!registry.language("novel").unwrap().highlights.is_empty());
    // Start a valid replacement, retire it before queued background work can finish, then drain tasks.
    files.insert("bad.scm".into(), files["highlights.scm"].clone());
    let source = String::from_utf8(files["plugin.toml"].clone())
        .unwrap()
        .replace("1.0.0", "1.0.1");
    files.insert("plugin.toml".into(), source.into_bytes());
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["version"] = "1.0.1".into();
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    manager
        .install(&repack(files).unwrap(), Default::default())
        .unwrap();
    queue_languages(&app, &manager, cx);
    manager.uninstall("novel-multi", false).unwrap();
    queue_languages(&app, &manager, cx);
    cx.run_until_parked();
    assert!(registry.language("novel").unwrap().highlights.is_empty());
    assert!(
        registry
            .language("another-language")
            .unwrap()
            .highlights
            .is_empty()
    );
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .tabs
            .iter()
            .all(|tab| tab.editor.read(cx).language_name().as_ref() == "text")
    }));
}
