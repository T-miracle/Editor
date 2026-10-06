//! Scroll regressions inspect the exact typed native operation produced by shared guest geometry decisions.
//! Actual package/native tests cover request cancellation and layout; these pure traces cover driver ownership.
use super::*;

/// Small source ranges keep every projected byte offset independently checkable, including an EOF-clamped viewport.
fn fixture() -> (Source, Vec<ui::Node>) {
    let source = Source {
        version: api::DocumentVersion {
            id: "native".into(),
            path: "notes.md".into(),
            revision: 7,
        },
        text: "前段\n\n目标中文段落\n\n尾部标题\n".into(),
    };
    let target = source.text.find("目标").unwrap();
    let tail = source.text.find("尾部").unwrap();
    let blocks = vec![
        ui::Node::text("first", "前段").source_range(0..target),
        ui::Node::text("target", "目标中文段落").source_range(target..tail),
        ui::Node::text("last-heading", "尾部标题").source_range(tail..source.text.len()),
    ];
    (source, blocks)
}

/// Source layout and source translation share authority, but only the latter can replace preview priority.
fn source_position(source: &Source, layout: bool, revision: u64) -> api::SourceViewport {
    api::SourceViewport {
        document: source.version.clone(),
        ui_revision: revision,
        offset: 0,
        line_fraction: 0.0,
        origin: None,
        layout,
    }
}

/// The actual top-visible block can precede an EOF heading; no test assumes the heading itself is the top block.
fn preview_position(block: &ui::Node, layout: bool) -> api::PreviewViewport {
    api::PreviewViewport {
        block: block.id.clone(),
        source_range: block.source_range.unwrap(),
        fraction: 0.0,
        origin: None,
        layout,
    }
}

/// Deferred operations after a link must follow actual preview geometry, never move the preview back to source top.
fn assert_source_target(projection: Option<Projection>, offset: usize) {
    assert!(
        matches!(projection.and_then(|projection| projection.intent).map(|intent| intent.target),
            Some(api::ViewportTarget::Source { offset: actual, .. }) if actual == offset),
        "navigation-owned geometry must only locate the source at {offset}"
    );
}

/// Returning to A while A is pending must discard B rather than scroll to it after A completes.
#[test]
fn scrolling_latest_manual_position_cancels_an_obsolete_deferred_intent() {
    let first = Intent {
        document: api::DocumentVersion {
            id: "native".into(),
            path: "notes.md".into(),
            revision: 7,
        },
        ui_revision: 4,
        target: api::ViewportTarget::Preview {
            node: "a".into(),
            fraction: 0.0,
        },
    };
    let mut second = first.clone();
    second.target = api::ViewportTarget::Preview {
        node: "b".into(),
        fraction: 0.5,
    };
    let mut latest = None;
    coalesce(&first, &mut latest, second.clone());
    assert!(latest.as_ref() == Some(&second));
    coalesce(&first, &mut latest, first.clone());
    assert!(latest.is_none(), "A→B→A retains A, not the obsolete B");
}

/// Unicode endpoints and blank gaps remain tied to the closest semantic block.
#[test]
fn scrolling_maps_blocks_and_unicode_boundaries_without_document_percentages() {
    let (source, blocks) = fixture();
    let target = &blocks[1];
    let range = target.source_range.unwrap();
    let mut anchor = source_position(&source, false, 4);
    anchor.offset = range.start;
    assert!(
        matches!(source_intent(&anchor, &source, &blocks, 4).unwrap().target,
            api::ViewportTarget::Preview { node, fraction } if node == "target" && fraction == 0.0)
    );
    for fraction in [0.0, 0.13, 0.5, 0.87, 1.0] {
        let preview = api::PreviewViewport {
            fraction,
            ..preview_position(target, false)
        };
        let api::ViewportTarget::Source { offset, .. } =
            preview_intent(&preview, &source, 4).unwrap().target
        else {
            panic!("source target")
        };
        assert!(
            (range.start..=range.end).contains(&offset) && source.text.is_char_boundary(offset)
        );
    }
}

/// A link discards deferred automatic work and suppresses initial source layout until preview geometry exists.
/// Real accepted-handle cancellation is covered through the public Manager/native integration boundary.
#[test]
fn navigation_discards_deferred_locations_and_suppresses_initial_source_layout() {
    let (source, blocks) = fixture();
    let mut scrolling = Scrolling::default();
    scrolling.latest = project_source(
        &mut scrolling.driver,
        &source_position(&source, true, 4),
        &source,
        &blocks,
        4,
    )
    .unwrap()
    .intent;
    assert!(scrolling.latest.is_some());
    scrolling.navigate();
    assert!(scrolling.pending.is_none() && scrolling.latest.is_none());
    assert!(!scrolling.request(&api::Notification::Request {
        handle: api::ResourceHandle {
            instance: "scroll-test".into(),
            scope: "workspace".into(),
            resource: 7,
        },
        update: api::RequestUpdate::Completed {
            result: Ok(api::EditorValue::Unit),
        },
    }));
    let projection = project_source(
        &mut scrolling.driver,
        &source_position(&source, true, 4),
        &source,
        &blocks,
        4,
    )
    .unwrap();
    assert!(
        projection.intent.is_none(),
        "initial source layout cannot override the link"
    );
}

