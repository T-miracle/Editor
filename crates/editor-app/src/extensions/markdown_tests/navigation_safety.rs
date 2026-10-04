//! Real guest navigation retains permission, source-version and canonical workspace authority.
use super::*;
use harness::NativeMarkdown;
use protocol::api::{
    DocumentVersion, EditorOperation as Op, EditorValue as Value, ErrorCode, NavigationTarget,
    RequestUpdate,
};

/// Click the actual first link glyphs; the paragraph wrapper is wider than its rendered text.
fn click_link(ui: &mut gpui_kit::VisualTestContext) {
    let bounds = ui.debug_bounds("plugin-ui-b-0-paragraph").unwrap();
    ui.simulate_click(
        point(bounds.left() + px(12.), bounds.center().y),
        Default::default(),
    );
    ui.run_until_parked();
}

/// Require one actual guest-issued handle, preserving its original instance and request identity.
fn take_request(fixture: &mut NativeMarkdown, plugin: &str) -> plugin_runtime::EditorRequest {
    let mut requests = fixture
        .manager
        .live
        .get_mut(plugin)
        .unwrap()
        .take_editor_requests();
    assert_eq!(requests.len(), 1, "one operation admits one request");
    requests.pop().unwrap()
}

/// Stop after the real WASM callback has requested navigation, before the native worker executes it.
fn pending_navigation(
    fixture: &mut NativeMarkdown,
    ui: &mut gpui_kit::VisualTestContext,
) -> plugin_runtime::EditorRequest {
    click_link(ui);
    super::super::composable_tests::pump(&mut fixture.manager, &fixture.app, ui);
    let request = take_request(fixture, "markdown");
    assert!(matches!(
        request.operation(),
        Op::NavigateDocument {
            target: NavigationTarget::ExternalUrl { .. },
            ..
        }
    ));
    request
}

/// Keep the original request handle and execute through the existing deferred host publication queue.
fn execute(
    fixture: &mut NativeMarkdown,
    plugin: &str,
    request: &plugin_runtime::EditorRequest,
    ui: &mut gpui_kit::VisualTestContext,
) {
    ui.update(|_, cx| {
        fixture
            .app
            .read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .editor_requests
            .push((plugin.into(), request.clone()))
    });
    fixture.settle(ui);
}

/// Observe the same open-document authority that real editor capability consumers receive.
fn document(fixture: &NativeMarkdown, ui: &mut gpui_kit::VisualTestContext) -> DocumentVersion {
    ui.update(|_, cx| {
        let app = fixture.app.read(cx);
        app.plugin_document_version(app.active_tab_index().unwrap())
            .unwrap()
    })
}

/// Drive the public locale notification explicitly; the foreground application's default can be English.
fn set_locale(fixture: &mut NativeMarkdown, ui: &mut gpui_kit::VisualTestContext, locale: &str) {
    fixture
        .manager
        .event(
            "markdown",
            None,
            PluginEvent::Theme(protocol::Environment {
                workspace: fixture.directory.path().display().to_string(),
                locale: locale.into(),
                ..Default::default()
            }),
        )
        .unwrap();
    super::super::composable_tests::publish(
        &mut fixture.manager,
        &mut fixture.renderer,
        &fixture.app,
        ui,
    );
}

/// Feedback must be drawn as a native control as well as appear in the guest's published tree.
fn feedback(fixture: &NativeMarkdown, ui: &mut gpui_kit::VisualTestContext) -> String {
    assert!(ui.debug_bounds("plugin-ui-format-error").is_some());
    // The public lookup respects disabled ancestors and modal ownership; visible feedback is enabled.
    let node = fixture.manager.live["markdown"].views["preview"]
        .active_node("format-error")
        .expect("enabled native feedback in the current toolbar");
    let protocol::ui::Kind::Text { text } = &node.kind else {
        panic!("native feedback text expected")
    };
    text.clone()
}

