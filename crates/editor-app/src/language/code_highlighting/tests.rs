//! Real WASM, provider generations and normalized capture budgets preserve readonly code authority.
use super::*;
use plugin_runtime::plugin_protocol::settings::Scope;
use plugin_schema::{Highlighter, LanguageDefinition, PluginManifest};
use std::{fs, path::PathBuf, sync::MutexGuard, time::Duration};

// These tests exercise the real shared provider service; isolate its workspace preferences.
static PROVIDERS: Mutex<()> = Mutex::new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    store: tempfile::TempDir,
    workspace: tempfile::TempDir,
    root: PathBuf,
    declaration: Highlighter,
    definition: LanguageDefinition,
}

impl Fixture {
    /// Use the shipped WASM under a novel language identity: no built-in grammar can satisfy it.
    fn new() -> Self {
        let guard = PROVIDERS.lock().unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/toml")
            .canonicalize()
            .unwrap();
        let manifest =
            PluginManifest::parse(&fs::read_to_string(root.join("plugin.toml")).unwrap()).unwrap();
        let mut declaration = manifest.highlighters[0].clone();
        declaration.language = "dialect".into();
        let fixture = Self {
            _guard: guard,
            store: tempfile::tempdir().unwrap(),
            workspace: tempfile::tempdir().unwrap(),
            root,
            declaration,
            definition: LanguageDefinition {
                id: "dialect".into(),
                name: "Dialect Display".into(),
                extensions: vec!["dia".into()],
                filenames: Vec::new(),
            },
        };
        providers::configure(fixture.store.path(), fixture.workspace.path());
        fixture.refresh(true);
        fixture
    }

    fn entry(&self, owner: &str) -> (String, PathBuf, Vec<LanguageDefinition>, Vec<Highlighter>) {
        (
            owner.into(),
            self.root.clone(),
            vec![self.definition.clone()],
            vec![self.declaration.clone()],
        )
    }

    fn refresh(&self, enabled: bool) {
        providers::refresh(
            self.store.path(),
            if enabled {
                vec![self.entry("primary")]
            } else {
                Vec::new()
            },
            Vec::new(),
        );
    }

