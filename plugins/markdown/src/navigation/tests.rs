//! Pure navigation regressions observe parser-derived targets and correlation without invoking native side effects.

use super::{
    Index, Navigation, Pending, Phase, Reason,
    opening::{Arrival, Opening},
    targets::{Destination, destination},
};
use crate::{Source, State, preview::blocks};
use plugin_protocol::{api, ui};

/// Expected identities use source byte offsets, independently of the slug implementation.
fn heading_id(source: &str, heading: &str) -> String {
    format!("b-{}-heading", source.find(heading).unwrap())
}

/// Unicode text, punctuation, whitespace and naturally suffixed headings share one collision-free namespace.
#[test]
fn parsed_heading_slugs_are_unicode_lowercase_and_collision_free() {
    let source = "# 标题\n\n# 标题\n\n# 标题-1\n\n# 标题\n\n# HELLO_ World！\n\n# !!!\n";
    let index = Index::parse(source);
    let nodes = blocks(source, "zh-CN").unwrap();
    let starts: Vec<_> = source.match_indices("# ").map(|(start, _)| start).collect();
    for (slug, start) in [
        "标题",
        "标题-1",
        "标题-1-1",
        "标题-2",
        "hello_-world",
        "section",
    ]
    .into_iter()
    .zip(starts)
    {
        assert_eq!(
            index.heading(&nodes, slug),
            Some(format!("b-{start}-heading")),
            "{slug}"
        );
    }
    assert!(
        index.heading(&nodes, "HELLO_-WORLD").is_none(),
        "fragment IDs match exactly"
    );
}

/// A pre-existing -1 heading is skipped rather than silently aliasing the first repeated title.
#[test]
fn natural_suffix_before_duplicate_title_is_not_reused() {
    let source = "# item-1\n\n# item\n\n# item\n\n# item-2\n";
    let index = Index::parse(source);
    let nodes = blocks(source, "en-US").unwrap();
    let starts: Vec<_> = source.match_indices("# ").map(|(start, _)| start).collect();
    for (slug, start) in ["item-1", "item", "item-2", "item-2-1"]
        .into_iter()
        .zip(starts)
    {
        assert_eq!(
            index.heading(&nodes, slug),
            Some(format!("b-{start}-heading"))
        );
    }
}

/// Image and quoted headings reveal real existing native nodes; fenced/raw HTML headings have no slug.
#[test]
fn heading_targets_follow_images_quotes_setext_and_literal_html() {
    let source = "# ![图像](img.png)\n\n> ## 内嵌 标题\n\nSetext\n====\n\n<h1>伪标题</h1>\n\n```\n# 假标题\n```\n";
    let index = Index::parse(source);
    let nodes = blocks(source, "zh-CN").unwrap();
    assert_eq!(index.heading(&nodes, "图像"), Some("b-2-image".into()));
    assert_eq!(
        index.heading(&nodes, "内嵌-标题"),
        Some(heading_id(source, "## 内嵌"))
    );
    assert_eq!(
        index.heading(&nodes, "setext"),
        Some(heading_id(source, "Setext"))
    );
    assert!(index.heading(&nodes, "伪标题").is_none());
    assert!(index.heading(&nodes, "假标题").is_none());
}

/// Link bindings come from actual Markdown events in the clicked native node, never a matching URI elsewhere.
#[test]
fn raw_html_code_and_unrelated_text_cannot_supply_link_events() {
    let source =
        "[真的](next.md)\n\n普通文字\n\n<a href=\"next.md\">假</a>\n\n```\n[代码](next.md)\n```\n";
    let index = Index::parse(source);
    let nodes = blocks(source, "zh-CN").unwrap();
    assert_eq!(
        index.resolve(&nodes, "b-0-paragraph", "next.md"),
        Some("next.md")
    );
    assert!(index.resolve(&nodes, "b-0-paragraph", "other.md").is_none());
    for (text, kind) in [
        ("普通文字", "paragraph"),
        ("<a href", "raw-html"),
        ("```", "code"),
    ] {
        let id = format!("b-{}-{kind}", source.find(text).unwrap());
        assert!(index.resolve(&nodes, &id, "next.md").is_none(), "{id}");
    }
    assert!(
        index
            .resolve(&nodes, "b-999-paragraph", "next.md")
            .is_none()
    );
}

