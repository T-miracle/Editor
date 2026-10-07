//! Disk undo is exercised through tree shortcuts, with session identity and disk bytes as the oracle.
use super::*;

/// Explicitly focus a visible tree row, leaving text undo to the editor's own key context.
fn history_key(ui: &mut VisualTestContext, key: &str) {
    let position = ui.debug_bounds("explorer-row-0").unwrap().center();
    ui.simulate_click(position, Modifiers::default());
    ui.simulate_keystrokes(key);
    redraw(ui);
}

/// Choose a visible recovery button through its actual pointer hitbox.
fn click_choice(ui: &mut VisualTestContext, selector: &'static str) {
    let position = ui.debug_bounds(selector).unwrap().center();
    ui.simulate_click(position, Modifiers::default());
}

#[gpui::test]
fn explorer_transfer_undo_copy_and_redo(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let source = external.path().join("import.txt");
    std::fs::write(&source, "import").unwrap();
    let (_, ui) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    transfer::paste(ui, vec![source.clone()], 0);
    let target = project.path().join("import.txt");
    assert!(target.exists());
    history_key(ui, "ctrl-z");
    assert!(!target.exists(), "file undo must remove the imported copy");
    assert!(source.exists());
    history_key(ui, "ctrl-shift-z");
    assert_eq!(std::fs::read_to_string(target).unwrap(), "import");
}

#[gpui::test]
fn explorer_transfer_undo_move_directory_preserves_dirty_session(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join("source/nested")).unwrap();
    std::fs::create_dir(project.path().join("destination")).unwrap();
    std::fs::write(project.path().join("source/nested/file.txt"), "saved").unwrap();
    let workspace = Workspace::open(project.path()).unwrap();
    let original = workspace.root().join("source/nested/file.txt");
    let moved = workspace.root().join("destination/source/nested/file.txt");
    let source = workspace.root().join("source");
    let (app, ui) = mount(cx, workspace, original.clone());
    ui.simulate_keystrokes("end");
    ui.simulate_input("draft");
    redraw(ui);
    let editor_id = ui.update(|_, cx| app.read(cx).editor.entity_id());
    let uri = url::Url::from_file_path(&source).unwrap();
    transfer::paste_item(
        ui,
        gpui_kit::ClipboardItem::new_string(format!("cut\n{uri}")),
        1,
    );
    assert!(moved.exists());
    history_key(ui, "ctrl-z");
    assert!(original.exists());
    assert!(!moved.exists());
    ui.update(|_, cx| {
        let app = app.read(cx);
        assert_eq!(app.active_path.as_ref(), Some(&original));
        assert_eq!(app.editor.entity_id(), editor_id);
        assert_eq!(app.editor.read(cx).value().to_string(), "saveddraft");
        assert!(app.tabs[0].text.as_ref().unwrap().session.is_dirty());
    });
    history_key(ui, "ctrl-shift-z");
    assert!(!original.exists());
    assert!(moved.exists());
    ui.update(|window, cx| {
        let app = app.read(cx);
        assert_eq!(app.active_path.as_ref(), Some(&moved));
        app.editor.focus_handle(cx).focus(window, cx);
    });
    ui.simulate_keystrokes("ctrl-z");
    redraw(ui);
    assert!(
        moved.exists(),
        "text undo must leave the disk move in place"
    );
    ui.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).value().to_string(), "saved"));
}

#[gpui::test]
fn explorer_transfer_undo_merge_restores_only_changed_leaves(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("folder")).unwrap();
    std::fs::create_dir(external.path().join("folder")).unwrap();
    std::fs::write(project.path().join("folder/replace.txt"), "old").unwrap();
    std::fs::write(project.path().join("folder/unique.txt"), "unique").unwrap();
    std::fs::write(external.path().join("folder/replace.txt"), "new").unwrap();
    std::fs::write(external.path().join("folder/add.txt"), "added").unwrap();
    let (_, ui) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    transfer::paste(ui, vec![external.path().join("folder")], 0);
    click_choice(ui, "transfer-replace");
    redraw(ui);
    click_choice(ui, "transfer-replace");
    redraw(ui);
    assert_eq!(
        std::fs::read_to_string(project.path().join("folder/replace.txt")).unwrap(),
        "new"
    );
    history_key(ui, "ctrl-z");
    assert_eq!(
        std::fs::read_to_string(project.path().join("folder/replace.txt")).unwrap(),
        "old"
    );
    assert!(!project.path().join("folder/add.txt").exists());
    assert_eq!(
        std::fs::read_to_string(project.path().join("folder/unique.txt")).unwrap(),
        "unique"
    );
    history_key(ui, "ctrl-shift-z");
    assert!(project.path().join("folder/add.txt").exists());
    assert_eq!(
        std::fs::read_to_string(project.path().join("folder/replace.txt")).unwrap(),
        "new"
    );
}

