//! Native Base link callbacks need a real press from the same portable scene before publication.
use super::*;
use gpui_kit::{TestAppContext, component::Root, gpui, point, px};
use plugin_runtime::plugin_protocol::ui::Node;
use std::cell::RefCell;

fn document(revision: u64, url: &str) -> Document {
    let mut document = Document::new(Node::rich_text(
        "link",
        format!("<p><a href=\"{url}\">链接</a></p>"),
    ))
    .revision(revision);
    document.link_events = true;
    document
}

/// MouseUp on a newly painted link cannot borrow the original press of its replaced scene.
#[gpui::test]
fn link_press_cannot_activate_a_replacement_scene(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::typography::init(cx);
        crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            PluginView::new(
                "test".into(),
                document(0, "https://old.example/"),
                Environment::default(),
                move |event, _| output.borrow_mut().push(event),
                window,
                cx,
            )
        });
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx.debug_bounds("plugin-ui-link").unwrap();
    let position = point(bounds.left() + px(8.), bounds.center().y);
    cx.simulate_mouse_down(position, gpui_kit::MouseButton::Left, Default::default());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.update_document(
                document(1, "https://new.example/"),
                Environment::default(),
                window,
                cx,
            )
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_up(position, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert!(
        events.borrow().is_empty(),
        "a stale press must not activate the new URI"
    );
    cx.simulate_mouse_up(position, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert!(events.borrow().is_empty(), "release alone is not a click");
    cx.simulate_click(position, Default::default());
    cx.run_until_parked();
    assert_eq!(events.borrow().len(), 1, "a fresh gesture remains usable");
    assert_eq!(events.borrow()[0].revision, 1);
    assert_eq!(
        events.borrow()[0].action,
        Action::Link {
            uri: "https://new.example/".into()
        }
    );
    events.borrow_mut().clear();
    cx.simulate_mouse_down(
        point(position.x + px(80.), position.y),
        gpui_kit::MouseButton::Left,
        Default::default(),
    );
    cx.simulate_mouse_up(position, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert!(
        events.borrow().is_empty(),
        "moving into a link is not its matching click"
    );
    // Releasing outside the original link ends the gesture, even if Base emits no link callback.
    cx.simulate_mouse_down(position, gpui_kit::MouseButton::Left, Default::default());
    cx.simulate_mouse_up(
        point(position.x + px(80.), position.y),
        gpui_kit::MouseButton::Left,
        Default::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_up(position, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert!(
        events.borrow().is_empty(),
        "a completed drag cannot be reused by a later release"
    );
    assert!(
        cx.opened_url().is_none(),
        "native UI only publishes an event"
    );
}
