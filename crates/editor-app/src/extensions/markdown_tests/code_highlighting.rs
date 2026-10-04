//! Code preview highlighting enters through real Markdown and independently named grammar packages.
use super::*;
use gpui_kit::{ClipboardItem, VisualTestContext};
use harness::NativeMarkdown;
use std::time::{Duration, Instant};

const CODE: &str = "plugin-ui-b-0-code";
const NUMBER: &str = "plugin-code-capture-b-0-code-0-number";
const STRING: &str = "plugin-code-capture-b-0-code-0-string";
const NUMBER_AFTER_COMMENT: &str = "plugin-code-capture-b-0-code-1-number";
const STRING_AFTER_COMMENT: &str = "plugin-code-capture-b-0-code-1-string";

/// Reinspect a real grammar ZIP with one integer capture so drawn color provenance is unambiguous.
/// Matching package/declaration versions make replacements ordinary public installations.
fn capture_package(id: &str, capture: &str, version: &str) -> Package {
    let mut files = language_tests::packages::language_package(id).files;
    files.insert(
        "highlights.scm".into(),
        format!("(integer) @{capture}\n").into_bytes(),
    );
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["version"] = serde_json::json!(version);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let declaration = String::from_utf8(files["plugin.toml"].clone()).unwrap();
    files.insert(
        "plugin.toml".into(),
        declaration
            .replacen(
                "version = \"1.0.0\"",
                &format!("version = \"{version}\""),
                1,
            )
            .into_bytes(),
    );
    language_tests::packages::repack(files).unwrap()
}

/// Publish installed resources through the existing manager-to-native contribution boundary.
fn install_provider(fixture: &mut NativeMarkdown, ui: &mut VisualTestContext, package: &Package) {
    fixture
        .manager
        .install(package, package.manifest.permissions.clone())
        .unwrap();
    language_tests::publish_languages(&fixture.app, &fixture.manager, ui);
}

/// Use the same persisted provider choice as native settings, isolated to this fixture workspace.
fn choose_provider(fixture: &mut NativeMarkdown, ui: &mut VisualTestContext, provider: &str) {
    crate::language::providers::choose(
        protocol::settings::Scope::Project,
        "highlight:novel",
        Some(provider),
    )
    .unwrap();
    ui.update(|_, cx| {
        fixture
            .app
            .update(cx, |app, cx| app.sync_dynamic_languages(cx))
    });
    ui.run_until_parked();
    fixture.settle(ui);
}

/// Await a real painted StyledText line while pumping actual guest and background worker results.
/// A bounded wait accommodates asynchronous WASM parsing without manufacturing a successful reply.
fn wait_for_highlight(
    fixture: &mut NativeMarkdown,
    ui: &mut VisualTestContext,
    selector: &'static str,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        ui.run_until_parked();
        fixture.settle(ui);
        if ui.debug_bounds(selector).is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the enabled provider did not paint {selector}; current code {:?}",
            fixture.manager.live["markdown"].views["preview"]
                .active_node("b-0-code")
                .map(|node| &node.kind)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Monospace fallback or retired blocks must remain stable through further worker publications.
/// This short drain gives real late completions a chance to draw; no parser result is injected.
fn assert_unstyled(fixture: &mut NativeMarkdown, ui: &mut VisualTestContext, code_visible: bool) {
    let deadline = Instant::now() + Duration::from_millis(150);
    loop {
        ui.run_until_parked();
        fixture.settle(ui);
        assert_eq!(ui.debug_bounds(CODE).is_some(), code_visible);
        for selector in [
            "plugin-code-highlight-b-0-code-0",
            "plugin-code-highlight-b-0-code-1",
        ] {
            assert!(
                ui.debug_bounds(selector).is_none(),
                "a retired or unavailable provider must not draw {selector}"
            );
        }
        if Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Read the native document authority rather than deriving a version from a preview node.
fn source_version(
    fixture: &NativeMarkdown,
    ui: &mut VisualTestContext,
) -> protocol::api::DocumentVersion {
    ui.update(|_, cx| {
        let app = fixture.app.read(cx);
        app.plugin_document_version(app.active_tab_index().unwrap())
            .unwrap()
    })
}

/// Compare the editor and guest source identity, while preserving the independent on-disk baseline.
fn assert_source(
    fixture: &NativeMarkdown,
    ui: &mut VisualTestContext,
    name: &str,
    current: &str,
    saved: &str,
) {
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        current
    );
    assert_eq!(
        fixture.manager.live["markdown"].views["preview"].source,
        Some(source_version(fixture, ui))
    );
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join(name)).unwrap(),
        saved
    );
}

/// The fenced block must still occupy the visible preview rather than merely exist in guest data.
fn assert_visible_code(ui: &mut VisualTestContext) {
    let bounds = ui.debug_bounds(CODE).expect("visible native code block");
    let pane = ui.debug_bounds("editor-preview-pane").unwrap();
    assert!(bounds.top() >= pane.top() && bounds.bottom() <= pane.bottom());
}

