//! File transfers enter through mounted tree menus, with actual disk results as the oracle.

use super::*;
use gpui_kit::{ClipboardEntry, ClipboardItem, ExternalPaths};

/// Find a menu item by its accessible label, then activate the real component.
fn click_menu(visual: &mut VisualTestContext, label: &str) {
    let bounds = visual.update(|window, _| {
        gpui_base::test_support::snapshots(window)
            .into_iter()
            .find(|item| {
                item.role() == Some(gpui_kit::Role::MenuItem) && item.label() == Some(label)
            })
            .unwrap()
            .bounds()
    });
    visual.simulate_click(bounds.center(), Modifiers::default());
    redraw(visual);
}

/// Populate the platform clipboard and invoke the tree's existing paste menu.
pub(super) fn paste(visual: &mut VisualTestContext, sources: Vec<PathBuf>, index: usize) {
    paste_item(
        visual,
        ClipboardItem {
            entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(
                sources.into_iter().collect(),
            ))],
        },
        index,
    );
}

/// File URI clipboard payloads are also a real supported input, not a test-only command.
pub(super) fn paste_item(visual: &mut VisualTestContext, item: ClipboardItem, index: usize) {
    visual.update(|_, cx| cx.write_to_clipboard(item));
    // GPUI's diagnostic lookup requires a static selector; this allocation lasts only for the test.
    let selector = Box::leak(format!("explorer-row-{index}").into_boxed_str());
    let position = visual.debug_bounds(selector).unwrap().center();
    visual.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Right,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    });
    visual.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Right,
        modifiers: Modifiers::default(),
        click_count: 1,
    });
    redraw(visual);
    click_menu(visual, &t!("explorer.paste").to_string());
}

#[gpui::test]
fn explorer_transfer_cut_preserves_unsaved_session(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("destination")).unwrap();
    std::fs::write(project.path().join("source.txt"), "saved").unwrap();
    let workspace = Workspace::open(project.path()).unwrap();
    let source = workspace.root().join("source.txt");
    let target = workspace.root().join("destination/source.txt");
    let (app, visual) = mount(cx, workspace, source.clone());
    visual.simulate_keystrokes("end");
    visual.simulate_input("draft");
    redraw(visual);
    let editor_id = visual.update(|_, cx| app.read(cx).editor.entity_id());
    let uri = url::Url::from_file_path(&source).unwrap();
    paste_item(visual, ClipboardItem::new_string(format!("cut\n{uri}")), 1);
    assert!(!source.exists(), "a cut input must move the source");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "saved");
    visual.update(|window, cx| {
        let app = app.read(cx);
        assert_eq!(app.tabs[0].path(), target);
        assert_eq!(app.active_path.as_ref(), Some(&target));
        assert_eq!(app.editor.entity_id(), editor_id);
        assert_eq!(app.editor.read(cx).value().to_string(), "saveddraft");
        assert!(app.tabs[0].text.as_ref().unwrap().session.is_dirty());
        app.editor.focus_handle(cx).focus(window, cx);
    });
    visual.simulate_keystrokes("ctrl-z");
    redraw(visual);
    visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).value().to_string(), "saved"));
}

#[gpui::test]
fn explorer_transfer_paste_shortcut_follows_tree_focus(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("document.txt"), "text").unwrap();
    let source = external.path().join("import.txt");
    std::fs::write(&source, "import").unwrap();
    let (app, visual) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    visual.update(|_, cx| {
        cx.write_to_clipboard(ClipboardItem {
            entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(
                [source.clone()].into_iter().collect(),
            ))],
        })
    });
    let root = visual.debug_bounds("explorer-row-0").unwrap().center();
    visual.simulate_click(root, Modifiers::default());
    redraw(visual);
    visual.simulate_keystrokes("ctrl-v");
    redraw(visual);
    assert_eq!(
        std::fs::read_to_string(project.path().join("import.txt")).unwrap(),
        "import"
    );
    visual.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("中文文本".into()));
        app.read(cx).editor.focus_handle(cx).focus(window, cx);
    });
    visual.simulate_keystrokes("ctrl-v");
    redraw(visual);
    visual.update(|_, cx| assert!(app.read(cx).editor.read(cx).value().contains("中文文本")));
}

#[gpui::test]
fn explorer_transfer_paste_conflict_keeps_both(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("same.txt"), "original").unwrap();
    let source = external.path().join("same.txt");
    std::fs::write(&source, "imported").unwrap();
    let (app, visual) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    paste(visual, vec![source.clone()], 0);
    let keep = visual
        .debug_bounds("transfer-keep-both")
        .expect("a same-name import requires a conflict decision");
    visual.simulate_click(keep.center(), Modifiers::default());
    redraw(visual);
    assert_eq!(
        std::fs::read_to_string(project.path().join("same.txt")).unwrap(),
        "original"
    );
    assert_eq!(
        std::fs::read_to_string(project.path().join("same (2).txt")).unwrap(),
        "imported"
    );
    assert!(source.exists(), "copy preserves the external source");
    visual.update(|_, cx| assert_eq!(app.read(cx).tabs.len(), 1));
}

