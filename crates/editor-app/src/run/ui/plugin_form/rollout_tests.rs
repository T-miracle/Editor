//! Real Cargo/Shell packages enter the same native window and execution path as external providers.
use crate::extensions::native_configuration_tests::{Driver, click, edit, fixture, open_form};
use crate::*;
use gpui_kit::{TestAppContext, gpui};
use plugin_runtime::{Manager, Package, plugin_protocol};
#[path = "../../../../../plugin-runtime/tests/support/debugger_packages.rs"]
mod debugger_packages;

/// A minimal genuine Cargo project keeps toolchain acceptance bounded and network independent.
fn project(root: &Path, source: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"configuration-rollout\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/main.rs"), source).unwrap();
}
fn rust_package(manager: &mut Manager, identity: &str) {
    let mut package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/rust.zip"),
    )
    .unwrap();
    if identity != "rust" {
        // Ordinary package metadata exercises identity replacement without changing guest or host code.
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&package.files["manifest.json"]).unwrap();
        manifest["id"] = serde_json::json!(identity);
        let contribution = String::from_utf8(package.files["plugin.toml"].clone())
            .unwrap()
            .replace("id = \"rust\"", &format!("id = \"{identity}\""));
        package
            .files
            .insert("plugin.toml".into(), contribution.into_bytes());
        package = debugger_packages::package(package.files, manifest);
    }
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
}
/// Reach a control by its real wheel scroller rather than dispatching to an off-screen hit region.
fn reveal(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    for _ in 0..30 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let viewport = cx.debug_bounds("plugin-ui-form-scroll").unwrap();
        let center = cx.debug_bounds(selector).unwrap().center();
        if viewport.contains(&center) {
            return;
        }
        cx.simulate_event(gpui_kit::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui_kit::ScrollDelta::Lines(point(
                0.,
                if center.y < viewport.top() { 3. } else { -3. },
            )),
            ..Default::default()
        });
        cx.run_until_parked();
    }
    panic!("native form cannot scroll to {selector}");
}
fn chosen(app: &Entity<EditorApp>, cx: &mut gpui_kit::VisualTestContext) -> String {
    cx.update(|_, cx| {
        app.read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .plugin
            .as_ref()
            .unwrap()
            .selected
            .clone()
            .unwrap()
    })
}
fn settle(
    driver: &mut Driver,
    manager: &mut Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) {
    driver.wait(manager, app, cx, |cx| {
        cx.update(|_, cx| {
            !app.read(cx)
                .run_form
                .as_ref()
                .unwrap()
                .read(cx)
                .plugin
                .as_ref()
                .unwrap()
                .busy()
        })
    });
}
fn save(
    driver: &mut Driver,
    manager: &mut Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    parent: gpui_kit::AnyWindowHandle,
) {
    click(cx, "run-config-save");
    // Save can remove the owned dialog during a receipt; continue painting its surviving parent.
    *cx = gpui_kit::VisualTestContext::from_window(parent.into(), &cx.cx);
    driver.wait(manager, app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_form.is_none())
    });
    driver.frame(manager, app, cx);
}
fn add(
    driver: &mut Driver,
    manager: &mut Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    selector: &'static str,
) -> String {
    click(cx, "run-config-add");
    driver.wait(manager, app, cx, |cx| cx.debug_bounds(selector).is_some());
    click(cx, selector);
    driver.wait(manager, app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    settle(driver, manager, app, cx);
    chosen(app, cx)
}

#[gpui::test]
#[ignore = "build the actual rust/terminal packages and configuration examples via public SDK"]
fn plugin_configuration_rollout_cargo_and_local_shell_execute(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    project(
        root.path(),
        "fn main() { let args: Vec<_> = std::env::args().skip(1).collect(); std::fs::write(\"cargo-args.txt\", format!(\"{args:?}\")).unwrap(); }\n",
    );
    let (mut manager, app, cx) = fixture(cx, root.path());
    rust_package(&mut manager, "rust");
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let parent = open_form(&app, cx);
    let cargo = add(&mut driver, &mut manager, &app, cx, "run-template-rust-run");
    assert!(cx.update(|_, cx| {
        let state = app.read(cx).run_form.as_ref().unwrap().read(cx).plugin.as_ref().unwrap();
        let plugin_protocol::ui::Kind::Scroll { content } = &state.documents[&cargo].root.kind else { return false };
        let plugin_protocol::ui::Kind::Column { children } = &content.kind else { return false };
        children.iter().any(|node| node.id == "program" && matches!(&node.kind, plugin_protocol::ui::Kind::Text { text } if text == "cargo"))
    }), "the provider's command is an actual read-only native text node");
    // The subcommand and all options are ordinary editable argv entries, not a fixed command token.
    edit(cx, "plugin-ui-arg-0", "build");
    settle(&mut driver, &mut manager, &app, cx);
    edit(cx, "plugin-ui-arg-0", "run");
    settle(&mut driver, &mut manager, &app, cx);
    for (index, value) in [(2, "--"), (3, "two words"), (4, "中文 & ; \"quotes\"")] {
        reveal(cx, "plugin-ui-arg-add");
        click(cx, "plugin-ui-arg-add");
        settle(&mut driver, &mut manager, &app, cx);
        let selector = format!("plugin-ui-arg-{index}").leak();
        reveal(cx, selector);
        edit(cx, selector, value);
        settle(&mut driver, &mut manager, &app, cx);
    }
    assert!(
        driver.launches.is_empty(),
        "adding and editing must not launch the configured command"
    );
    save(&mut driver, &mut manager, &app, cx, parent);
    click(cx, "run-build");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| !app.read(cx).run_controls.is_preparing(&cargo))
            && root
                .path()
                .join("target/release/configuration-rollout.exe")
                .exists()
    });
    assert!(
        !root.path().join("cargo-args.txt").exists(),
        "Build must not run the final program"
    );
    click(cx, "run-start");
    driver.wait(&mut manager, &app, cx, |_| {
        root.path().join("cargo-args.txt").exists()
    });
    assert_eq!(
        std::fs::read_to_string(root.path().join("cargo-args.txt")).unwrap(),
        format!("{:?}", ["two words", "中文 & ; \"quotes\""])
    );
    assert_eq!(
        driver.launches.len(),
        2,
        "Run must not duplicate Cargo's own build with a host prebuild"
    );
    let parent = open_form(&app, cx);
    let shell = add(
        &mut driver,
        &mut manager,
        &app,
        cx,
        "run-template-terminal-PowerShell",
    );
    let script = "Set-Content -LiteralPath 'shell-result.txt' -Value '原生脚本' -Encoding UTF8\nWrite-Output 'done'";
    reveal(cx, "plugin-ui-script");
    edit(cx, "plugin-ui-script", script);
    settle(&mut driver, &mut manager, &app, cx);
    save(&mut driver, &mut manager, &app, cx, parent);
    click(cx, "run-start");
    driver.wait(&mut manager, &app, cx, |_| {
        root.path().join("shell-result.txt").exists()
    });
    assert!(
        std::fs::read_to_string(root.path().join("shell-result.txt"))
            .unwrap()
            .contains("原生脚本")
    );
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .configuration_set()
            .plugin_configurations
            .contains_key(&shell)
    }));
    manager.shutdown();
}