/// Permission and domain refusals remain visible in both locales without consuming the source or browser.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_link_refusals_preserve_source_and_localize(cx: &mut TestAppContext) {
    let web = "[网页](https://example.com/refused)\n";
    let unsupported = "[邮件](mailto:reader@example.com)\n";
    let missing = "[标题](#missing-heading)\n\n# 已有标题\n";
    // Install a legally narrower real ZIP; revoking a required grant after install would block enable.
    let package = harness::package_without_permission("navigation.external");
    let (mut fixture, ui) = NativeMarkdown::mount_package(
        cx,
        &[
            ("web.md", web),
            ("scheme.md", unsupported),
            ("missing.md", missing),
        ],
        &package,
    );
    for (name, original, chinese, english) in [
        ("web.md", web, "没有导航权限", "permission"),
        (
            "scheme.md",
            unsupported,
            "仅支持标题锚点",
            "only heading anchors",
        ),
        ("missing.md", missing, "锚点", "anchor"),
    ] {
        fixture.open(name, ui);
        set_locale(&mut fixture, ui, "zh-CN");
        let original_version = document(&fixture, ui);
        click_link(ui);
        fixture.settle(ui);
        let message = feedback(&fixture, ui);
        assert!(
            message.contains(chinese),
            "{name}: expected {chinese}, actual {message}"
        );
        set_locale(&mut fixture, ui, "en");
        assert!(feedback(&fixture, ui).to_lowercase().contains(english));
        assert_eq!(document(&fixture, ui), original_version);
        assert_eq!(
            ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
            original
        );
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join(name)).unwrap(),
            original
        );
        assert!(ui.opened_url().is_none());
    }
}

/// Editing, switching, closing and retirement each seal a real admitted browser request before execution.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_pending_links_cannot_follow_edits_tabs_close_or_disable(
    cx: &mut TestAppContext,
) {
    let original = "[网页](https://example.com/never-opened)\n";
    let (mut fixture, ui) = NativeMarkdown::mount(
        cx,
        &[
            ("edit.md", original),
            ("switch.md", original),
            ("close.md", original),
            ("disable.md", original),
            ("other.md", "其他文档\n"),
        ],
    );
    fixture.open("other.md", ui);
    for name in ["edit.md", "switch.md", "close.md", "disable.md"] {
        fixture.open(name, ui);
        let request = pending_navigation(&mut fixture, ui);
        match name {
            "edit.md" => {
                fixture.focus_editor(ui);
                ui.simulate_keystrokes("ctrl-home");
                ui.simulate_input("变化\n");
                ui.run_until_parked();
            }
            "switch.md" => fixture.open("other.md", ui),
            "close.md" => {
                let path = fixture.directory.path().join(name).canonicalize().unwrap();
                ui.update(|window, cx| {
                    fixture
                        .app
                        .update(cx, |app, cx| app.close_tab(path, window, cx))
                });
                ui.run_until_parked();
            }
            "disable.md" => fixture.manager.disable("markdown").unwrap(),
            _ => unreachable!(),
        }
        execute(&mut fixture, "markdown", &request, ui);
        assert!(matches!(
            request.status(),
            RequestUpdate::Completed { result: Err(_) } | RequestUpdate::Cancelled { .. }
        ));
        assert!(
            ui.opened_url().is_none(),
            "obsolete navigation launched a browser: {name}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join(name)).unwrap(),
            original
        );
        if name == "edit.md" {
            assert_eq!(
                ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
                format!("变化\n{original}")
            );
        }
        if name == "switch.md" {
            assert_eq!(
                ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
                "其他文档\n"
            );
        }
        if name == "disable.md" {
            assert!(ui.debug_bounds("editor-preview-pane").is_none());
        }
    }
}

