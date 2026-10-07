//! Exercise typed tree gestures and platform drops through the mounted native tree.
use super::*;
use gpui_kit::{ExternalPaths, FileDropEvent, PlatformInput};

/// Drag a visible row using the same pointer sequence as a real mouse.
fn drag_row(ui: &mut VisualTestContext, from: &'static str, to: &'static str, copy: bool) {
    let start = ui.debug_bounds(from).unwrap().center();
    let end = ui.debug_bounds(to).unwrap().center();
    let modifiers = Modifiers {
        control: copy,
        ..Modifiers::default()
    };
    ui.simulate_mouse_down(start, MouseButton::Left, modifiers);
    ui.simulate_mouse_move(
        start + point(px(5.), px(0.)),
        Some(MouseButton::Left),
        modifiers,
    );
    redraw(ui);
    ui.simulate_mouse_move(end, Some(MouseButton::Left), modifiers);
    redraw(ui);
    ui.simulate_mouse_up(end, MouseButton::Left, modifiers);
    redraw(ui);
}

#[gpui::test]
fn explorer_transfer_tree_drag_moves_and_modifier_copies(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("destination")).unwrap();
    std::fs::write(project.path().join("a.txt"), "a").unwrap();
    std::fs::write(project.path().join("b.txt"), "b").unwrap();
    let (app, ui) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    let opened = ui.update(|_, cx| app.read(cx).tabs.len());
    drag_row(ui, "explorer-row-2", "explorer-row-1", true);
    assert!(project.path().join("a.txt").exists());
    assert!(project.path().join("destination/a.txt").exists());
    drag_row(ui, "explorer-row-3", "explorer-row-1", false);
    assert!(!project.path().join("b.txt").exists());
    assert!(project.path().join("destination/b.txt").exists());
    ui.update(|_, cx| assert_eq!(app.read(cx).tabs.len(), opened));
}

#[gpui::test]
fn explorer_transfer_external_drop_targets_file_parent(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("destination")).unwrap();
    let existing = project.path().join("destination/existing.txt");
    std::fs::write(&existing, "existing").unwrap();
    let source = external.path().join("one.txt");
    let source2 = external.path().join("two.txt");
    std::fs::write(&source, "one").unwrap();
    std::fs::write(&source2, "two").unwrap();
    let (_, ui) = mount(cx, Workspace::open(project.path()).unwrap(), existing);
    let position = ui.debug_bounds("explorer-row-2").unwrap().center();
    ui.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::FileDrop(FileDropEvent::Entered {
                position,
                paths: ExternalPaths(vec![source.clone(), source2.clone()].into()),
            }),
            cx,
        );
        window.draw(cx).clear(cx);
        window.dispatch_event(
            PlatformInput::FileDrop(FileDropEvent::Submit { position }),
            cx,
        );
    });
    redraw(ui);
    assert!(source.exists());
    assert!(source2.exists());
    assert!(project.path().join("destination/one.txt").exists());
    assert!(project.path().join("destination/two.txt").exists());
}

