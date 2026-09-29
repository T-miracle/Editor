use super::*;
use crate::theme::{apply_theme, builtin_theme};
use editor_core::Workspace;
use gpui_kit::{AppContext as _, TestAppContext, component::Root, gpui};
use std::{cell::RefCell, rc::Rc};

#[test]
fn menu_position_stays_inside_window() {
    let (x, y) = menu_position(
        point(px(950.), px(720.)),
        size(px(1000.), px(760.)),
        MENU_WIDTH,
        160.,
    );
    assert!(x + px(MENU_WIDTH) <= px(1000.));
    assert!(y + px(160.) <= px(760.));
}

#[gpui::test]
fn builtin_themes_define_explorer_menu_colors(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        for dark in [false, true] {
            apply_theme(builtin_theme(dark), cx);
            let styles = component_styles(cx, ThemeComponent::ExplorerMenu);
            assert!(styles.base.background.is_some());
            assert!(styles.base.border.is_some());
            assert!(styles.hover.background.is_some());
        }
    });
}

#[gpui::test]
fn empty_explorer_opens_project_menu_and_new_submenu(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, window_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    window_cx.simulate_resize(size(px(800.), px(600.)));
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let position = point(px(90.), px(140.));
    window_cx.simulate_mouse_down(position, MouseButton::Right, Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(window_cx.update(|_, cx| app.read(cx).explorer_menu.is_some()));
    assert!(window_cx.debug_bounds("explorer-context-menu").is_some());
    let new = window_cx.debug_bounds("explorer-menu-new").unwrap();
    window_cx.simulate_click(new.center(), Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(window_cx.debug_bounds("explorer-new-submenu").is_some());
}

#[gpui::test]
fn right_clicked_file_is_the_menu_target(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("note.txt");
    std::fs::write(&file, "note").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, window_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    window_cx.simulate_resize(size(px(800.), px(600.)));
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let row = window_cx.debug_bounds("explorer-row-0").unwrap();
    window_cx.simulate_mouse_down(row.center(), MouseButton::Right, Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let target = window_cx.update(|_, cx| {
        app.read(cx)
            .explorer_menu
            .as_ref()
            .and_then(|menu| menu.target.clone())
    });
    assert_eq!(target, Some(file.canonicalize().unwrap()));
    assert!(window_cx.debug_bounds("explorer-menu-rename").is_some());
}