/// Observe the actual safe preview HTML instead of computing an expected href with the production matcher.
fn emitted_href(source: &str) -> String {
    let markup = blocks(source, "zh-CN")
        .unwrap()
        .into_iter()
        .find_map(|node| match node.kind {
            ui::Kind::RichText { html } => Some(html),
            _ => None,
        })
        .expect("the isolated Markdown link renders as one rich paragraph");
    markup
        .split_once("href=\"")
        .and_then(|(_, rest)| rest.split_once('"').map(|(href, _)| href.to_owned()))
        .expect("the actual public HTML writer supplied a quoted link attribute")
}

/// Native callbacks decode HTML attributes once but retain the writer's percent escapes for Chinese and spaces.
#[test]
fn rendered_hrefs_match_exactly_and_recover_original_parsed_uris() {
    for (source, attribute, native, parsed) in [
        (
            "[go](#标题-1)",
            "#%E6%A0%87%E9%A2%98-1",
            "#%E6%A0%87%E9%A2%98-1",
            "#标题-1",
        ),
        (
            "[go](<目录/a b.md>)",
            "%E7%9B%AE%E5%BD%95/a%20b.md",
            "%E7%9B%AE%E5%BD%95/a%20b.md",
            "目录/a b.md",
        ),
        ("[go](<a&b.md>)", "a&amp;b.md", "a&b.md", "a&b.md"),
        ("[go](<a'b.md>)", "a&#x27;b.md", "a'b.md", "a'b.md"),
        (
            "[go](<a&amp;amp;b.md>)",
            "a&amp;amp;b.md",
            "a&amp;b.md",
            "a&amp;b.md",
        ),
        (
            "[go](<a&amp;#x27;b.md>)",
            "a&amp;#x27;b.md",
            "a&#x27;b.md",
            "a&#x27;b.md",
        ),
    ] {
        assert_eq!(emitted_href(source), attribute, "{source}");
        let index = Index::parse(source);
        let nodes = blocks(source, "zh-CN").unwrap();
        assert_eq!(
            index.resolve(&nodes, "b-0-paragraph", native),
            Some(parsed),
            "{source}"
        );
        if native != parsed {
            assert!(
                index.resolve(&nodes, "b-0-paragraph", parsed).is_none(),
                "the event must equal the actual rendered href"
            );
        }
    }
}

/// Matching cannot collapse encoded delimiters or double percent escapes into a different destination.
#[test]
fn rendered_href_matching_preserves_reserved_delimiters_and_double_encoding() {
    let source = "[go](<%252e%252e/a.md#标题>)";
    let native = "%252e%252e/a.md#%E6%A0%87%E9%A2%98";
    assert_eq!(emitted_href(source), native);
    let index = Index::parse(source);
    let nodes = blocks(source, "zh-CN").unwrap();
    let parsed = index.resolve(&nodes, "b-0-paragraph", native).unwrap();
    assert_eq!(
        destination(parsed),
        Ok(Destination::Relative {
            path: "%252e%252e/a.md".into(),
            fragment: Some("标题".into())
        })
    );
    assert!(
        index
            .resolve(&nodes, "b-0-paragraph", "%2e%2e/a.md#%E6%A0%87%E9%A2%98")
            .is_none()
    );
    assert!(
        index
            .resolve(&nodes, "b-0-paragraph", "../a.md#标题")
            .is_none()
    );
    for (source, native, altered) in [
        ("[go](<a%23b.md>)", "a%23b.md", "a#b.md"),
        ("[go](<a%3Fb.md>)", "a%3Fb.md", "a?b.md"),
    ] {
        assert_eq!(emitted_href(source), native);
        let index = Index::parse(source);
        let nodes = blocks(source, "zh-CN").unwrap();
        assert_eq!(index.resolve(&nodes, "b-0-paragraph", native), Some(native));
        assert!(index.resolve(&nodes, "b-0-paragraph", altered).is_none());
    }
}

