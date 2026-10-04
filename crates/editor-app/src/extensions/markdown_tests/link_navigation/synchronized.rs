//! Real navigation completions and source publications can arrive in either order during sync.
use super::super::super::composable_tests::{publish, pump};
use super::*;
use protocol::api::{
    EditorOperation, EditorValue, NavigationTarget, RequestUpdate, ViewportTarget,
};

/// Execute the guest's owned open request, then deliver its real completion and preview separately.
/// Reordering existing queues exercises the worker seam without inventing a result or host API.
fn open_fragment_in_order(
    fixture: &mut NativeMarkdown,
    ui: &mut gpui_kit::VisualTestContext,
    opened_first: bool,
    target: &str,
) {
    assert!(
        fixture
            .manager
            .live
            .get_mut("markdown")
            .unwrap()
            .take_editor_requests()
            .is_empty(),
        "the previous native scene is settled"
    );
    let bounds = ui.debug_bounds("plugin-ui-b-0-paragraph").unwrap();
    ui.simulate_click(
        point(bounds.left() + px(12.), bounds.center().y),
        Default::default(),
    );
    ui.run_until_parked();
    pump(&mut fixture.manager, &fixture.app, ui);
    let mut requests = fixture
        .manager
        .live
        .get_mut("markdown")
        .unwrap()
        .take_editor_requests();
    assert_eq!(requests.len(), 1, "one actual link issues one owned open");
    let request = requests.pop().unwrap();
    assert!(
        matches!(request.operation(), EditorOperation::NavigateDocument {
        target: NavigationTarget::RelativeDocument { path }, ..
    } if path == target)
    );
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
            .push(("markdown".into(), request.clone()));
    });
    publish(
        &mut fixture.manager,
        &mut fixture.renderer,
        &fixture.app,
        ui,
    );
    assert!(
        matches!(request.status(), RequestUpdate::Completed {
        result: Ok(EditorValue::Opened { ref document })
    } if document.path == target),
        "the production editor completes the actual open"
    );

    if opened_first {
        fixture.manager.poll();
        assert_ne!(
            fixture.manager.live["markdown"].views["preview"]
                .source
                .as_ref()
                .unwrap()
                .path,
            target
        );
        pump(&mut fixture.manager, &fixture.app, ui);
    } else {
        pump(&mut fixture.manager, &fixture.app, ui);
        assert_eq!(
            fixture.manager.live["markdown"].views["preview"]
                .source
                .as_ref()
                .unwrap()
                .path,
            target
        );
        fixture.manager.poll();
    }
    fixture.settle(ui);
}

/// Read the mapped block actually nearest the native viewport top, rather than assuming an EOF title is first.
fn visible_anchor(
    fixture: &NativeMarkdown,
    ui: &mut gpui_kit::VisualTestContext,
) -> protocol::ui::SourceRange {
    let viewport = ui.debug_bounds("plugin-ui-preview-scroll").unwrap();
    let mut candidates = Vec::new();
    fixture.manager.live["markdown"].views["preview"]
        .root
        .visit(&mut |node| {
            if let Some(range) = node.source_range {
                let selector = Box::leak(format!("plugin-ui-{}", node.id).into_boxed_str());
                if let Some(bounds) = ui.debug_bounds(selector)
                    && bounds.bottom() > viewport.top()
                    && bounds.top() < viewport.bottom()
                {
                    candidates.push((range, (bounds.top() - viewport.top()).abs()));
                }
            }
        });
    candidates
        .into_iter()
        .min_by(|left, right| left.1.partial_cmp(&right.1).unwrap())
        .expect("actual visible mapped native block")
        .0
}