#[gpui::test]
fn explorer_transfer_drag_hover_cancel_and_edge_scroll(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join("destination/nested")).unwrap();
    for index in 0..60 {
        std::fs::write(project.path().join(format!("file{index:02}.txt")), "file").unwrap();
    }
    let (app, ui) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    let start = ui.debug_bounds("explorer-row-2").unwrap().center();
    let end = ui.debug_bounds("explorer-row-1").unwrap().center();
    ui.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    ui.simulate_mouse_move(
        start + point(px(5.), px(0.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    redraw(ui);
    ui.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    redraw(ui);
    ui.executor().advance_clock(Duration::from_millis(599));
    redraw(ui);
    assert!(ui.debug_bounds("explorer-row-1").is_some());
    ui.update(|_, cx| {
        assert!(
            !find_tree_item(
                &root_items(app.read(cx).tree_state.read(cx)),
                &project.path().canonicalize().unwrap().join("destination")
            )
            .unwrap()
            .is_expanded()
        )
    });
    ui.executor().advance_clock(Duration::from_millis(1));
    redraw(ui);
    ui.update(|_, cx| {
        assert!(
            find_tree_item(
                &root_items(app.read(cx).tree_state.read(cx)),
                &project.path().canonicalize().unwrap().join("destination")
            )
            .unwrap()
            .is_expanded()
        )
    });
    let edge = ui.debug_bounds("explorer-root").unwrap();
    let visible_last = |ui: &mut VisualTestContext| {
        (0..65)
            .filter(|index| {
                let selector = Box::leak(format!("explorer-row-{index}").into_boxed_str());
                ui.debug_bounds(selector)
                    .is_some_and(|bounds| edge.contains(&bounds.center()))
            })
            .last()
            .unwrap()
    };
    let before_scroll = visible_last(ui);
    ui.simulate_mouse_move(
        point(edge.center().x, edge.bottom() - px(3.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    for _ in 0..4 {
        ui.executor().advance_clock(Duration::from_millis(80));
        redraw(ui);
    }
    assert!(
        visible_last(ui) > before_scroll,
        "edge scrolling reveals previously hidden rows"
    );
    ui.simulate_keystrokes("escape");
    redraw(ui);
    ui.update(|_, cx| {
        let states = expansion_state(app.read(cx), cx);
        assert!(
            states[0].1,
            "the pre-existing open project root remains open"
        );
        assert!(
            !states[1].1,
            "only the temporary hover expansion is restored"
        );
    });
    assert!(project.path().join("file00.txt").exists());
    assert!(!project.path().join("destination/file00.txt").exists());
}

/// Leaving a folder cancels its timer; returning starts a fresh full hover interval.
#[gpui::test]
fn explorer_transfer_drag_hover_restarts_after_leaving(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join("destination/nested")).unwrap();
    std::fs::write(project.path().join("source.txt"), "source").unwrap();
    let (app, ui) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    let start = ui.debug_bounds("explorer-row-2").unwrap().center();
    let folder = ui.debug_bounds("explorer-row-1").unwrap().center();
    let blank = ui.debug_bounds("explorer-root").unwrap().bottom_left() + point(px(40.), px(-80.));
    ui.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    ui.simulate_mouse_move(
        start + point(px(5.), px(0.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    ui.simulate_mouse_move(folder, Some(MouseButton::Left), Modifiers::default());
    ui.executor().advance_clock(Duration::from_millis(300));
    ui.simulate_mouse_move(blank, Some(MouseButton::Left), Modifiers::default());
    ui.executor().advance_clock(Duration::from_millis(400));
    ui.simulate_mouse_move(folder, Some(MouseButton::Left), Modifiers::default());
    ui.executor().advance_clock(Duration::from_millis(600));
    redraw(ui);
    ui.update(|_, cx| {
        assert!(
            find_tree_item(
                &root_items(app.read(cx).tree_state.read(cx)),
                &project.path().canonicalize().unwrap().join("destination"),
            )
            .unwrap()
            .is_expanded()
        );
    });
    ui.simulate_keystrokes("escape");
}

/// A stationary pointer follows the freshly drawn folder after automatic edge scrolling.
#[gpui::test]
fn explorer_transfer_drop_rechecks_target_after_edge_scroll(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let source = external.path().join("import.txt");
    std::fs::write(&source, "import").unwrap();
    for index in 0..60 {
        std::fs::create_dir(project.path().join(format!("folder{index:02}"))).unwrap();
    }
    let (_, ui) = mount(cx, Workspace::open(project.path()).unwrap(), None);
    let tree = ui.debug_bounds("explorer-root").unwrap();
    let position = point(tree.center().x, tree.bottom() - px(6.));
    let folder_at = |ui: &mut VisualTestContext| {
        (1..=60)
            .find(|index| {
                let selector = Box::leak(format!("explorer-row-{index}").into_boxed_str());
                ui.debug_bounds(selector)
                    .is_some_and(|bounds| bounds.contains(&position))
            })
            .expect("the pointer must be on an actually visible folder")
            - 1
    };
    let first = folder_at(ui);
    ui.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::FileDrop(FileDropEvent::Entered {
                position,
                paths: ExternalPaths(vec![source.clone()].into()),
            }),
            cx,
        );
    });
    redraw(ui);
    for _ in 0..4 {
        ui.executor().advance_clock(Duration::from_millis(80));
        redraw(ui);
    }
    let current = folder_at(ui);
    assert_ne!(first, current);
    ui.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::FileDrop(FileDropEvent::Submit { position }),
            cx,
        );
    });
    redraw(ui);
    assert!(
        project
            .path()
            .join(format!("folder{current:02}/import.txt"))
            .exists()
    );
    assert!(
        !project
            .path()
            .join(format!("folder{first:02}/import.txt"))
            .exists()
    );
    assert!(source.exists());
}