/// The public writer and parsed email semantics agree on mailto; policy still rejects unsupported navigation.
#[test]
fn email_autolinks_match_writer_prefix_without_becoming_external_browser_requests() {
    let source = "<user@example.com>";
    assert_eq!(emitted_href(source), "mailto:user@example.com");
    let index = Index::parse(source);
    let nodes = blocks(source, "en-US").unwrap();
    let parsed = index
        .resolve(&nodes, "b-0-paragraph", "mailto:user@example.com")
        .unwrap();
    assert_eq!(parsed, "mailto:user@example.com");
    assert_eq!(destination(parsed), Err(Reason::Unsupported));
}

/// Email autolinks retain implicit mailto semantics even with Markdown suffixes; explicit @ filenames remain valid.
#[test]
fn email_autolinks_with_markdown_suffixes_cannot_become_relative_document_links() {
    for extension in ["md", "markdown"] {
        let address = format!("user@example.{extension}");
        let source = format!("<{address}>");
        let href = format!("mailto:{address}");
        assert_eq!(emitted_href(&source), href);
        let index = Index::parse(&source);
        let nodes = blocks(&source, "en-US").unwrap();
        let parsed = index.resolve(&nodes, "b-0-paragraph", &href).unwrap();
        assert_eq!(destination(parsed), Err(Reason::Unsupported));
        assert_eq!(parsed, href);

        // The same characters in an explicit Markdown destination are a filename, not an email autolink.
        let source = format!("[user]({address})");
        assert_eq!(emitted_href(&source), address);
        let index = Index::parse(&source);
        let nodes = blocks(&source, "en-US").unwrap();
        let parsed = index.resolve(&nodes, "b-0-paragraph", &address).unwrap();
        assert_eq!(parsed, address);
        assert_eq!(
            destination(parsed),
            Ok(Destination::Relative {
                path: address,
                fragment: None,
            })
        );
    }
}

/// Relative paths retain their original encoding while Chinese fragments and literal percent signs decode once.
#[test]
fn encoded_paths_and_unicode_fragments_are_decoded_only_once() {
    let path = "%E4%B8%AD%E6%96%87.%6d%64";
    assert_eq!(
        destination(&format!("{path}#%E6%A0%87%E9%A2%98")),
        Ok(Destination::Relative {
            path: path.into(),
            fragment: Some("标题".into())
        })
    );
    assert_eq!(
        destination("dir%252e/file.MARKDOWN#%252F"),
        Ok(Destination::Relative {
            path: "dir%252e/file.MARKDOWN".into(),
            fragment: Some("%2F".into())
        })
    );
    assert_eq!(
        destination("#%E6%A0%87%E9%A2%98-1"),
        Ok(Destination::Anchor("标题-1".into()))
    );
    assert_eq!(
        destination("HTTPS://example.com/a?q=1#title"),
        Ok(Destination::External(
            "HTTPS://example.com/a?q=1#title".into()
        ))
    );
}

/// Unsupported protocols/files/queries and malformed encodings produce a reason instead of a native effect.
#[test]
fn unsupported_paths_and_invalid_fragments_do_not_become_navigation_targets() {
    for uri in [
        "image.png",
        "a.md?q=1",
        "/a.md",
        "%2Fa.md",
        "C%3Aa.md",
        "//example.com/a.md",
        "javascript:alert(1)",
        "mailto:a@b",
        "file:///a.md",
    ] {
        assert_eq!(destination(uri), Err(Reason::Unsupported), "{uri}");
    }
    for uri in ["bad%ff.md", "bad%GG.md", "#%E6%", "#%ff", "https://"] {
        assert_eq!(destination(uri), Err(Reason::InvalidUri), "{uri}");
    }
    assert_eq!(destination("#"), Err(Reason::InvalidAnchor));
    assert_eq!(destination("next.md#"), Err(Reason::InvalidAnchor));
}