/// Legal parent paths stay within the workspace, encoded percents decode once, and real sibling files stay closed.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_relative_links_decode_once_and_reject_workspace_escape(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("中文 one.md", "# 中文目标\n")]);
    let docs = fixture.directory.path().join("docs");
    std::fs::create_dir_all(docs.join("%2e%2e")).unwrap();
    std::fs::write(
        docs.join("unicode.md"),
        "[打开](../%E4%B8%AD%E6%96%87%20one.md)\n",
    )
    .unwrap();
    std::fs::write(docs.join("percent.md"), "[打开](%252e%252e/target.md)\n").unwrap();
    std::fs::write(docs.join("%2e%2e/target.md"), "# 单次解码目标\n").unwrap();
    // A second decoding pass would incorrectly land on this separate existing document.
    std::fs::write(
        fixture.directory.path().join("target.md"),
        "wrong traversal target",
    )
    .unwrap();
    for (source, target, expected) in [
        ("docs/unicode.md", "中文 one.md", "# 中文目标\n"),
        (
            "docs/percent.md",
            "docs/%2e%2e/target.md",
            "# 单次解码目标\n",
        ),
    ] {
        fixture.open(source, ui);
        click_link(ui);
        fixture.settle(ui);
        assert_eq!(
            ui.update(|_, cx| fixture.app.read(cx).active_path.clone()),
            Some(
                fixture
                    .directory
                    .path()
                    .join(target)
                    .canonicalize()
                    .unwrap()
            )
        );
        assert_eq!(
            ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
            expected
        );
        assert!(ui.opened_url().is_none());
    }
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(outside.path().parent(), fixture.directory.path().parent());
    std::fs::write(outside.path().join("escape.md"), "outside document").unwrap();
    let source = format!(
        "[打开](%2e%2e/%2e%2e/{}/escape.md)\n",
        outside.path().file_name().unwrap().to_str().unwrap()
    );
    std::fs::write(docs.join("escape.md"), &source).unwrap();
    fixture.open("docs/escape.md", ui);
    set_locale(&mut fixture, ui, "zh-CN");
    let original_version = document(&fixture, ui);
    click_link(ui);
    fixture.settle(ui);
    assert_eq!(document(&fixture, ui), original_version);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        source
    );
    let message = feedback(&fixture, ui);
    assert!(
        message.contains("工作区"),
        "expected workspace boundary reason, actual {message}"
    );
    assert!(ui.opened_url().is_none());
}

/// A lexical in-workspace link must reject a real Windows junction that resolves to an external document.
#[cfg(windows)]
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_relative_link_rejects_external_junction(cx: &mut TestAppContext) {
    let source = "[打开](redirect/secret.md)\n";
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", source)]);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.md"), "outside document").unwrap();
    let junction = fixture.directory.path().join("redirect");
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:ME_EDITOR_NAV_JUNCTION -Value $env:ME_EDITOR_NAV_TARGET | Out-Null"])
        .env("ME_EDITOR_NAV_JUNCTION", &junction)
        .env("ME_EDITOR_NAV_TARGET", outside.path())
        .output().unwrap();
    assert!(
        output.status.success(),
        "junction creation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.open("notes.md", ui);
    set_locale(&mut fixture, ui, "zh-CN");
    let original_version = document(&fixture, ui);
    click_link(ui);
    fixture.settle(ui);
    // Remove only this known temporary junction entry; never traverse or delete its external target.
    std::fs::remove_dir(&junction).unwrap();
    assert_eq!(document(&fixture, ui), original_version);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        source
    );
    let message = feedback(&fixture, ui);
    assert!(
        message.contains("工作区"),
        "expected junction boundary reason, actual {message}"
    );
    assert!(ui.opened_url().is_none());
    assert_eq!(
        std::fs::read_to_string(outside.path().join("secret.md")).unwrap(),
        "outside document"
    );
}

/// Repackage only declarations: the independent public SDK guest handles ordinary Operation JSON unchanged.
fn navigation_peer(browser: bool) -> Package {
    let mut files = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap()
    .files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = serde_json::json!("navigation-peer");
    manifest["name"] = serde_json::json!("Independent document navigation");
    manifest["api"]["required"]["editor.navigation"] = serde_json::json!("^1");
    if browser {
        manifest["permissions"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("navigation.external"));
    }
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    language_tests::packages::repack(files).unwrap()
}

