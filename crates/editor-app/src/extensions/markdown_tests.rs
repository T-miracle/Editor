//! Markdown packages enter through the public ZIP/Manager boundary and affect native documents.

use super::*;
use gpui_kit::{TestAppContext, gpui};

mod format_toolbar;
mod harness;
mod image_import;
mod image_preview;
mod link_navigation;
mod modes;
mod navigation_safety;
mod range_edits;
mod source_overlay;
mod task_checkboxes;
mod task_safety;

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
    // Grammar lifecycle remains independently testable when the delivered package adds a UI guest.
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest.as_object_mut().unwrap().remove("component");
    manifest.as_object_mut().unwrap().remove("panels");
    manifest["api"] = serde_json::json!({"base":"^1"});
    manifest["permissions"] = serde_json::json!([]);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    language_tests::packages::repack(files).unwrap()
}

/// A delivered guest parses unsaved prose and the host displays the native split without disk reads.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_preview_tracks_unsaved_native_edits_and_reclaims_split(
    cx: &mut TestAppContext,
) {
    use super::composable_tests::{publish, pump};
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/markdown.zip"),
    )
    .unwrap();
    assert!(
        package.manifest.component.is_some(),
        "the preview requires the independently built guest"
    );
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("notes.md");
    let original = "# 中文标题\n\n**粗体** *斜体* ~~删除~~ [链接](next.md)\n\n> 引用\n\n- item\n- [ ] 任务\n\n| 名称 | 数值 |\n| --- | --- |\n| 表格 | 42 |\n\n```rust\nfn main() {}\n```\n\n![替代文字](missing.png)\n";
    std::fs::write(&path, original).unwrap();
    std::fs::write(root.path().join("plain.txt"), "plain").unwrap();
    let mut manager = plugin_runtime::Manager::open(
        root.path().join("runtime"),
        protocol::Environment {
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let mut renderer = images::VectorRenderer::default();
    let workspace = Workspace::open(root.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1400.), px(900.)));
    publish(&mut manager, &mut renderer, &app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    cx.run_until_parked();
    for _ in 0..3 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    let preview = cx.debug_bounds("editor-preview-pane").unwrap();
    assert!(preview.size.width > px(100.) && preview.size.height > px(100.));
    assert!(cx.debug_bounds("editor-preview-divider").is_some());
    let tree = &manager.live["markdown"].views["preview"];
    let serialized = serde_json::to_string(tree).unwrap();
    for expected in [
        "<h1>",
        "<strong>",
        "<em>",
        "<del>",
        "<table>",
        "checkbox",
        "code_block",
        "替代文字",
        "source_range",
    ] {
        assert!(
            serialized.contains(expected),
            "missing rendered syntax: {expected}"
        );
    }
    assert!(tree.source.is_some());
    let mut visible_blocks = 0;
    tree.root.visit(&mut |node| {
        if let Some(range) = &node.source_range {
            assert!(range.start <= range.end && range.end <= original.len());
            assert!(original.is_char_boundary(range.start) && original.is_char_boundary(range.end));
            if matches!(
                node.kind,
                protocol::ui::Kind::RichText { .. } | protocol::ui::Kind::CodeBlock { .. }
            ) {
                // GPUI's test selector API requires static strings; this bounded fixture owns a few labels.
                let selector = Box::leak(format!("plugin-ui-{}", node.id).into_boxed_str());
                let bounds = cx.debug_bounds(selector).unwrap();
                assert!(
                    bounds.size.width > px(0.) && bounds.size.height > px(0.),
                    "block has no native layout: {}",
                    node.id
                );
                visible_blocks += 1;
            }
        }
    });
    assert!(visible_blocks >= 4);
    cx.update(|window, cx| {
        app.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.focus(window, cx))
    });
    cx.simulate_keystrokes("ctrl-a");
    // A single native paste is one edit; simulated multiline typing has several legitimate undo groups.
    cx.update(|_, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("# 未保存中文\n\n实时预览".into()))
    });
    cx.simulate_keystrokes("ctrl-v");
    cx.run_until_parked();
    for _ in 0..2 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    assert!(
        serde_json::to_string(&manager.live["markdown"].views["preview"])
            .unwrap()
            .contains("未保存中文")
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    for _ in 0..2 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    let after_undo = serde_json::to_string(&manager.live["markdown"].views["preview"]).unwrap();
    assert!(
        after_undo.contains("中文标题"),
        "source after undo: {:?}; preview: {after_undo}",
        cx.update(|_, cx| app.read(cx).editor.read(cx).text().to_string())
    );
    cx.simulate_keystrokes("ctrl-y");
    cx.run_until_parked();
    for _ in 0..2 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    assert!(
        serde_json::to_string(&manager.live["markdown"].views["preview"])
            .unwrap()
            .contains("未保存中文")
    );
    cx.simulate_input("追加中文");
    cx.run_until_parked();
    for _ in 0..2 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    assert!(
        serde_json::to_string(&manager.live["markdown"].views["preview"])
            .unwrap()
            .contains("追加中文")
    );
    cx.update(|window, cx| {
        apply_theme(builtin_theme(true), cx);
        app.update(cx, |app, cx| {
            app.open_file(root.path().join("plain.txt"), window, cx)
        });
    });
    cx.run_until_parked();
    for _ in 0..2 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    assert!(cx.debug_bounds("editor-preview-pane").is_none());
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    cx.run_until_parked();
    for _ in 0..2 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    assert!(cx.debug_bounds("editor-preview-pane").is_some());
    // A clean document reload uses the production reconciliation path rather than an editor edit.
    let reload_path = root.path().join("reloaded.md");
    std::fs::write(&reload_path, "# Before reload\n").unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.open_file(reload_path.clone(), window, cx))
    });
    cx.run_until_parked();
    for _ in 0..2 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    let old_tree = manager.live["markdown"].views["preview"].clone();
    let old_source = old_tree.source.clone().unwrap();
    // The delivered image node replaces the historical text-only fallback; empty URIs stay local text.
    let replacement = "# Reloaded heading\n\n![fallback](missing.png)\n\n![empty]()\n";
    std::fs::write(&reload_path, replacement).unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.apply_reconciliation(
                Reconciliation {
                    snapshot: None,
                    documents: vec![(
                        reload_path.canonicalize().unwrap(),
                        Ok(replacement.into()),
                        Instant::now(),
                    )],
                    renames: vec![],
                    native: true,
                },
                window,
                cx,
            )
        });
    });
    cx.run_until_parked();
    for _ in 0..2 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    assert!(
        serde_json::to_string(&manager.live["markdown"].views["preview"])
            .unwrap()
            .contains("Reloaded heading")
    );
    assert!(cx.update(|_, cx| {
        !app.read(cx).tabs[app.read(cx).active_tab_index().unwrap()]
            .session
            .is_dirty()
    }));
    // The real runtime rejects late notifications before invoking the guest or replacing its source.
    let stale = manager
        .event(
            "markdown",
            Some("preview".into()),
            protocol::api::Notification::Preview {
                document: Some(old_source),
                text: "# Obsolete".into(),
            },
        )
        .unwrap_err();
    assert!(
        matches!(stale.downcast_ref::<protocol::api::Failure>(), Some(error) if error.code == protocol::api::ErrorCode::StaleRevision)
    );
    manager
        .event(
            "markdown",
            None,
            protocol::api::Notification::Theme(protocol::Environment {
                locale: "en".into(),
                workspace: root.path().display().to_string(),
                ..Default::default()
            }),
        )
        .unwrap();
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(
        serde_json::to_string(&manager.live["markdown"].views["preview"])
            .unwrap()
            .contains("empty: The image reference has no source.")
    );
    let mut declared_image = false;
    manager.live["markdown"].views["preview"].root.visit(&mut |node| {
        declared_image |= matches!(&node.kind, protocol::ui::Kind::Image { source, alt } if source == "missing.png" && alt == "fallback");
    });
    assert!(declared_image);
    let closed_tree = manager.live["markdown"].views["preview"].clone();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.close_tab(reload_path.canonicalize().unwrap(), window, cx)
        })
    });
    cx.run_until_parked();
    for _ in 0..2 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(reload_path, window, cx)));
    cx.run_until_parked();
    for _ in 0..2 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    assert_ne!(
        closed_tree.source.as_ref().unwrap().id,
        manager.live["markdown"].views["preview"]
            .source
            .as_ref()
            .unwrap()
            .id
    );
    // Delayed worker output still carries the retired identity: it cannot mount native event targets.
    cx.update(|_, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            owner
                .worker
                .state
                .lock()
                .unwrap()
                .views
                .insert("markdown/preview".into(), closed_tree);
            owner.poll(cx);
        });
        app.read(cx).plugin_panels["markdown/preview"]
            .clone()
            .update(cx, |panel, cx| panel.poll(cx));
        assert!(
            app.read(cx).plugin_panels["markdown/preview"]
                .read(cx)
                .current_document()
                .is_none()
        );
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.update(|_, cx| {
        app.read(cx).plugin_panels["markdown/preview"]
            .read(cx)
            .native_ui
            .is_none()
    }));
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(cx.update(|_, cx| {
        app.read(cx).plugin_panels["markdown/preview"]
            .read(cx)
            .native_ui
            .is_some()
    }));
    manager.disable("markdown").unwrap();
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(cx.debug_bounds("editor-preview-pane").is_none());
    manager.uninstall("markdown", false).unwrap();
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(cx.update(|_, cx| app.read(cx).plugin_panels.is_empty()));
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