/// Small identity fixtures model actual Opened/Preview receipts, not target identities inferred from paths.
fn version(id: &str, path: &str, revision: u64) -> api::DocumentVersion {
    api::DocumentVersion {
        id: id.into(),
        path: path.into(),
        revision,
    }
}

/// Both event orders converge only when the actual target identity and revision agree.
#[test]
fn relative_open_accepts_both_preview_and_receipt_arrival_orders() {
    let origin = version("origin", "notes.md", 3);
    let target = version("opened", "next.md", 8);
    let mut receipt_first = Opening::default();
    assert_eq!(
        receipt_first.complete(&origin, &target, Some(&origin)),
        Ok(Arrival::Waiting)
    );
    assert!(receipt_first.observe(&origin, Some(&target)));
    assert_eq!(
        receipt_first.complete(&origin, &target, Some(&target)),
        Ok(Arrival::Ready)
    );
    let mut preview_first = Opening::default();
    assert!(preview_first.observe(&origin, Some(&target)));
    assert_eq!(
        preview_first.complete(&origin, &target, Some(&target)),
        Ok(Arrival::Ready)
    );
}

/// Closure, edits, reopened identities or a third document cannot authorize a target's heading follow-up.
#[test]
fn relative_open_rejects_third_documents_reopens_and_changed_revisions() {
    let origin = version("origin", "notes.md", 3);
    let target = version("opened", "next.md", 8);
    let third = version("third", "other.md", 1);
    let mut opening = Opening::default();
    assert!(opening.observe(&origin, Some(&third)));
    assert_eq!(
        opening.complete(&origin, &target, Some(&third)),
        Err(Reason::SourceChanged)
    );
    assert!(
        !opening.observe(&origin, Some(&target)),
        "a second source transition cancels ownership"
    );
    let mut opening = Opening::default();
    assert!(opening.observe(&origin, Some(&target)));
    assert_eq!(
        opening.complete(&origin, &target, Some(&version("opened", "next.md", 9))),
        Err(Reason::SourceChanged)
    );
    assert_eq!(
        opening.complete(&origin, &target, Some(&version("reopened", "next.md", 8))),
        Err(Reason::SourceChanged)
    );
    assert_eq!(
        opening.complete(&origin, &target, None),
        Err(Reason::SourceChanged)
    );
    assert!(!Opening::default().observe(&origin, Some(&version("origin", "notes.md", 4))));
    assert!(!Opening::default().observe(&origin, None));
}

/// A fake accepted handle exercises public SDK correlation without starting a host operation in pure tests.
fn owner(document: api::DocumentVersion, resource: u64) -> (Navigation, api::ResourceHandle) {
    let handle = api::ResourceHandle {
        instance: "test".into(),
        scope: "workspace".into(),
        resource,
    };
    let owner = Navigation {
        pending: Some(Pending {
            task: api::guest::EditorTask::from_accepted(handle.clone()),
            document,
            phase: Phase::External,
        }),
        ..Default::default()
    };
    (owner, handle)
}

/// Only the owning task can finish; duplicate terminal receipts remain inert and never change text.
#[test]
fn request_completion_ignores_unrelated_and_post_terminal_receipts() {
    let source = Source {
        version: version("origin", "notes.md", 3),
        text: "未保存正文".into(),
    };
    let (mut navigation, handle) = owner(source.version.clone(), 1);
    let unrelated = api::Notification::Request {
        handle: api::ResourceHandle {
            resource: 2,
            ..handle.clone()
        },
        update: api::RequestUpdate::Completed {
            result: Ok(api::EditorValue::Unit),
        },
    };
    assert!(!navigation.request(&unrelated, Some(&source), &Index::default(), &[], 5));
    assert!(navigation.pending.is_some());
    let completed = api::Notification::Request {
        handle,
        update: api::RequestUpdate::Completed {
            result: Ok(api::EditorValue::Unit),
        },
    };
    assert!(!navigation.request(&completed, Some(&source), &Index::default(), &[], 5));
    assert!(navigation.pending.is_none());
    assert!(!navigation.request(&completed, Some(&source), &Index::default(), &[], 5));
    assert_eq!(source.text, "未保存正文");
}

