//! Actual document and command consumers share captured resource metadata without sharing authority.
use super::*;
use crate::extensions::composable_tests;
use plugin_runtime::plugin_protocol::commands::Location;

/// An ordinary menu consumer reports only the context delivered by the production command route.
fn menu_consumer() -> Package {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example-0.18.0.zip");
    let mut files = Package::read(&path).unwrap().files;
    let mut manifest: Json = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!("document-menu-reader");
    manifest["commands"].as_array_mut().unwrap().push(json!({
        "id":"menu-context", "title":"Read document context", "menus":[
            {"location":"editor", "enabled_when":{"writable":false}},
            {"location":"selection", "enabled_when":{"writable":false}},
            {"location":"tab", "enabled_when":{"writable":false}}
        ]
    }));
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    Package::from_files(files).unwrap()
}

/// A local left session is captured by identity, even when its relative name also exists in cwd.
#[gpui::test]
#[ignore = "build history-preview and capability-example with the integrated host first"]
fn local_comparison_menu_keeps_exact_document_context_and_rejects_stale_versions(
    cx: &mut TestAppContext,
) {
    crate::tests::with_shortcut_editor(cx, false, Vec::new(), |visual, app, path| {
        let left_path = path.parent().unwrap().join("Cargo.toml");
        let left_original = "左侧本地文档😀\r\nvalue = true\r\n";
        std::fs::write(&left_path, left_original).unwrap();
        let (_runtime, mut manager) = manager(path.parent().unwrap());
        let package = menu_consumer();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let right = visual.update(|_, cx| app.read(cx).plugin_document_info(0, cx).unwrap());
        let left = visual.update(|window, cx| {
            app.update(cx, |owner, cx| {
                owner.open_file(left_path.clone(), window, cx);
                owner.plugin_document_info(1, cx).unwrap()
            })
        });
        assert_eq!(left.document.path, "Cargo.toml");
        invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"compare_documents", "left":left.document, "right":right.document}),
        );
        let mut renderer = images::VectorRenderer::default();
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        let bounds = visual.debug_bounds("document-diff-left").unwrap();
        let click = bounds.origin + gpui_kit::point(gpui_kit::px(20.), gpui_kit::px(60.));
        visual.simulate_mouse_down(click, gpui_kit::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(click, gpui_kit::MouseButton::Left, Default::default());
        visual.simulate_keystrokes("ctrl-a");
        // This ordinary captured contribution will later be replayed after only its left revision changes.
        let (target, row) = visual.update(|_, cx| {
            let owner = app.read(cx);
            let target = owner.plugin_document_menu_target(&left.document);
            let row = owner
                .plugin_menu_entries(&target, &[Location::Editor], cx)
                .into_iter()
                .find(|row| row.command == "menu-context")
                .unwrap();
            (target, row)
        });
        visual.simulate_mouse_down(click, gpui_kit::MouseButton::Right, Default::default());
        visual.simulate_mouse_up(click, gpui_kit::MouseButton::Right, Default::default());
        visual.run_until_parked();
        let menu = visual
            .debug_bounds("native-menu-document-command-0")
            .expect("the local left pane must expose its readonly command");
        visual.simulate_click(menu.center(), Default::default());
        composable_tests::pump(&mut manager, &app, visual);
        let protocol::ui::Kind::Text { text } =
            &manager.live[&package.manifest.id].views["welcome"]
                .root
                .kind
        else {
            panic!("the real menu click must deliver the local left context")
        };
        let context: protocol::commands::Context = serde_json::from_str(text).unwrap();
        assert_eq!(context.language.as_deref(), Some(left.language.as_str()));
        assert_eq!(context.path.as_deref(), Some("Cargo.toml"));
        assert_eq!(context.extension.as_deref(), Some("toml"));
        assert!(context.has_selection);
        assert!(!context.writable);
        assert!(!context.directory);
        let receipt = text.clone();

        // Close via the real button, then edit that same left EditorState through native input.
        let close = visual.update(|window, _| {
            gpui_base::test_support::snapshots(window)
                .into_iter()
                .find(|node| {
                    node.role() == Some(gpui_kit::Role::Button)
                        && node.label() == Some(t!("editor.close_diff").as_ref())
                        && node.visible()
                })
                .unwrap()
                .bounds()
                .center()
        });
        visual.simulate_click(close, Default::default());
        visual.update(|window, cx| app.update(cx, |owner, cx| owner.activate_tab(1, window, cx)));
        visual.simulate_keystrokes("ctrl-a");
        visual.simulate_input("左侧已更新😀\r\n");
        visual.run_until_parked();
        let updated = visual.update(|_, cx| app.read(cx).plugin_document_info(1, cx).unwrap());
        assert_eq!(left.document.id, updated.document.id);
        assert_eq!(left.document.path, updated.document.path);
        assert_ne!(left.document, updated.document);
        // Restore readonly conditions with a new comparison: rejection must depend on the old version,
        // rather than merely succeeding because the formerly readonly source became editable.
        invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"compare_documents", "left":updated.document, "right":right.document}),
        );
        visual.update(|window, cx| {
            app.update(cx, |owner, cx| {
                owner.invoke_plugin_menu(&row, &target, window, cx)
            })
        });
        composable_tests::pump(&mut manager, &app, visual);
        assert_eq!(
            manager.live[&package.manifest.id].views["welcome"]
                .root
                .kind,
            protocol::ui::Kind::Text { text: receipt }
        );
        visual.update(|_, cx| {
            let owner = app.read(cx);
            assert_eq!(
                owner.editor.read(cx).value().to_string(),
                "original on disk"
            );
            assert_eq!(
                owner.tabs[1]
                    .text
                    .as_ref()
                    .unwrap()
                    .editor
                    .read(cx)
                    .value()
                    .to_string(),
                "左侧已更新😀\r\n"
            );
        });
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original on disk");
        assert_eq!(std::fs::read_to_string(left_path).unwrap(), left_original);
    });
}

