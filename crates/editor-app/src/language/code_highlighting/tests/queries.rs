//! Real WASM queries exercise static injection authority, allocation budgets and literal byte mapping.
use super::*;

/// Copy only the real WASM fixture and replace its query, keeping package-path validation active.
fn query_package(fixture: &Fixture, query: &str) -> (tempfile::TempDir, Highlighter) {
    let directory = tempfile::tempdir().unwrap();
    let mut declaration = fixture.declaration.clone();
    fs::copy(
        fixture.root.join(&declaration.grammar),
        directory.path().join("grammar.wasm"),
    )
    .unwrap();
    fs::write(directory.path().join("highlights.scm"), query).unwrap();
    declaration.grammar = "grammar.wasm".into();
    declaration.highlights = "highlights.scm".into();
    (directory, declaration)
}

/// Block and inline WASM providers compose independently, and withdrawal keeps block text styles.
#[test]
fn readonly_code_highlighting_static_injections_follow_independent_selected_wasm() {
    let fixture = Fixture::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/markdown")
        .canonicalize()
        .unwrap();
    let manifest =
        PluginManifest::parse(&fs::read_to_string(root.join("plugin.toml")).unwrap()).unwrap();
    let block = (
        "blocks".into(),
        root.clone(),
        manifest.language_definitions.clone(),
        vec![manifest.highlighters[0].clone()],
    );
    let inline = (
        "inline".into(),
        root.clone(),
        Vec::new(),
        vec![manifest.highlighters[1].clone()],
    );
    providers::refresh(
        fixture.store.path(),
        vec![block.clone(), inline.clone()],
        Vec::new(),
    );
    let original = selected("markdown").unwrap();
    let source = "# 标题\r\n\r\n正文 **粗体** 与 *斜体*。\r\n";
    let tokens = highlight(&original, source, &AtomicBool::new(false), deadline());
    assert!(
        tokens.iter().any(|token| token.capture == "emphasis.strong"
            && source[token.range.clone()].contains("粗体")),
        "inline WASM must contribute strong emphasis: {tokens:?}"
    );
    assert!(
        tokens.iter().any(
            |token| token.capture == "emphasis" && source[token.range.clone()].contains("斜体")
        )
    );
    assert!(
        tokens
            .iter()
            .any(|token| token.capture == "title" && source[token.range.clone()].contains("标题"))
    );
    assert!(
        tokens
            .windows(2)
            .all(|pair| pair[0].range.end <= pair[1].range.start)
    );

    // A competing injected provider reuses the grammar but declares different observable captures.
    let alternative_directory = tempfile::tempdir().unwrap();
    let mut declaration = manifest.highlighters[1].clone();
    fs::copy(
        root.join(&declaration.grammar),
        alternative_directory.path().join("inline.wasm"),
    )
    .unwrap();
    fs::write(
        alternative_directory.path().join("highlights.scm"),
        "(strong_emphasis) @constant",
    )
    .unwrap();
    declaration.grammar = "inline.wasm".into();
    declaration.highlights = "highlights.scm".into();
    let alternative = (
        "alternative".into(),
        alternative_directory.path().canonicalize().unwrap(),
        Vec::new(),
        vec![declaration],
    );
    providers::refresh(
        fixture.store.path(),
        vec![block.clone(), inline.clone(), alternative],
        Vec::new(),
    );
    assert_eq!(
        selected("markdown_inline").unwrap().provider.owner,
        "inline"
    );
    providers::choose(
        Scope::User,
        "highlight:markdown_inline",
        Some("alternative/inline"),
    )
    .unwrap();
    assert!(!is_current(&original));
    let tokens = highlight(
        &selected("markdown").unwrap(),
        source,
        &AtomicBool::new(false),
        deadline(),
    );
    assert!(
        tokens.iter().any(
            |token| token.capture == "constant" && source[token.range.clone()].contains("粗体")
        )
    );
    assert!(
        !tokens
            .iter()
            .any(|token| token.capture == "emphasis.strong")
    );

    providers::refresh(fixture.store.path(), vec![block.clone()], Vec::new());
    assert!(selected("markdown_inline").is_none());
    let tokens = highlight(
        &selected("markdown").unwrap(),
        source,
        &AtomicBool::new(false),
        deadline(),
    );
    assert!(tokens.iter().any(|token| token.capture == "title"));
    assert!(
        !tokens
            .iter()
            .any(|token| token.capture == "constant" || token.capture == "emphasis.strong")
    );
    providers::refresh(fixture.store.path(), vec![block, inline], Vec::new());
    assert!(selected("markdown").unwrap().epoch > original.epoch);
    assert!(highlight(&original, source, &AtomicBool::new(false), deadline()).is_empty());
    assert!(
        highlight(
            &selected("markdown").unwrap(),
            source,
            &AtomicBool::new(false),
            deadline()
        )
        .iter()
        .any(|token| token.capture == "emphasis.strong")
    );
}