#[gpui::test]
fn explorer_transfer_recovery_dirty_force_needs_second_confirmation(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let source = external.path().join("file.txt");
    std::fs::write(&source, "disk").unwrap();
    let workspace = Workspace::open(project.path()).unwrap();
    let target = workspace.root().join("file.txt");
    let (app, ui) = mount(cx, workspace, None);
    transfer::paste(ui, vec![source], 0);
    ui.update(|window, cx| app.update(cx, |app, cx| app.open_file(target.clone(), window, cx)));
    ui.simulate_keystrokes("end");
    ui.simulate_input("draft");
    redraw(ui);
    history_key(ui, "ctrl-z");
    click_choice(ui, "transfer-force");
    redraw(ui);
    assert!(target.exists());
    assert!(
        ui.debug_bounds("transfer-force").is_some(),
        "the second confirmation remains visible"
    );
    click_choice(ui, "transfer-conflict-cancel");
    redraw(ui);
    assert!(target.exists());
    ui.update(|_, cx| {
        assert_eq!(
            app.read(cx).editor.read(cx).value().to_string(),
            "diskdraft"
        )
    });
    history_key(ui, "ctrl-z");
    for _ in 0..2 {
        click_choice(ui, "transfer-force");
        redraw(ui);
    }
    assert!(!target.exists());
    ui.update(|_, cx| {
        assert!(
            !app.read(cx).tabs[0]
                .text
                .as_ref()
                .unwrap()
                .session
                .is_dirty()
        );
        assert_eq!(app.read(cx).editor.read(cx).value().to_string(), "");
    });
    // Redo rechecks a newly occupied path instead of silently overwriting it.
    std::fs::write(&target, "later").unwrap();
    history_key(ui, "ctrl-shift-z");
    assert!(ui.debug_bounds("transfer-force").is_some());
    click_choice(ui, "transfer-conflict-cancel");
    redraw(ui);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "later");
    history_key(ui, "ctrl-shift-z");
    click_choice(ui, "transfer-force");
    redraw(ui);
    assert_eq!(std::fs::read_to_string(target).unwrap(), "disk");
}

#[gpui::test]
fn explorer_transfer_recovery_occupied_move_source_skip_keeps_both_endpoints(
    cx: &mut TestAppContext,
) {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("destination")).unwrap();
    std::fs::write(project.path().join("source.txt"), "source").unwrap();
    let workspace = Workspace::open(project.path()).unwrap();
    let source = workspace.root().join("source.txt");
    let target = workspace.root().join("destination/source.txt");
    let (_, ui) = mount(cx, workspace, None);
    let uri = url::Url::from_file_path(&source).unwrap();
    transfer::paste_item(
        ui,
        gpui_kit::ClipboardItem::new_string(format!("cut\n{uri}")),
        1,
    );
    std::fs::write(&source, "occupied").unwrap();
    history_key(ui, "ctrl-z");
    click_choice(ui, "transfer-skip");
    redraw(ui);
    assert_eq!(std::fs::read_to_string(source).unwrap(), "occupied");
    assert_eq!(std::fs::read_to_string(target).unwrap(), "source");
}

#[gpui::test]
fn explorer_transfer_menu_undo_records_partial_success(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let completed = external.path().join("good.txt");
    std::fs::write(&completed, "good").unwrap();
    let (_, ui) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    transfer::paste(ui, vec![completed, external.path().join("missing.txt")], 0);
    assert!(project.path().join("good.txt").exists());
    assert!(!project.path().join("missing.txt").exists());
    let root = ui.debug_bounds("explorer-row-0").unwrap().center();
    ui.simulate_mouse_down(root, MouseButton::Right, Modifiers::default());
    ui.simulate_mouse_up(root, MouseButton::Right, Modifiers::default());
    redraw(ui);
    let undo = ui.update(|window, _| {
        gpui_base::test_support::snapshots(window)
            .into_iter()
            .find(|item| {
                item.role() == Some(gpui_kit::Role::MenuItem)
                    && item.label().is_some_and(|label| {
                        label.starts_with(t!("transfer.undo", operation = "").as_ref())
                            && label.contains("good.txt")
                    })
            })
            .expect("menu identifies the completed file operation")
            .bounds()
    });
    ui.simulate_click(undo.center(), Modifiers::default());
    redraw(ui);
    assert!(!project.path().join("good.txt").exists());
    history_key(ui, "ctrl-shift-z");
    assert_eq!(
        std::fs::read_to_string(project.path().join("good.txt")).unwrap(),
        "good"
    );
    assert!(!project.path().join("missing.txt").exists());
}