/// The visible left comparison pane supplies its own readonly context through a real menu click.
#[gpui::test]
#[ignore = "build history-preview and capability-example with the integrated host first"]
fn comparison_plugin_menu_uses_left_context_and_retires_with_its_view(cx: &mut TestAppContext) {
    crate::tests::with_shortcut_editor(cx, false, Vec::new(), |visual, app, path| {
        let (_runtime, mut manager) = manager(path.parent().unwrap());
        let package = menu_consumer();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let right = visual.update(|_, cx| app.read(cx).plugin_document_info(0, cx).unwrap());
        let api::EditorValue::DocumentOpened(left) = invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"open_virtual_document", "title":"readonly.js", "language":"rust", "text":"左侧内容😀\r\n"}),
        ) else {
            panic!("virtual resource must open")
        };
        invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"compare_documents", "left":left.document, "right":right.document}),
        );
        let mut renderer = images::VectorRenderer::default();
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        let bounds = visual.debug_bounds("document-diff-left").unwrap();
        let click = bounds.origin + gpui_kit::point(gpui_kit::px(20.), gpui_kit::px(60.));
        visual.simulate_mouse_down(click, gpui_kit::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(click, gpui_kit::MouseButton::Left, Default::default());
        visual.simulate_keystrokes("ctrl-a");
        let right_text = visual.update(|_, cx| app.read(cx).editor.read(cx).value().to_string());
        visual.simulate_mouse_down(click, gpui_kit::MouseButton::Right, Default::default());
        visual.simulate_mouse_up(click, gpui_kit::MouseButton::Right, Default::default());
        visual.run_until_parked();
        let menu = visual
            .debug_bounds("native-menu-document-command-0")
            .expect("the clicked readonly left pane must expose plugin commands");
        visual.simulate_click(menu.center(), Default::default());
        composable_tests::pump(&mut manager, &app, visual);
        let protocol::ui::Kind::Text { text } =
            &manager.live[&package.manifest.id].views["welcome"]
                .root
                .kind
        else {
            panic!("context receipt expected")
        };
        let context: protocol::commands::Context = serde_json::from_str(text).unwrap();
        assert_eq!(context.language.as_deref(), Some("rust"));
        assert_eq!(context.path, None);
        assert_eq!(context.extension, None);
        assert!(context.has_selection);
        assert!(!context.writable);
        visual.update(|_, cx| {
            assert_eq!(app.read(cx).editor.read(cx).value().to_string(), right_text)
        });

        // A stale popup must dismiss before its origin pane disappears, so focus restoration
        // cannot resurrect an unmounted left handle after the comparison has already retired.
        visual.simulate_mouse_down(click, gpui_kit::MouseButton::Right, Default::default());
        visual.simulate_mouse_up(click, gpui_kit::MouseButton::Right, Default::default());
        visual.run_until_parked();
        assert!(
            visual
                .debug_bounds("native-menu-document-command-0")
                .is_some()
        );
        invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"refresh_virtual_document", "document":left.document, "text":"刷新后的左侧😀\r\n"}),
        );
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            let owner = app.read(cx);
            assert!(owner.document_comparison.is_none());
            assert!(owner.editor.read(cx).focus_handle(cx).is_focused(window));
        });
        assert!(
            visual
                .debug_bounds("native-menu-document-command-0")
                .is_none()
        );
        visual.simulate_keystrokes("ctrl-a");
        visual.simulate_input("菜单退役后仍可输入😀");
        visual.run_until_parked();
        visual.update(|_, cx| {
            assert_eq!(
                app.read(cx).editor.read(cx).value().to_string(),
                "菜单退役后仍可输入😀"
            )
        });
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original on disk");
    });
}