/// Initial target layout may precede reveal, and the revealed frame may itself still be classified as layout.
#[test]
fn navigation_preview_layout_and_clamped_geometry_only_locate_source() {
    let (source, blocks) = fixture();
    for actual_top in [&blocks[1], &blocks[2]] {
        for layout in [true, false] {
            let mut scrolling = Scrolling::default();
            scrolling.navigate();
            assert_source_target(
                project_preview(
                    &mut scrolling.driver,
                    &preview_position(&blocks[0], true),
                    &source,
                    &blocks,
                    4,
                ),
                0,
            );
            // For an EOF heading, actual_top can be its preceding paragraph rather than the requested heading.
            let offset = actual_top.source_range.unwrap().start;
            assert_source_target(
                project_preview(
                    &mut scrolling.driver,
                    &preview_position(actual_top, layout),
                    &source,
                    &blocks,
                    4,
                ),
                offset,
            );
            assert_source_target(
                project_source(
                    &mut scrolling.driver,
                    &source_position(&source, true, 4),
                    &source,
                    &blocks,
                    4,
                ),
                offset,
            );
        }
    }
}

/// Scene refresh cancels old requests but preserves a navigation's current preview anchor across reflow.
#[test]
fn navigation_scene_refresh_keeps_preview_priority_without_fixed_frame_counts() {
    let (source, blocks) = fixture();
    let mut scrolling = Scrolling::default();
    scrolling.navigate();
    project_preview(
        &mut scrolling.driver,
        &preview_position(&blocks[1], true),
        &source,
        &blocks,
        4,
    );
    scrolling.refresh(true);
    assert!(scrolling.pending.is_none() && scrolling.latest.is_none());
    let projection = project_source(
        &mut scrolling.driver,
        &source_position(&source, true, 5),
        &source,
        &blocks,
        5,
    );
    assert_eq!(
        projection
            .as_ref()
            .unwrap()
            .intent
            .as_ref()
            .unwrap()
            .ui_revision,
        5
    );
    assert_source_target(projection, blocks[1].source_range.unwrap().start);
}

/// Source input and preview input can both take the wheel again after a reveal's actual geometry arrives.
#[test]
fn navigation_allows_both_manual_sides_to_resume_bidirectional_scrolling() {
    let (source, blocks) = fixture();
    let mut scrolling = Scrolling::default();
    scrolling.navigate();
    project_preview(
        &mut scrolling.driver,
        &preview_position(&blocks[2], true),
        &source,
        &blocks,
        4,
    );
    let source_translation = project_source(
        &mut scrolling.driver,
        &source_position(&source, false, 4),
        &source,
        &blocks,
        4,
    )
    .unwrap();
    assert!(source_translation.translated);
    assert!(
        source_translation.takeover,
        "old Source locations cannot later move the new source driver"
    );
    assert!(matches!(
        source_translation.intent.as_ref().map(|intent| &intent.target),
        Some(api::ViewportTarget::Preview { node, .. }) if node == "first"
    ));
    let preview_translation = project_preview(
        &mut scrolling.driver,
        &preview_position(&blocks[1], false),
        &source,
        &blocks,
        4,
    )
    .unwrap();
    assert!(preview_translation.translated);
    assert!(
        preview_translation.takeover,
        "old Preview locations cannot later move the new preview driver"
    );
    assert_source_target(
        Some(preview_translation),
        blocks[1].source_range.unwrap().start,
    );
}

/// Receipt-marked translation never changes the driver; released navigation restores ordinary initial alignment.
#[test]
fn navigation_ignores_program_receipts_and_preserves_ordinary_initial_alignment() {
    let (source, blocks) = fixture();
    let mut scrolling = Scrolling::default();
    scrolling.navigate();
    let mut position = source_position(&source, false, 4);
    position.origin = Some(12);
    assert!(project_source(&mut scrolling.driver, &position, &source, &blocks, 4).is_none());
    let mut preview = preview_position(&blocks[2], false);
    preview.origin = Some(12);
    assert!(project_preview(&mut scrolling.driver, &preview, &source, &blocks, 4).is_none());
    assert!(scrolling.latest.is_none());
    scrolling.refresh(false);
    let projection = project_source(
        &mut scrolling.driver,
        &source_position(&source, true, 4),
        &source,
        &blocks,
        4,
    )
    .unwrap();
    assert!(matches!(
        projection.intent.as_ref().map(|intent| &intent.target),
        Some(api::ViewportTarget::Preview { node, .. }) if node == "first"
    ));
}
