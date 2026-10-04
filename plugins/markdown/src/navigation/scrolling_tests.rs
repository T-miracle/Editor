//! Navigation/scroll ownership traces use real SDK receipt correlation without native host imports.
//! Native package regressions cover the actual two arrival orders, reveal paint, EOF clamping and cancellation.
use super::*;
use crate::State;

/// Identity fixtures distinguish a real target receipt from a guessed path or a reopened document.
fn document(id: &str, path: &str, revision: u64) -> api::DocumentVersion {
    api::DocumentVersion {
        id: id.into(),
        path: path.into(),
        revision,
    }
}

/// Accepted handles exercise SDK correlation only; these pure tests never invoke imported start/cancel functions.
fn accepted(document: &api::DocumentVersion, phase: Phase) -> (Navigation, api::ResourceHandle) {
    let handle = api::ResourceHandle {
        instance: "navigation-scroll".into(),
        scope: "workspace".into(),
        resource: 17,
    };
    (
        Navigation {
            pending: Some(Pending {
                task: api::guest::EditorTask::from_accepted(handle.clone()),
                document: document.clone(),
                phase,
            }),
            viewport: Some(document.clone()),
            ..Default::default()
        },
        handle,
    )
}

/// Unit acknowledges a queued effect, so later source layout/scene refresh must not restore a Source driver.
#[test]
fn queued_reveal_unit_retains_preview_priority_until_real_source_input() {
    let version = document("target", "next.md", 8);
    let (navigation, handle) = accepted(&version, Phase::Revealing);
    let mut state = State {
        source: Some(Source {
            version: version.clone(),
            text: "# 目标\n".into(),
        }),
        navigation,
        ..Default::default()
    };
    state.refresh();
    state.event(
        Some("preview"),
        api::Notification::Request {
            handle,
            update: api::RequestUpdate::Completed {
                result: Ok(api::EditorValue::Unit),
            },
        },
    );
    assert!(state.navigation.pending.is_none());
    assert!(state.navigation.controls_viewport(Some(&version)));
    // Source-first measurement produces no host operation before preview geometry exists.
    assert!(state.viewport_event(
        Some("preview"),
        &api::Notification::SourceViewport(api::SourceViewport {
            document: version.clone(),
            ui_revision: state.revision,
            offset: 0,
            line_fraction: 0.0,
            origin: None,
            layout: true,
        }),
    ));
    state.refresh();
    assert!(state.navigation.controls_viewport(Some(&version)));
    state.navigation.preview_drives();
    assert!(state.navigation.controls_viewport(Some(&version)));
    state.navigation.source_drives();
    assert!(!state.navigation.controls_viewport(Some(&version)));
}

/// Receipt-first ownership moves only after its exact target Preview; preview-first ownership awaits its receipt.
#[test]
fn relative_fragment_priority_follows_both_actual_identity_arrival_orders() {
    let origin = document("origin", "notes.md", 3);
    let target = document("target", "next.md", 8);
    for preview_first in [false, true] {
        let (mut navigation, handle) = accepted(
            &origin,
            Phase::Opening {
                fragment: Some("目标".into()),
                transition: Opening::default(),
            },
        );
        if preview_first {
            navigation.source_changed(Some(&target));
            assert!(!navigation.controls_viewport(Some(&origin)));
            assert!(navigation.controls_viewport(Some(&target)));
            // The shared identity state admits the receipt; starting its actual reveal requires the WASM fixture.
            let pending = navigation.pending.as_ref().unwrap();
            let Phase::Opening { transition, .. } = &pending.phase else {
                panic!("the real receipt is still awaited");
            };
            assert!(matches!(
                transition.complete(&origin, &target, Some(&target)),
                Ok(Arrival::Ready)
            ));
        } else {
            let source = Source {
                version: origin.clone(),
                text: "[目标](next.md#目标)\n".into(),
            };
            assert!(!navigation.request(
                &api::Notification::Request {
                    handle,
                    update: api::RequestUpdate::Completed {
                        result: Ok(api::EditorValue::Opened {
                            document: target.clone(),
                        }),
                    },
                },
                Some(&source),
                &Index::default(),
                &[],
                4,
            ));
            assert!(navigation.pending.is_none() && navigation.waiting.is_some());
            assert!(
                !navigation.controls_viewport(Some(&target)),
                "the path receipt alone is not a rendered source"
            );
            navigation.source_changed(Some(&target));
            assert!(navigation.controls_viewport(Some(&target)));
        }
    }
}

/// Even after Unit, source edits, closure, unrelated documents and reopened identities revoke preview priority.
#[test]
fn queued_reveal_priority_is_revoked_by_edit_close_switch_or_reopen() {
    let target = document("target", "next.md", 8);
    for next in [
        None,
        Some(document("target", "next.md", 9)),
        Some(document("other", "other.md", 1)),
        Some(document("reopened", "next.md", 8)),
    ] {
        let (mut navigation, handle) = accepted(&target, Phase::Revealing);
        let source = Source {
            version: target.clone(),
            text: "# 目标\n".into(),
        };
        navigation.request(
            &api::Notification::Request {
                handle,
                update: api::RequestUpdate::Completed {
                    result: Ok(api::EditorValue::Unit),
                },
            },
            Some(&source),
            &Index::default(),
            &[],
            4,
        );
        navigation.source_changed(next.as_ref());
        assert!(!navigation.controls_viewport(Some(&target)));
        assert!(!navigation.controls_viewport(next.as_ref()));
    }
}

/// A terminal failure releases the hold, while unrelated and duplicate Unit receipts cannot change ownership.
#[test]
fn reveal_priority_uses_only_owned_receipts_and_releases_on_failure() {
    let target = document("target", "next.md", 8);
    let source = Source {
        version: target.clone(),
        text: "# 目标\n".into(),
    };
    let (mut navigation, handle) = accepted(&target, Phase::Revealing);
    let unrelated = api::Notification::Request {
        handle: api::ResourceHandle {
            resource: 18,
            ..handle.clone()
        },
        update: api::RequestUpdate::Completed {
            result: Ok(api::EditorValue::Unit),
        },
    };
    assert!(!navigation.request(&unrelated, Some(&source), &Index::default(), &[], 4));
    assert!(navigation.controls_viewport(Some(&target)));
    assert!(navigation.request(
        &api::Notification::Request {
            handle,
            update: api::RequestUpdate::Cancelled {
                reason: api::ErrorCode::TimedOut,
                effect: api::CancellationEffect::NotExecuted,
            },
        },
        Some(&source),
        &Index::default(),
        &[],
        4,
    ));
    assert!(!navigation.controls_viewport(Some(&target)));
    assert!(navigation.pending.is_none());
    assert!(!navigation.request(&unrelated, Some(&source), &Index::default(), &[], 4));
}