/// Terminal timeouts release ownership and produce bilingual feedback only for their bound source document.
#[test]
fn request_timeout_feedback_is_localized_and_source_bound() {
    let source = Source {
        version: version("origin", "notes.md", 3),
        text: String::new(),
    };
    let (mut navigation, handle) = owner(source.version.clone(), 3);
    let timeout = api::Notification::Request {
        handle,
        update: api::RequestUpdate::Cancelled {
            reason: api::ErrorCode::TimedOut,
            effect: api::CancellationEffect::NotExecuted,
        },
    };
    assert!(navigation.request(&timeout, Some(&source), &Index::default(), &[], 5));
    assert!(navigation.pending.is_none());
    assert!(
        navigation
            .message(Some(&source), false)
            .unwrap()
            .contains("超时")
    );
    assert!(
        navigation
            .message(Some(&source), true)
            .unwrap()
            .contains("timed out")
    );
    let other = Source {
        version: version("other", "other.md", 1),
        text: String::new(),
    };
    assert!(navigation.message(Some(&other), true).is_none());
    assert!(navigation.message(None, true).is_none());
}

/// A real parsed missing-anchor action reports outside the scrolling preview as well as in the source toolbar.
#[test]
fn navigation_failure_remains_visible_without_the_source_toolbar_and_clears_on_source_change() {
    // Search public nodes only; the toolbar and preview use distinct feedback identities.
    fn text<'a>(node: &'a ui::Node, id: &str) -> Option<&'a str> {
        match &node.kind {
            ui::Kind::Text { text } if node.id == id => Some(text),
            ui::Kind::Column { children } | ui::Kind::Row { children } => {
                children.iter().find_map(|node| text(node, id))
            }
            ui::Kind::Scroll { content } => text(content, id),
            _ => None,
        }
    }

    for (locale, expected) in [
        ("zh-CN", "链接的标题锚点不存在或无效。"),
        ("en-US", "The linked heading anchor is missing or invalid."),
    ] {
        let mut state = State::default();
        state.environment.locale = locale.into();
        state.event(
            Some("preview"),
            api::Notification::Preview {
                document: Some(version("origin", "notes.md", 3)),
                text: "[失效](#missing)\n\n# 存在\n".into(),
            },
        );
        // This invokes the actual parsed-link policy; no test injects an Issue or starts a native host effect.
        state.event(
            Some("preview"),
            api::Notification::Ui(ui::UiEvent {
                revision: state.revision,
                node: "b-0-paragraph".into(),
                action: ui::Action::Link {
                    uri: "#missing".into(),
                },
            }),
        );
        let view = state.view();
        let ui::Kind::Column { children } = &view.document.root.kind else {
            panic!("navigation feedback must stay outside the scrolling document body");
        };
        let feedback = children
            .iter()
            .find(|node| node.id == "preview-navigation-feedback")
            .expect("preview-only mode must retain a visible navigation failure");
        assert!(matches!(&feedback.kind, ui::Kind::Text { text } if text == expected));
        assert!(feedback.links.is_empty());
        assert!(feedback.source_range.is_none());
        assert!(children.iter().any(
            |node| node.id == "preview-scroll" && matches!(node.kind, ui::Kind::Scroll { .. })
        ));
        assert_eq!(
            text(
                view.document.editor_toolbar.as_ref().unwrap(),
                "format-error"
            ),
            Some(expected)
        );
        assert!(view.document.validate().is_ok());

        // Editing the same source and then closing it both retire its old failure rather than leaking into another view.
        for document in [Some(version("origin", "notes.md", 4)), None] {
            state.event(
                Some("preview"),
                api::Notification::Preview {
                    document,
                    text: "更新后的正文".into(),
                },
            );
            let view = state.view();
            assert!(text(&view.document.root, "preview-navigation-feedback").is_none());
            assert!(
                view.document
                    .editor_toolbar
                    .as_ref()
                    .is_none_or(|toolbar| text(toolbar, "format-error").is_none())
            );
        }
    }
}