#[gpui::test]
fn explorer_transfer_replace_updates_open_clean_tab(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("same.txt"), "old").unwrap();
    let source = external.path().join("same.txt");
    std::fs::write(&source, "new").unwrap();
    let (app, visual) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    paste(visual, vec![source], 0);
    let position = visual.debug_bounds("transfer-replace").unwrap().center();
    visual.simulate_click(position, Modifiers::default());
    redraw(visual);
    assert_eq!(
        std::fs::read_to_string(project.path().join("same.txt")).unwrap(),
        "new"
    );
    visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).value().to_string(), "new"));
}

#[gpui::test]
fn explorer_transfer_dirty_target_can_only_keep_both_or_skip(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("same.txt"), "saved").unwrap();
    let source = external.path().join("same.txt");
    std::fs::write(&source, "replacement").unwrap();
    let (app, visual) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    visual.simulate_input("draft");
    redraw(visual);
    let before = visual.update(|_, cx| app.read(cx).editor.read(cx).value().to_string());
    paste(visual, vec![source], 0);
    let position = visual.debug_bounds("transfer-replace").unwrap().center();
    visual.simulate_click(position, Modifiers::default());
    redraw(visual);
    assert_eq!(
        std::fs::read_to_string(project.path().join("same.txt")).unwrap(),
        "saved"
    );
    assert!(visual.debug_bounds("transfer-keep-both").is_some());
    let position = visual.debug_bounds("transfer-keep-both").unwrap().center();
    visual.simulate_click(position, Modifiers::default());
    redraw(visual);
    assert_eq!(
        std::fs::read_to_string(project.path().join("same (2).txt")).unwrap(),
        "replacement"
    );
    visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).value().to_string(), before));
}

#[gpui::test]
fn explorer_transfer_merge_keeps_unique_files_and_applies_subsequent_choice(
    cx: &mut TestAppContext,
) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("folder")).unwrap();
    std::fs::create_dir(external.path().join("folder")).unwrap();
    std::fs::write(project.path().join("folder/unique.txt"), "unique").unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(project.path().join("folder").join(name), "old").unwrap();
        std::fs::write(external.path().join("folder").join(name), "new").unwrap();
    }
    let workspace = Workspace::open(project.path()).unwrap();
    let unique = workspace.root().join("folder/unique.txt");
    let (app, visual) = mount(cx, workspace, unique);
    visual.simulate_input("draft");
    redraw(visual);
    let draft = visual.update(|_, cx| app.read(cx).editor.read(cx).value().to_string());
    paste(visual, vec![external.path().join("folder")], 0);
    let position = visual.debug_bounds("transfer-replace").unwrap().center();
    visual.simulate_click(position, Modifiers::default());
    redraw(visual);
    let checkbox = visual.update(|window, _| {
        gpui_base::test_support::snapshots(window)
            .into_iter()
            .find(|item| {
                item.role() == Some(gpui_kit::Role::CheckBox)
                    && item.label() == Some(t!("transfer.apply_subsequent").as_ref())
            })
            .unwrap()
            .bounds()
    });
    visual.simulate_click(checkbox.center(), Modifiers::default());
    redraw(visual);
    let position = visual.debug_bounds("transfer-replace").unwrap().center();
    visual.simulate_click(position, Modifiers::default());
    redraw(visual);
    assert!(visual.debug_bounds("transfer-replace").is_none());
    for name in ["a.txt", "b.txt"] {
        assert_eq!(
            std::fs::read_to_string(project.path().join("folder").join(name)).unwrap(),
            "new"
        );
    }
    assert_eq!(
        std::fs::read_to_string(project.path().join("folder/unique.txt")).unwrap(),
        "unique"
    );
    visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).value().to_string(), draft));
}

#[gpui::test]
fn explorer_transfer_failure_and_cancel_retain_only_completed_results(cx: &mut TestAppContext) {
    for cancel in [false, true] {
        let project = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let first = external.path().join("a.txt");
        let second = external.path().join("b.txt");
        let third = external.path().join("c.txt");
        std::fs::write(&first, "completed").unwrap();
        std::fs::write(&third, "not started").unwrap();
        if cancel {
            std::fs::write(&second, "imported").unwrap();
            std::fs::write(project.path().join("b.txt"), "original").unwrap();
        }
        let (_, visual) = mount(cx, Workspace::open(project.path()).unwrap(), None);
        paste(visual, vec![first.clone(), second.clone(), third], 0);
        if cancel {
            visual.executor().advance_clock(Duration::from_millis(400));
            redraw(visual);
            assert!(visual.debug_bounds("transfer-progress").is_some());
            let position = visual
                .debug_bounds("transfer-conflict-cancel")
                .unwrap()
                .center();
            visual.simulate_click(position, Modifiers::default());
            redraw(visual);
            assert_eq!(
                std::fs::read_to_string(project.path().join("b.txt")).unwrap(),
                "original"
            );
        }
        assert_eq!(
            std::fs::read_to_string(project.path().join("a.txt")).unwrap(),
            "completed"
        );
        assert!(!project.path().join("c.txt").exists());
        assert!(
            first.exists(),
            "failed or cancelled copies preserve their source"
        );
        visual.executor().advance_clock(Duration::from_secs(3));
        redraw(visual);
        assert!(
            visual.debug_bounds("local-notification").is_some(),
            "operation errors wait for explicit dismissal"
        );
        assert!(!std::fs::read_dir(project.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".me-transfer-")
        }));
    }
}

