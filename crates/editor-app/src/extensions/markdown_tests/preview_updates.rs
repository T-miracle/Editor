//! Native preview updates keep the published tree readable and coalesce typing bursts.
use super::*;
use harness::NativeMarkdown;

/// New source offsets must wait for the matching preview revision, even while its predecessor stays visible.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_typing_does_not_stamp_new_offsets_with_old_scene(cx: &mut TestAppContext) {
    let source = "# Heading\n\nParagraph\n\n".repeat(40);
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &source)]);
    fixture.open("notes.md", ui);
    fixture.focus_editor(ui);
    fixture.settle(ui);
    let worker = ui.update(|_, cx| fixture.app.read(cx).extensions.read(cx).worker.clone());
    let _ = worker
        .recorded
        .lock()
        .unwrap()
        .try_iter()
        .collect::<Vec<_>>();
    ui.update(|window, cx| {
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("中").unwrap(), cx);
    });
    ui.update(|window, cx| window.draw(cx).clear(cx));
    // Do not advance the cadence or pump the guest: the visible scene deliberately still denotes revision 0.
    let work = worker
        .recorded
        .lock()
        .unwrap()
        .try_iter()
        .collect::<Vec<_>>();
    assert!(
        !work.iter().any(|work| matches!(
            work,
            Work::Event(_, _, _, protocol::api::Notification::SourceViewport(_))
        )),
        "a source measurement must not borrow the previous preview's document revision"
    );
    // Return unrelated work to the normal public transport, then verify publication catches up.
    for work in work {
        worker.tx.send(work).unwrap();
    }
    fixture.settle(ui);
    let (sent, published) = preview_state(&fixture, ui);
    assert_eq!(sent, published);
    assert!(sent.is_some_and(|revision| revision > 0));
}

/// The revision this panel last sent to the guest, and the revision the guest published back.
fn preview_state(
    fixture: &NativeMarkdown,
    ui: &mut gpui_kit::VisualTestContext,
) -> (Option<u64>, Option<u64>) {
    ui.update(|_, cx| {
        fixture.app.read(cx).plugin_panels["markdown/preview"]
            .read(cx)
            .sent_and_published_revision()
    })
}

/// A displayed preview keeps its tree while the newer revision crosses the guest boundary.
/// Typing must therefore never blank the pane, and intermediate revisions must coalesce.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_typing_keeps_the_preview_published(cx: &mut TestAppContext) {
    let source = format!(
        "# 标题\n\n段落\n\n| 验收 | 结果 |\n| --- | --- |\n| T01 | `language_tests` 高亮 |\n\n{}",
        (0..40)
            .map(|index| format!("段落 {index:03}：实时预览跟随输入。\n\n"))
            .collect::<String>()
    );
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &source)]);
    fixture.open("notes.md", ui);
    let revision = |fixture: &NativeMarkdown, ui: &mut gpui_kit::VisualTestContext| {
        ui.update(|_, cx| {
            fixture.app.read(cx).plugin_panels["markdown/preview"]
                .read(cx)
                .current_document()
                .map(|document| document.revision)
        })
    };
    let displayed = revision(&fixture, ui).expect("first publication is displayed");
    let (sent, published) = preview_state(&fixture, ui);
    assert_eq!((sent, published), (Some(0), Some(0)));
    assert!(ui.debug_bounds("plugin-ui-b-0-heading").is_some());
    fixture.focus_editor(ui);
    // Four characters in one burst: no guest answer is delivered between them, exactly as when
    // the WASM parse is slower than typing.
    for character in ["甲", "乙", "丙", "丁"] {
        ui.simulate_input(character);
        ui.run_until_parked();
        assert!(
            ui.debug_bounds("plugin-ui-b-0-heading").is_some(),
            "typing {character} must not withdraw the displayed preview"
        );
        assert!(
            revision(&fixture, ui).is_some(),
            "typing {character} left the preview unpublished"
        );
    }
    let (sent, published) = preview_state(&fixture, ui);
    assert_eq!(
        sent,
        Some(0),
        "the bounded publication timer absorbs the whole initial typing burst"
    );
    assert_eq!(
        published,
        Some(0),
        "the guest cannot answer a publication it has not received"
    );
    // The guest answer republishes the newest revision, so a burst still ends at the last character.
    for _ in 0..3 {
        fixture.settle(ui);
        ui.run_until_parked();
        ui.update(|window, cx| window.draw(cx).clear(cx));
    }
    let (sent, published) = preview_state(&fixture, ui);
    assert_eq!(sent, Some(4), "the newest revision is published once");
    assert_eq!(published, Some(4), "the guest answers the newest revision");
    assert!(revision(&fixture, ui).is_some_and(|next| next > displayed));
    // The preview still owns its scroll node: the publication replaced content in place.
    assert!(ui.debug_bounds("plugin-ui-preview-scroll").is_some());
    let text = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string());
    assert!(text.starts_with("甲乙丙丁"), "typed text: {text:.20}");
    assert!(text.contains("# 标题"));
}

/// Every table cell is an independent read-only block with its own native layout.
/// One generated `<table>` used to share native inline state between cells, which painted the
/// code surfaces of the earlier cells without their glyphs.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_table_cells_keep_independent_blocks(cx: &mut TestAppContext) {
    let source = "| 验收 | 当前验证入口 |\n| --- | --- |\n| T01 | `language_tests`：高亮出现 |\n| T02 | `lsp_tests` 与 `dependency_tests` |\n| T03 | 普通文本 |\n";
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("table.md", source)]);
    fixture.open("table.md", ui);
    let scene = fixture.manager.live["markdown"].views["preview"].clone();
    let mut cells = Vec::new();
    scene.root.visit(&mut |node| {
        assert!(
            !matches!(&node.kind, protocol::ui::Kind::RichText { html } if html.contains("<table")),
            "a table must be composed from native blocks instead of one generated HTML table"
        );
        if let protocol::ui::Kind::RichText { html } = &node.kind
            && node.id.contains("table-cell-")
        {
            assert!(
                html.contains("</p>"),
                "a cell carries its own rich paragraph"
            );
            cells.push(node.id.clone());
        }
    });
    assert_eq!(
        cells.len(),
        8,
        "every header and body cell keeps its own rich block"
    );
    // Each cell is laid out natively: a shared element would collapse or overlap these bounds.
    let mut rows = Vec::new();
    for (index, id) in cells.iter().enumerate() {
        let selector: &'static str = Box::leak(format!("plugin-ui-{id}").into_boxed_str());
        let bounds = ui
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("cell {id} has no native layout"));
        assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
        if index % 2 == 0 {
            rows.push(bounds.top());
        }
    }
    assert!(
        rows.windows(2).all(|pair| pair[0] < pair[1]),
        "table rows must stack instead of sharing one inline element: {rows:?}"
    );
    let serialized = serde_json::to_string(scene.as_ref()).unwrap();
    for expected in ["language_tests", "dependency_tests", "普通文本"] {
        assert!(serialized.contains(expected), "missing {expected}");
    }
}