/// Applicability is actual guest policy; a catalog lookup must not run a Cargo or Shell process.
#[gpui::test]
#[ignore = "build real rust/terminal packages and independent configuration examples first"]
fn plugin_configuration_rollout_templates_filter_tools_without_execution(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(cx, root.path());
    rust_package(&mut manager, "rust");
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let existing_shells = manager.live["terminal"].process_count();
    open_form(&app, cx);
    click(cx, "run-config-add");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| {
            app.read(cx)
                .run_form
                .as_ref()
                .unwrap()
                .read(cx)
                .plugin
                .as_ref()
                .unwrap()
                .catalog
                .iter()
                .any(|(provider, _)| provider == "rust")
        })
    });
    let catalog = cx.update(|_, cx| {
        app.read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .plugin
            .as_ref()
            .unwrap()
            .catalog
            .clone()
    });
    let cargo: Vec<_> = catalog
        .iter()
        .filter(|(provider, _)| provider == "rust")
        .collect();
    assert_eq!(cargo.len(), 3);
    assert!(cargo.iter().all(|(_, template)| {
        template
            .unavailable
            .as_ref()
            .is_some_and(|reason| reason.contains("Cargo.toml"))
    }));
    let shells: Vec<_> = catalog
        .iter()
        .filter(|(provider, _)| provider == "terminal")
        .map(|(_, template)| template.id.as_str())
        .collect();
    assert!(
        shells.contains(&"PowerShell"),
        "Windows PowerShell is installed in this actual Windows environment"
    );
    assert!(
        shells
            .iter()
            .all(|id| matches!(*id, "PowerShell" | "PowerShell 7" | "cmd"))
    );
    assert_eq!(manager.live["rust"].process_count(), 0);
    // Activation preserves the existing interactive terminal; catalog resolution adds no interpreter.
    assert_eq!(manager.live["terminal"].process_count(), existing_shells);
    assert!(driver.launches.is_empty());
    manager.shutdown();
}