/// An enabled unknown language provider must color the native code line without editing its source.
#[gpui::test]
#[ignore = "build current markdown and TOML WASM resources through scripts/build-plugins.ps1 first"]
fn delivered_markdown_code_uses_an_independent_enabled_language_provider(cx: &mut TestAppContext) {
    let original = "```novel\nanswer = 42\n```\n";
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", original)]);
    let provider = language_tests::packages::language_package("novel-code-primary");
    fixture
        .manager
        .install(&provider, provider.manifest.permissions.clone())
        .unwrap();
    language_tests::publish_languages(&fixture.app, &fixture.manager, ui);
    fixture.open("notes.md", ui);

    // preview.rs derives fenced identities from the opening source byte and the literal "code" kind.
    // Inspect the actual guest reply before requiring its native line to carry nonempty highlight spans.
    let scene = fixture.manager.live["markdown"].views["preview"].as_ref();
    let block = scene
        .active_node("b-0-code")
        .expect("delivered fenced code block");
    assert!(matches!(
        &block.kind,
        protocol::ui::Kind::CodeBlock { text, language }
            if text == "answer = 42\n" && language.as_deref() == Some("novel")
    ));
    assert_eq!(block.source_range.unwrap().start, 0);
    let bounds = ui
        .debug_bounds("plugin-ui-b-0-code")
        .expect("visible native code block");
    let pane = ui.debug_bounds("editor-preview-pane").unwrap();
    assert!(bounds.top() >= pane.top() && bounds.bottom() <= pane.bottom());
    assert_eq!(
        scene.source,
        ui.update(|_, cx| {
            let app = fixture.app.read(cx);
            Some(
                app.plugin_document_version(app.active_tab_index().unwrap())
                    .unwrap(),
            )
        })
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("notes.md")).unwrap(),
        original
    );
    // This selector belongs only to StyledText lines containing real nonempty provider captures.
    // A recognized language label or plain monospace wrapper cannot satisfy the visible behavior.
    wait_for_highlight(&mut fixture, ui, "plugin-code-highlight-b-0-code-0");
}

/// Explicit choices survive competing installation, while replacement/restart drops old captures.
#[gpui::test]
#[ignore = "build current markdown and TOML WASM resources through scripts/build-plugins.ps1 first"]
fn delivered_markdown_code_tracks_provider_choice_replacement_and_plain_fallback(
    cx: &mut TestAppContext,
) {
    let original = "```novel\nanswer = 42\n```\n";
    let unknown = "```unknown-code\nanswer = 42\n```\n";
    let unmarked = "```\nanswer = 42\n```\n";
    let (mut fixture, ui) = NativeMarkdown::mount(
        cx,
        &[
            ("notes.md", original),
            ("unknown.md", unknown),
            ("unmarked.md", unmarked),
        ],
    );
    let primary = capture_package("novel-number", "number", "1.0.0");
    let alternate = capture_package("novel-string", "string", "1.0.0");
    install_provider(&mut fixture, ui, &primary);
    fixture.open("notes.md", ui);
    wait_for_highlight(&mut fixture, ui, NUMBER);
    let authority = source_version(&fixture, ui);

    // Installing a competing package cannot silently reorder a still-valid user's choice.
    install_provider(&mut fixture, ui, &alternate);
    wait_for_highlight(&mut fixture, ui, NUMBER);
    assert!(ui.debug_bounds(STRING).is_none());
    choose_provider(&mut fixture, ui, "novel-string/syntax");
    wait_for_highlight(&mut fixture, ui, STRING);
    assert!(ui.debug_bounds(NUMBER).is_none());

    let replacement = capture_package("novel-string", "number", "1.1.0");
    install_provider(&mut fixture, ui, &replacement);
    wait_for_highlight(&mut fixture, ui, NUMBER);
    assert!(ui.debug_bounds(STRING).is_none());
    fixture.manager.disable("novel-number").unwrap();
    fixture.manager.disable("novel-string").unwrap();
    language_tests::publish_languages(&fixture.app, &fixture.manager, ui);
    assert_unstyled(&mut fixture, ui, true);
    fixture.manager.enable("novel-string").unwrap();
    language_tests::publish_languages(&fixture.app, &fixture.manager, ui);
    wait_for_highlight(&mut fixture, ui, NUMBER);

    // The same logical owner returns after A/disabled/A while its resource content has changed.
    // Old number captures must not survive an immediate restart of the new string-query version.
    let restarted = capture_package("novel-string", "string", "1.2.0");
    install_provider(&mut fixture, ui, &restarted);
    fixture.manager.disable("novel-string").unwrap();
    language_tests::publish_languages(&fixture.app, &fixture.manager, ui);
    fixture.manager.enable("novel-string").unwrap();
    language_tests::publish_languages(&fixture.app, &fixture.manager, ui);
    wait_for_highlight(&mut fixture, ui, STRING);
    assert!(ui.debug_bounds(NUMBER).is_none());
    for dark in [true, false] {
        ui.update(|_, cx| apply_theme(builtin_theme(dark), cx));
        fixture.settle(ui);
        wait_for_highlight(&mut fixture, ui, STRING);
        assert_visible_code(ui);
    }
    assert_eq!(source_version(&fixture, ui), authority);
    assert_source(&fixture, ui, "notes.md", original, original);

    // Identical code text cannot invent a provider when its fence is unknown or has no marker.
    for (name, text) in [("unknown.md", unknown), ("unmarked.md", unmarked)] {
        fixture.open(name, ui);
        assert_unstyled(&mut fixture, ui, true);
        assert_visible_code(ui);
        let block = fixture.manager.live["markdown"].views["preview"]
            .active_node("b-0-code")
            .unwrap();
        assert!(matches!(
            &block.kind,
            protocol::ui::Kind::CodeBlock { text, .. } if text == "answer = 42\n"
        ));
        assert_source(&fixture, ui, name, text, text);
    }
}