/// Initial source probes cannot replace either arrival order's actual middle/EOF navigation.
/// Resizing preserves this fixture's short top block and its source correspondence, not a pinned EOF title.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_fragment_sync_preserves_both_arrival_orders_and_clamped_tail(
    cx: &mut TestAppContext,
) {
    let middle = format!(
        "{}# 目标\n\n{}",
        "目标前置段落。\n\n".repeat(70),
        "目标尾段。\n\n".repeat(20)
    );
    let tail = format!("{}# 目标\n", "目标前置段落。\n\n".repeat(70));
    let first = "[打开](middle.md#%E7%9B%AE%E6%A0%87)\n";
    let second = "[打开](tail.md#%E7%9B%AE%E6%A0%87)\n";
    assert_eq!(middle.find("# 目标"), Some(1610));
    assert_eq!(tail.find("# 目标"), Some(1610));
    let (mut fixture, ui) = NativeMarkdown::mount(
        cx,
        &[
            ("first.md", first),
            ("second.md", second),
            ("middle.md", &middle),
            ("tail.md", &tail),
        ],
    );
    for (source, target, expected, opened_first) in [
        ("first.md", "middle.md", middle.as_str(), true),
        ("second.md", "tail.md", tail.as_str(), false),
    ] {
        ui.simulate_resize(size(px(1400.), px(900.)));
        fixture.open(source, ui);
        open_fragment_in_order(&mut fixture, ui, opened_first, target);
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
        let mut previous_anchor = None;
        for (index, dimensions) in [size(px(1400.), px(900.)), size(px(980.), px(700.))]
            .into_iter()
            .enumerate()
        {
            ui.simulate_resize(dimensions);
            fixture.settle(ui);
            let heading = ui
                .debug_bounds("plugin-ui-b-1610-heading")
                .expect("native target heading");
            let pane = ui.debug_bounds("editor-preview-pane").unwrap();
            if index == 0 || opened_first {
                assert!(
                    heading.top() >= pane.top() && heading.bottom() <= pane.bottom(),
                    "initial navigation is visible in both arrival orders: {heading:?}, {pane:?}"
                );
            }
            let anchor = visible_anchor(&fixture, ui);
            if let Some(previous) = previous_anchor {
                assert_eq!(
                    anchor, previous,
                    "short nonwrapping blocks retain their native top anchor"
                );
            }
            previous_anchor = Some(anchor);
            ui.update(|_, cx| {
                let editor = fixture.app.read(cx).editor.read(cx);
                let first_row = editor.text().offset_to_point(anchor.start).row;
                let last_row = editor
                    .text()
                    .offset_to_point(anchor.end.saturating_sub(1))
                    .row;
                let visible = editor.visible_row_range().unwrap();
                assert!(
                    first_row < visible.end && last_row >= visible.start,
                    "actual preview range {anchor:?} must intersect source rows {visible:?}"
                );
            });
            assert_eq!(
                ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
                expected
            );
        }
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join(target)).unwrap(),
            expected
        );
        assert!(ui.opened_url().is_none());
    }
}

/// A queued source-driven locate is cancelled while its original source is still active.
/// Replaying the same owned handle proves cancellation, rather than a later stale-document refusal.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_link_cancels_queued_sync_before_opening_target(cx: &mut TestAppContext) {
    let source =
        "[打开](next.md#%E7%9B%AE%E6%A0%87)\n\n".to_owned() + &"源侧长段落。\n\n".repeat(70);
    let target = format!(
        "{}# 目标\n\n{}",
        "目标前置段落。\n\n".repeat(70),
        "目标尾段。\n\n".repeat(20)
    );
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", &source), ("next.md", &target)]);
    fixture.open("notes.md", ui);
    let position = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).input_bounds().center());
    ui.simulate_event(gpui_kit::ScrollWheelEvent {
        position,
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-350.))),
        ..Default::default()
    });
    ui.run_until_parked();
    pump(&mut fixture.manager, &fixture.app, ui);
    let mut requests = fixture
        .manager
        .live
        .get_mut("markdown")
        .unwrap()
        .take_editor_requests();
    assert_eq!(
        requests.len(),
        1,
        "real source wheel issues one coalesced locate"
    );
    let locate = requests.pop().unwrap();
    assert!(matches!(
        locate.operation(),
        EditorOperation::LocateViewport {
            target: ViewportTarget::Preview { .. },
            ..
        }
    ));
    let link = ui.debug_bounds("plugin-ui-b-0-paragraph").unwrap();
    let pane = ui.debug_bounds("editor-preview-pane").unwrap();
    assert!(
        link.top() >= pane.top() && link.bottom() <= pane.bottom(),
        "the withheld locate leaves the link visible"
    );
    ui.simulate_click(
        point(link.left() + px(12.), link.center().y),
        Default::default(),
    );
    ui.run_until_parked();
    pump(&mut fixture.manager, &fixture.app, ui);
    assert!(
        matches!(locate.status(), RequestUpdate::Cancelled { .. }),
        "navigation must cancel the existing owned locate before any open"
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).active_path.clone()),
        Some(
            fixture
                .directory
                .path()
                .join("notes.md")
                .canonicalize()
                .unwrap()
        )
    );
    let mut requests = fixture
        .manager
        .live
        .get_mut("markdown")
        .unwrap()
        .take_editor_requests();
    assert_eq!(requests.len(), 1);
    let open = requests.pop().unwrap();
    assert!(
        matches!(open.operation(), EditorOperation::NavigateDocument {
        target: NavigationTarget::RelativeDocument { path }, ..
    } if path == "next.md")
    );
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
            .push(("markdown".into(), locate.clone()));
    });
    publish(
        &mut fixture.manager,
        &mut fixture.renderer,
        &fixture.app,
        ui,
    );
    assert!(matches!(locate.status(), RequestUpdate::Cancelled { .. }));
    assert_eq!(
        ui.debug_bounds("plugin-ui-b-0-paragraph").unwrap(),
        link,
        "a late replay of the cancelled locate cannot move the old preview"
    );
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
            .push(("markdown".into(), open.clone()));
    });
    publish(
        &mut fixture.manager,
        &mut fixture.renderer,
        &fixture.app,
        ui,
    );
    fixture.settle(ui);
    let heading = ui.debug_bounds("plugin-ui-b-1610-heading").unwrap();
    let pane = ui.debug_bounds("editor-preview-pane").unwrap();
    assert!(heading.top() >= pane.top() && heading.bottom() <= pane.bottom());
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        target
    );
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("notes.md")).unwrap(),
        source
    );
}