/// An otherwise valid real query cannot cause large capture-name copies for even one token.
#[test]
fn readonly_code_highlighting_rejects_oversized_wasm_capture_name() {
    let fixture = Fixture::new();
    let name = "n".repeat(257);
    let (directory, declaration) = query_package(&fixture, &format!("(integer) @{name}"));
    providers::refresh(
        fixture.store.path(),
        vec![(
            "primary".into(),
            directory.path().canonicalize().unwrap(),
            vec![fixture.definition.clone()],
            vec![declaration],
        )],
        Vec::new(),
    );
    assert!(
        highlight(
            &fixture.selection(),
            "x = 42",
            &AtomicBool::new(false),
            deadline()
        )
        .is_empty(),
        "capture names above 128 UTF-8 bytes must be rejected before copying"
    );
}

/// Individually small names still share a bounded cumulative raw capture byte budget.
#[test]
fn readonly_code_highlighting_bounds_cumulative_wasm_capture_name_bytes() {
    let fixture = Fixture::new();
    let name = "n".repeat(120);
    let (directory, declaration) = query_package(&fixture, &format!("(integer) @{name}"));
    providers::refresh(
        fixture.store.path(),
        vec![(
            "primary".into(),
            directory.path().canonicalize().unwrap(),
            vec![fixture.definition.clone()],
            vec![declaration],
        )],
        Vec::new(),
    );
    let source = "x = 1\n".repeat(600);
    assert!(source.len() < MAX_TEXT_BYTES);
    assert!(
        highlight(
            &fixture.selection(),
            &source,
            &AtomicBool::new(false),
            deadline()
        )
        .is_empty(),
        "600 copies of 120-byte names exceed the 64 KiB capture-name budget"
    );
}

/// Normalization may repeat a broad capture around many narrow ones, so output copies are bounded too.
#[test]
fn readonly_code_highlighting_bounds_normalized_wasm_capture_name_bytes() {
    let fixture = Fixture::new();
    let name = "n".repeat(120);
    let (directory, declaration) =
        query_package(&fixture, &format!("(document) @{name}\n(integer) @n"));
    providers::refresh(
        fixture.store.path(),
        vec![(
            "primary".into(),
            directory.path().canonicalize().unwrap(),
            vec![fixture.definition.clone()],
            vec![declaration],
        )],
        Vec::new(),
    );
    let source = "x = 1\n".repeat(600);
    assert!(
        highlight(
            &fixture.selection(),
            &source,
            &AtomicBool::new(false),
            deadline()
        )
        .is_empty(),
        "normalized capture copies must also fit the 64 KiB name budget"
    );
}

/// Match the native regression's literal and query exactly so source-byte captures can isolate paint.
#[test]
fn readonly_code_highlighting_integer_after_chinese_crlf_comment_has_exact_source_range() {
    let fixture = Fixture::new();
    let (directory, declaration) = query_package(&fixture, "(integer) @number");
    providers::refresh(
        fixture.store.path(),
        vec![(
            "primary".into(),
            directory.path().canonicalize().unwrap(),
            vec![fixture.definition.clone()],
            vec![declaration],
        )],
        Vec::new(),
    );
    let source = "# 中文示例\r\nanswer = 42\r\n";
    let start = source.find("42").unwrap();
    assert_eq!(
        highlight(
            &fixture.selection(),
            source,
            &AtomicBool::new(false),
            deadline()
        ),
        vec![Token {
            range: start..start + 2,
            capture: "number".into()
        }]
    );
}