/// Chinese/CRLF source stays native and undoable; old code cannot follow edits, tabs or reopened IDs.
#[gpui::test]
#[ignore = "build current markdown and TOML WASM resources through scripts/build-plugins.ps1 first"]
fn delivered_markdown_code_keeps_source_undo_and_rejects_retired_document_scenes(
    cx: &mut TestAppContext,
) {
    let original = "```novel\r\n# 中文示例\r\nanswer = 42\r\n```\r\n";
    let changed = "# 修改后的中文\r\n";
    let other = "# 另一份文档\r\n";
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", original), ("other.md", other)]);
    let provider = capture_package("novel-source-authority", "number", "1.0.0");
    install_provider(&mut fixture, ui, &provider);
    fixture.open("notes.md", ui);
    let authority = source_version(&fixture, ui);
    assert_source(&fixture, ui, "notes.md", original, original);
    assert_visible_code(ui);
    let block = fixture.manager.live["markdown"].views["preview"]
        .active_node("b-0-code")
        .unwrap();
    assert!(
        matches!(
            &block.kind,
            protocol::ui::Kind::CodeBlock { text, .. }
                if text.lines().collect::<Vec<_>>() == ["# 中文示例", "answer = 42"]
        ),
        "the literal code must preserve its Chinese comment and second line: {:?}",
        block.kind
    );
    wait_for_highlight(&mut fixture, ui, NUMBER_AFTER_COMMENT);

    // A native paste is one Undo transaction, whereas simulated typing groups its newline separately.
    // The real clipboard/input path removes the old scene, including any late colored line.
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-a");
    ui.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(changed.to_owned())));
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    assert_unstyled(&mut fixture, ui, false);
    assert_source(&fixture, ui, "notes.md", changed, original);
    fixture.open("other.md", ui);
    assert_unstyled(&mut fixture, ui, false);
    assert_source(&fixture, ui, "other.md", other, other);
    fixture.open("notes.md", ui);
    assert_source(&fixture, ui, "notes.md", changed, original);

    // Highlighting is derived display data: a single native Undo must undo the user's replacement.
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_source(&fixture, ui, "notes.md", original, original);
    wait_for_highlight(&mut fixture, ui, NUMBER_AFTER_COMMENT);
    let restored = source_version(&fixture, ui);
    assert_eq!(restored.id, authority.id);
    assert!(restored.revision > authority.revision);

    // The editor refuses to close dirty sessions, including content restored by Undo.
    // Save the identical restored bytes through the ordinary native command before testing retirement.
    // This isolated window initializes Base keys, while the main application installs Ctrl+S.
    // Dispatch the same native SaveDocument action through its real window listener.
    ui.update(|window, cx| window.dispatch_action(Box::new(SaveDocument), cx));
    ui.run_until_parked();
    fixture.settle(ui);
    assert_source(&fixture, ui, "notes.md", original, original);
    assert!(ui.update(|_, cx| {
        let app = fixture.app.read(cx);
        !app.tabs[app.active_tab_index().unwrap()].session.is_dirty()
    }));

    // Replacing the real query schedules fresh work; closing the source cannot reattach its scene.
    let replacement = capture_package("novel-source-authority", "string", "1.1.0");
    install_provider(&mut fixture, ui, &replacement);
    fixture.open("other.md", ui);
    let path = fixture
        .directory
        .path()
        .join("notes.md")
        .canonicalize()
        .unwrap();
    ui.update(|window, cx| {
        fixture
            .app
            .update(cx, |app, cx| app.close_tab(path, window, cx))
    });
    ui.run_until_parked();
    assert_unstyled(&mut fixture, ui, false);
    assert_source(&fixture, ui, "other.md", other, other);
    fixture.open("notes.md", ui);
    let reopened = source_version(&fixture, ui);
    assert_ne!(
        reopened.id, authority.id,
        "a reopened file owns a new identity"
    );
    wait_for_highlight(&mut fixture, ui, STRING_AFTER_COMMENT);
    assert!(ui.debug_bounds(NUMBER_AFTER_COMMENT).is_none());
    assert_source(&fixture, ui, "notes.md", original, original);
    assert_visible_code(ui);
}