/// The probe creates a real request in WASM; neither handles nor native completion results are fabricated.
fn probe(fixture: &mut NativeMarkdown, version: DocumentVersion, target: NavigationTarget) {
    fixture
        .manager
        .invoke_command(
            "navigation-peer",
            "scope-probe",
            serde_json::to_value(protocol::api::Operation::Editor {
                operation: Op::NavigateDocument {
                    document: version,
                    target,
                },
                timeout_ms: 30_000,
            })
            .unwrap(),
        )
        .unwrap();
}

/// Missing browser authority refuses the real SDK operation before allocating a host request.
fn refuse_ungranted_browser(
    fixture: &mut NativeMarkdown,
    ui: &mut gpui_kit::VisualTestContext,
) -> DocumentVersion {
    let version = document(fixture, ui);
    probe(
        fixture,
        version.clone(),
        NavigationTarget::ExternalUrl {
            url: "https://example.com/denied".into(),
        },
    );
    let protocol::ui::Kind::Text { text } =
        &fixture.manager.live["navigation-peer"].views["welcome"]
            .root
            .kind
    else {
        panic!("public probe result expected")
    };
    let rejected: Result<protocol::api::Value, protocol::api::Failure> =
        serde_json::from_str(text).unwrap();
    assert_eq!(rejected.unwrap_err().code, ErrorCode::PermissionDenied);
    assert!(
        fixture
            .manager
            .live
            .get_mut("navigation-peer")
            .unwrap()
            .take_editor_requests()
            .is_empty()
    );
    assert!(ui.opened_url().is_none());
    assert_eq!(document(fixture, ui), version);
    version
}

/// A non-Markdown consumer opens .txt, refuses absent permission and stale sources, then reaches the real browser boundary.
#[gpui::test]
#[ignore = "build current markdown and capability-example through the public SDK first"]
fn delivered_independent_navigation_peer_guards_txt_browser_grants_and_revision(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) = NativeMarkdown::mount(
        cx,
        &[
            ("source.txt", "plain source\n"),
            ("next.txt", "plain target\n"),
        ],
    );
    let peer = navigation_peer(false);
    fixture
        .manager
        .install(&peer, peer.manifest.permissions.clone())
        .unwrap();
    fixture.open("source.txt", ui);
    assert!(ui.debug_bounds("editor-preview-pane").is_none());
    let version = refuse_ungranted_browser(&mut fixture, ui);
    probe(
        &mut fixture,
        version,
        NavigationTarget::RelativeDocument {
            path: "next.txt".into(),
        },
    );
    let request = take_request(&mut fixture, "navigation-peer");
    execute(&mut fixture, "navigation-peer", &request, ui);
    assert!(
        matches!(request.status(), RequestUpdate::Completed { result: Ok(Value::Opened { ref document }) } if document.path == "next.txt")
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "plain target\n"
    );
    assert!(ui.opened_url().is_none());

    fixture.manager.uninstall("navigation-peer", false).unwrap();
    let peer = navigation_peer(true);
    fixture
        .manager
        .install(&peer, peer.manifest.permissions.clone())
        .unwrap();
    fixture.settle(ui);
    let version = document(&fixture, ui);
    probe(
        &mut fixture,
        version,
        NavigationTarget::ExternalUrl {
            url: "https://example.com/stale".into(),
        },
    );
    let stale = take_request(&mut fixture, "navigation-peer");
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-home");
    ui.simulate_input("变更");
    ui.run_until_parked();
    execute(&mut fixture, "navigation-peer", &stale, ui);
    assert!(
        matches!(stale.status(), RequestUpdate::Completed { result: Err(ref failure) } if failure.code == ErrorCode::StaleRevision)
    );
    assert!(ui.opened_url().is_none());
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "变更plain target\n"
    );
    let version = document(&fixture, ui);
    let url = "https://example.com/independent";
    probe(
        &mut fixture,
        version,
        NavigationTarget::ExternalUrl { url: url.into() },
    );
    let browser = take_request(&mut fixture, "navigation-peer");
    execute(&mut fixture, "navigation-peer", &browser, ui);
    assert!(matches!(
        browser.status(),
        RequestUpdate::Completed {
            result: Ok(Value::Unit)
        }
    ));
    assert_eq!(ui.opened_url().as_deref(), Some(url));
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "变更plain target\n"
    );
}