    fn selection(&self) -> Selection {
        selected("dialect").unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Teardown withdraws contributions and prepared resources before deleting preferences.
        self.refresh(false);
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}

/// Actual packaged bytes produce captures into unchanged Chinese/emoji and CRLF input.
#[test]
fn readonly_code_highlighting_uses_selected_novel_wasm_and_utf8_ranges() {
    let fixture = Fixture::new();
    let selection = fixture.selection();
    for alias in ["DIALECT", "Dialect Display", "dia", ".DIA"] {
        assert_eq!(selected(alias), Some(selection.clone()), "alias {alias}");
    }
    // The upstream registry has a TOML parser, but no enabled declaration authorizes this ID.
    assert!(selected("toml").is_none());
    assert!(selected("unknown").is_none());
    let source = "answer = 42\r\nlabel = \"中文🙂\"\r\n";
    let tokens = highlight(&selection, source, &AtomicBool::new(false), deadline());
    assert!(!tokens.is_empty());
    assert!(
        tokens
            .iter()
            .any(|token| token.capture == "number" && &source[token.range.clone()] == "42")
    );
    assert!(
        tokens
            .iter()
            .any(|token| token.capture == "string" && &source[token.range.clone()] == "\"中文🙂\"")
    );
    assert!(
        tokens
            .iter()
            .all(|token| token.range.start < token.range.end
                && token.range.end <= source.len()
                && source.is_char_boundary(token.range.start)
                && source.is_char_boundary(token.range.end))
    );
    assert!(
        tokens
            .windows(2)
            .all(|pair| pair[0].range.end <= pair[1].range.start)
    );
    assert_eq!(source, "answer = 42\r\nlabel = \"中文🙂\"\r\n");
}

/// A removal followed by an identical package must not authorize a result from its first epoch.
#[test]
fn readonly_code_highlighting_epochs_reject_remove_reenable_and_choice_aba() {
    let fixture = Fixture::new();
    let first = fixture.selection();
    fixture.refresh(true);
    assert_eq!(fixture.selection(), first, "equal refresh preserves work");
    fixture.refresh(false);
    assert!(selected("dialect").is_none());
    assert!(!is_current(&first));
    assert!(PREPARED.lock().unwrap().entries.is_empty());
    fixture.refresh(true);
    let restored = fixture.selection();
    assert_eq!(restored.provider, first.provider);
    assert!(restored.epoch > first.epoch);
    assert!(!is_current(&first));
    providers::refresh(
        fixture.store.path(),
        vec![fixture.entry("primary"), fixture.entry("secondary")],
        Vec::new(),
    );
    let retained = fixture.selection();
    assert_eq!(
        retained.provider.owner, "primary",
        "install order cannot replace valid adoption"
    );
    providers::choose(Scope::User, "highlight:dialect", Some("secondary/syntax")).unwrap();
    let second = fixture.selection();
    assert_eq!(second.provider.owner, "secondary");
    providers::choose(Scope::User, "highlight:dialect", Some("primary/syntax")).unwrap();
    let again = fixture.selection();
    assert_eq!(again.provider, retained.provider);
    assert!(again.epoch > second.epoch && second.epoch > retained.epoch);
    assert!(!is_current(&retained) && !is_current(&second));
    assert!(highlight(&retained, "x = 1", &AtomicBool::new(false), deadline()).is_empty());
}

/// Display-name ambiguity is plain; extension ambiguity follows explicit recognition choice.
#[test]
fn readonly_code_highlighting_aliases_preserve_independent_provider_preferences() {
    let fixture = Fixture::new();
    let mut other = fixture.entry("other");
    other.2[0].id = "other-dialect".into();
    other.3[0].language = "other-dialect".into();
    providers::refresh(
        fixture.store.path(),
        vec![fixture.entry("primary"), other],
        Vec::new(),
    );
    assert!(selected("Dialect Display").is_none());
    assert_eq!(selected("dia").unwrap().provider.owner, "primary");
    providers::choose(
        Scope::User,
        "recognition:ext:dia",
        Some("other/other-dialect"),
    )
    .unwrap();
    assert_eq!(selected(".dia").unwrap().provider.owner, "other");
    assert_eq!(selected("dialect").unwrap().provider.owner, "primary");
    providers::choose(Scope::User, "recognition:ext:dia", None).unwrap();
    assert!(selected("dia").is_none());
}

/// Budget or cancellation failure rejects the whole result instead of drawing partial styles.
#[test]
fn readonly_code_highlighting_is_plain_on_cancellation_deadline_and_limits() {
    let fixture = Fixture::new();
    let selection = fixture.selection();
    assert!(highlight(&selection, "x = 1", &AtomicBool::new(true), deadline()).is_empty());
    assert!(highlight(&selection, "x = 1", &AtomicBool::new(false), Instant::now()).is_empty());
    assert!(
        highlight(
            &selection,
            &" ".repeat(MAX_TEXT_BYTES + 1),
            &AtomicBool::new(false),
            deadline()
        )
        .is_empty()
    );
    // A short file can still produce too many captures; bytes alone are not a sufficient limit.
    let dense = "x = 1\n".repeat(1400);
    assert!(dense.len() < MAX_TEXT_BYTES);
    assert!(highlight(&selection, &dense, &AtomicBool::new(false), deadline()).is_empty());
}

/// A selected package with missing assets cannot borrow any process-wide native parser.
#[test]
fn readonly_code_highlighting_failed_provider_is_plain_and_context_rebuild_advances_epoch() {
    let fixture = Fixture::new();
    let original = fixture.selection();
    let mut missing = fixture.entry("primary");
    missing.3[0].grammar = "grammar/missing.wasm".into();
    providers::refresh(fixture.store.path(), vec![missing], Vec::new());
    assert!(!is_current(&original));
    let failed = fixture.selection();
    assert!(highlight(&failed, "x = 1", &AtomicBool::new(false), deadline()).is_empty());
    let other_workspace = tempfile::tempdir().unwrap();
    providers::configure(fixture.store.path(), other_workspace.path());
    assert!(epoch() > failed.epoch);
    assert!(!is_current(&failed));
    fixture.refresh(true);
    assert!(fixture.selection().epoch > failed.epoch);
}

/// More specific ranges override broad styles; query order breaks ties without splitting UTF-8.
#[test]
fn readonly_code_highlighting_normalizes_overlap_priority_and_adjacent_spans() {
    let capture = |range, name: &str, pattern| Capture {
        token: Token {
            range,
            capture: name.into(),
        },
        pattern,
    };
    let source = "甲🙂乙abc";
    let tokens = normalize(
        vec![
            capture(0..13, "outer", 0),
            capture(3..10, "middle", 1),
            capture(7..10, "short", 2),
            capture(7..10, "later", 3),
            capture(10..12, "cross", 4),
            capture(11..13, "tail", 5),
        ],
        &AtomicBool::new(false),
        deadline(),
    );
    let actual = tokens
        .iter()
        .map(|token| (&source[token.range.clone()], token.capture.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            ("甲", "outer"),
            ("🙂", "middle"),
            ("乙", "later"),
            ("a", "cross"),
            ("bc", "tail")
        ]
    );
    assert!(
        tokens
            .windows(2)
            .all(|pair| pair[0].range.end <= pair[1].range.start)
    );
    assert_eq!(
        normalize(
            vec![capture(0..2, "same", 0), capture(2..4, "same", 1)],
            &AtomicBool::new(false),
            deadline()
        ),
        vec![Token {
            range: 0..4,
            capture: "same".into()
        }]
    );
    // Capture sequence remains deterministic within one query pattern as well.
    assert_eq!(
        normalize(
            vec![capture(0..4, "first", 0), capture(0..4, "last", 0)],
            &AtomicBool::new(false),
            deadline()
        ),
        vec![Token {
            range: 0..4,
            capture: "last".into()
        }]
    );
}

/// A bounded raw query can still create too many disjoint spans; reject that entire result.
#[test]
fn readonly_code_highlighting_bounds_normalized_spans_and_interrupted_sweeps() {
    let capture = |range, name: &str| Capture {
        token: Token {
            range,
            capture: name.into(),
        },
        pattern: 0,
    };
    let mut captures = vec![capture(0..4097, "outer")];
    captures.extend((0..2048).map(|index| capture((index * 2 + 1)..(index * 2 + 2), "inner")));
    assert!(captures.len() < MAX_TOKENS);
    assert!(normalize(captures, &AtomicBool::new(false), deadline()).is_empty());
    assert!(
        normalize(
            vec![capture(0..1, "number")],
            &AtomicBool::new(true),
            deadline()
        )
        .is_empty()
    );
    assert!(
        normalize(
            vec![capture(0..1, "number")],
            &AtomicBool::new(false),
            Instant::now()
        )
        .is_empty()
    );
}

// Query composition and name-allocation regressions share the actual provider fixture above.
mod queries;