#[gpui::test]
fn explorer_transfer_move_replaces_clean_tab_identity(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("destination")).unwrap();
    std::fs::write(project.path().join("same.txt"), "source").unwrap();
    std::fs::write(project.path().join("destination/same.txt"), "target").unwrap();
    let workspace = Workspace::open(project.path()).unwrap();
    let source = workspace.root().join("same.txt");
    let target = workspace.root().join("destination/same.txt");
    let (app, visual) = mount(cx, workspace, source.clone());
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_file(target.clone(), window, cx);
            app.activate_tab(0, window, cx);
        })
    });
    redraw(visual);
    visual.simulate_input("draft");
    redraw(visual);
    let before = visual.update(|_, cx| app.read(cx).editor.read(cx).value().to_string());
    let uri = url::Url::from_file_path(&source).unwrap();
    paste_item(visual, ClipboardItem::new_string(format!("cut\n{uri}")), 1);
    let position = visual.debug_bounds("transfer-replace").unwrap().center();
    visual.simulate_click(position, Modifiers::default());
    redraw(visual);
    visual.update(|_, cx| {
        let app = app.read(cx);
        assert_eq!(
            app.tabs.len(),
            1,
            "a replaced clean target must not block the moving document's identity"
        );
        assert_eq!(app.tabs[0].path(), target);
        assert_eq!(app.editor.read(cx).value().to_string(), before);
    });
}

#[cfg(windows)]
#[gpui::test]
fn explorer_transfer_skips_windows_junction_and_continues(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let linked = tempfile::tempdir().unwrap();
    std::fs::create_dir(external.path().join("folder")).unwrap();
    std::fs::write(external.path().join("folder/plain.txt"), "plain").unwrap();
    std::fs::write(linked.path().join("secret.txt"), "must not traverse").unwrap();
    // PowerShell creates only a filesystem fixture; no terminal window or shell UI is automated.
    let result = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:ME_TRANSFER_TEST_LINK -Target $env:ME_TRANSFER_TEST_TARGET -ErrorAction Stop | Out-Null"])
        .env("ME_TRANSFER_TEST_LINK", external.path().join("folder/junction"))
        .env("ME_TRANSFER_TEST_TARGET", linked.path())
        .output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let (_, visual) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    paste(visual, vec![external.path().join("folder")], 0);
    assert_eq!(
        std::fs::read_to_string(project.path().join("folder/plain.txt")).unwrap(),
        "plain"
    );
    assert!(!project.path().join("folder/junction").exists());
    assert!(linked.path().join("secret.txt").exists());
    assert!(visual.debug_bounds("local-notification").is_some());
}

#[cfg(windows)]
#[gpui::test]
fn explorer_transfer_failed_move_preserves_source_and_cleans_current_target(
    cx: &mut TestAppContext,
) {
    use std::os::windows::fs::OpenOptionsExt as _;
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let source = external.path().join("locked.txt");
    std::fs::write(&source, "locked source").unwrap();
    // A real Windows handle denies deletion while allowing the background copy to read the file.
    let _lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&source)
        .unwrap();
    let (_, visual) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    let uri = url::Url::from_file_path(&source).unwrap();
    paste_item(visual, ClipboardItem::new_string(format!("cut\n{uri}")), 0);
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "locked source");
    assert!(!project.path().join("locked.txt").exists());
    assert!(visual.debug_bounds("local-notification").is_some());
}

/// The originally canonical workspace cannot be redirected by replacing one of its parent directories.
#[cfg(windows)]
#[gpui::test]
fn explorer_transfer_rejects_replaced_workspace_ancestor(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let parent = fixture.path().join("parent");
    let parked = fixture.path().join("parked");
    let redirected = fixture.path().join("redirected");
    std::fs::create_dir_all(parent.join("project")).unwrap();
    std::fs::create_dir_all(redirected.join("project")).unwrap();
    let source = external.path().join("import.txt");
    std::fs::write(&source, "must stay outside redirected tree").unwrap();
    let (_, ui) = mount(cx, Workspace::open(parent.join("project")).unwrap(), None);
    std::fs::rename(&parent, &parked).unwrap();
    // This command only prepares an explicitly named temporary filesystem fixture.
    let result = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:ME_TRANSFER_TEST_LINK -Target $env:ME_TRANSFER_TEST_TARGET -ErrorAction Stop | Out-Null"])
        .env("ME_TRANSFER_TEST_LINK", &parent).env("ME_TRANSFER_TEST_TARGET", &redirected)
        .output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    transfer::paste(ui, vec![source], 0);
    let wrote_outside = redirected.join("project/import.txt").exists();
    std::fs::remove_dir(&parent).unwrap();
    std::fs::rename(&parked, &parent).unwrap();
    assert!(!wrote_outside);
    assert!(ui.debug_bounds("local-notification").is_some());
}