#[gpui::test]
#[ignore = "build real rust/terminal/rust-debugger packages and verify the pinned CodeLLDB archive"]
fn plugin_configuration_rollout_real_cargo_debug_pause_inspect_step_and_stop(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let source = "#[inline(never)]\nfn calculate(value: i32) -> i32 {\n    let doubled = value * 2;\n    println!(\"{doubled}\");\n    doubled\n}\nfn main() {\n    let answer = calculate(21);\n    println!(\"{answer}\");\n}\n";
    project(root.path(), source);
    let (mut manager, app, cx) = fixture(cx, root.path());
    rust_package(&mut manager, "renamed-cargo");
    let debugger = debugger_packages::debugger("configuration-debugger");
    manager
        .install(&debugger, debugger.manifest.permissions.clone())
        .unwrap();
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let parent = open_form(&app, cx);
    let id = add(
        &mut driver,
        &mut manager,
        &app,
        cx,
        "run-template-renamed-cargo-debug",
    );
    save(&mut driver, &mut manager, &app, cx, parent);
    cx.update(|_, cx| {
        app.update(cx, |app, _| {
            let mut config = app.run_controls.configuration(&id).unwrap().clone();
            config
                .breakpoints
                .insert(root.path().join("src/main.rs").to_str().unwrap(), 3)
                .unwrap();
            app.run_controls
                .upsert(config, &app.workspace_key())
                .unwrap();
        })
    });
    click(cx, "run-debug");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| {
            matches!(
                app.read(cx).run_controls.debug_state(),
                editor_core::DebugSessionState::Paused { line: 3, .. }
            )
        })
    });
    assert!(
        root.path()
            .join("target/debug/configuration-rollout.exe")
            .exists()
    );
    assert!(
        driver.launches.is_empty(),
        "the debugger owns the final target exactly once"
    );
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| {
            let controls = &app.read(cx).run_controls;
            !controls.debug_frames().is_empty()
                && controls.selected_debug_frame().is_some_and(|frame| {
                    controls
                        .debug_variables(frame)
                        .iter()
                        .any(|variable| variable.name == "value" && variable.value == "21")
                })
        })
    });
    let epoch = cx.update(|_, cx| app.read(cx).run_controls.debug_pause_epoch().unwrap());
    // The actual native keyboard command must step the selected real debugger target.
    click(cx, "debug-panel-state");
    cx.simulate_keystrokes("f10");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| {
            app.read(cx)
                .run_controls
                .debug_pause_epoch()
                .is_some_and(|next| next > epoch)
        })
    });
    click(cx, "run-stop");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| !app.read(cx).run_controls.debug_target_active(&id))
    });
    manager.shutdown();
}

/// Actual guest panels keep visible actions and literal multiline input across both translations,
/// both themes and the supported font-size endpoints; this does not simulate an OS IME candidate.
#[gpui::test]
#[ignore = "build actual rust/terminal packages and configuration examples via public SDK"]
fn plugin_configuration_rollout_locales_themes_and_font_sizes(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    project(root.path(), "// Layout fixture only.\nfn main() {}\n");
    let (mut manager, app, cx) = fixture(cx, root.path());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let previous_locale = rust_i18n::locale().to_string();
    let parent = open_form(&app, cx);
    let id = add(
        &mut driver,
        &mut manager,
        &app,
        cx,
        "run-template-terminal-PowerShell",
    );
    for (locale, dark, font_size) in [
        ("zh-CN", false, 14.),
        ("en", true, 18.),
        ("zh-CN", true, 24.),
        ("en", false, 10.),
    ] {
        rust_i18n::set_locale(locale);
        cx.update(|window, cx| {
            typography::set_font_size(cx, font_size);
            apply_theme(builtin_theme(dark), cx);
            theme::sync_font_sizes(cx);
            window.refresh();
        });
        // A native event requests the provider's newly localized panel without resetting its values.
        reveal(cx, "plugin-ui-name");
        edit(cx, "plugin-ui-name", &format!("{locale} / {font_size}"));
        settle(&mut driver, &mut manager, &app, cx);
        let document = cx.update(|_, cx| {
            app.read(cx)
                .run_form
                .as_ref()
                .unwrap()
                .read(cx)
                .plugin
                .as_ref()
                .unwrap()
                .documents[&id]
                .clone()
        });
        assert!(
            serde_json::to_string(&document)
                .unwrap()
                .contains(if locale == "en" {
                    "Arguments"
                } else {
                    "参数"
                })
        );
        reveal(cx, "plugin-ui-script");
        edit(
            cx,
            "plugin-ui-script",
            "第一行 中文 & ; \"quotes\"\nsecond line",
        );
        settle(&mut driver, &mut manager, &app, cx);
        let values = cx.update(|_, cx| {
            app.read(cx)
                .run_form
                .as_ref()
                .unwrap()
                .read(cx)
                .plugin
                .as_ref()
                .unwrap()
                .draft
                .plugin_configurations[&id]
                .values
                .clone()
        });
        let values: serde_json::Value = serde_json::from_str(&values).unwrap();
        assert_eq!(
            values["fields"]["script"],
            "第一行 中文 & ; \"quotes\"\nsecond line"
        );
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let body = cx.debug_bounds("plugin-ui-form-scroll").unwrap();
        for selector in [
            "run-config-add",
            "run-config-apply",
            "run-config-cancel",
            "run-config-save",
        ] {
            let control = cx.debug_bounds(selector).unwrap();
            assert!(control.size.width > px(0.) && control.size.height > px(0.));
            assert!(control.bottom() <= cx.update(|window, _| window.viewport_size().height));
            if selector != "run-config-add" {
                assert!(body.bottom() <= control.top());
            }
        }
    }
    click(cx, "run-config-cancel");
    *cx = gpui_kit::VisualTestContext::from_window(parent.into(), &cx.cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_form.is_none())
    });
    rust_i18n::set_locale(&previous_locale);
    manager.shutdown();
}