/// A virtual target retains its own language/version even after another native tab becomes active.
#[gpui::test]
#[ignore = "build history-preview and capability-example with the integrated host first"]
fn virtual_plugin_menus_capture_resource_context_and_reject_stale_targets(cx: &mut TestAppContext) {
    crate::tests::with_shortcut_editor(cx, false, Vec::new(), |visual, app, path| {
        let (_runtime, mut manager) = manager(path.parent().unwrap());
        let package = menu_consumer();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let api::EditorValue::DocumentOpened(opened) = invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"open_virtual_document", "title":"preview.js", "language":"rust", "text":"历史内容😀\r\n"}),
        ) else {
            panic!("virtual resource must open")
        };
        let mut renderer = images::VectorRenderer::default();
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        let (target, row) = visual.update(|_, cx| {
            let owner = app.read(cx);
            let target = owner.plugin_menu_target(Path::new(&opened.document.path));
            let context = owner.plugin_menu_context(&target, cx);
            assert_eq!(context.language.as_deref(), Some("rust"));
            assert_eq!(context.path, None);
            assert_eq!(context.extension, None);
            assert!(!context.writable);
            assert!(!context.directory);
            let row = owner
                .plugin_menu_entries(&target, &[Location::Tab], cx)
                .into_iter()
                .find(|row| row.command == "menu-context")
                .unwrap();
            (target, row)
        });
        // Activate the local right document, then invoke the captured background virtual target.
        visual.update(|window, cx| app.update(cx, |owner, cx| owner.activate_tab(0, window, cx)));
        visual.run_until_parked();
        visual.update(|window, cx| {
            app.update(cx, |owner, cx| {
                owner.invoke_plugin_menu(&row, &target, window, cx)
            })
        });
        composable_tests::pump(&mut manager, &app, visual);
        let protocol::ui::Kind::Text { text } =
            &manager.live[&package.manifest.id].views["welcome"]
                .root
                .kind
        else {
            panic!("context receipt expected")
        };
        let context: protocol::commands::Context = serde_json::from_str(text).unwrap();
        assert_eq!(context.language.as_deref(), Some("rust"));
        assert!(!context.writable);
        assert_eq!(context.path, None);
        let receipt = text.clone();
        invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"refresh_virtual_document", "document":opened.document, "text":"刷新内容😀\r\n"}),
        );
        visual.update(|window, cx| {
            app.update(cx, |owner, cx| {
                owner.invoke_plugin_menu(&row, &target, window, cx)
            })
        });
        composable_tests::pump(&mut manager, &app, visual);
        assert_eq!(
            manager.live[&package.manifest.id].views["welcome"]
                .root
                .kind,
            protocol::ui::Kind::Text { text: receipt }
        );
        // Once closed, recapturing the same resource URI remains descriptive and unusable.
        visual.update(|window, cx| {
            app.update(cx, |owner, cx| {
                owner.close_tab(PathBuf::from(&opened.document.path), window, cx);
                let retired = owner.plugin_menu_target(Path::new(&opened.document.path));
                let context = owner.plugin_menu_context(&retired, cx);
                assert_eq!(context.language, None);
                assert_eq!(context.extension, None);
                assert_eq!(context.path, None);
                assert!(!context.writable);
                assert!(!context.directory);
            });
        });
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original on disk");
    });
}
