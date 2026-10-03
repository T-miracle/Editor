//! Loads declarative language plugins into GPUI Kit's Tree-sitter registry.

use anyhow::{Context as _, ensure};
use gpui_kit::{
    SharedString,
    component::highlighter::{GrammarConfig, LanguageParserFactory, LanguageRegistry},
};
use plugin_schema::Highlighter;
#[cfg(test)]
use plugin_schema::PluginManifest;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tree_sitter::{Parser, WasmStore, wasmtime::Engine};

/// Remove a plugin parser immediately when its installed package is disabled or uninstalled.
pub fn mask_language(language_id: &str) {
    let registry = LanguageRegistry::singleton();
    registry.register(language_id, &GrammarConfig::plain(language_id.to_owned()));
    // GPUI Kit has no parser-factory removal API, so an inert factory masks the old WASM parser.
    registry.register_parser_factory(
        language_id,
        Arc::new(|| Err(anyhow::anyhow!("language plugin is disabled"))),
    );
}

/// Test fixtures validate packaged declarations through the same dynamic loader used by the UI.
#[cfg(test)]
pub fn register_plugin(plugin_root: &Path) -> anyhow::Result<()> {
    let root = plugin_root.canonicalize()?;
    let manifest = PluginManifest::parse(&fs::read_to_string(root.join("plugin.toml"))?)?;
    for definition in &manifest.highlighters {
        let (grammar, query) = load_plugin_language(&root, definition)?;
        publish_dynamic(&definition.language, grammar, query);
    }
    Ok(())
}

/// Load a grammar and query from a plugin directory before registering them.
fn load_plugin_language(
    plugin_root: &Path,
    contribution: &Highlighter,
) -> anyhow::Result<(Arc<LoadedGrammar>, String)> {
    let grammar_path = plugin_asset(plugin_root, &contribution.grammar)?;
    let highlights_path = plugin_asset(plugin_root, &contribution.highlights)?;
    let grammar_bytes = fs::read(&grammar_path)
        .with_context(|| format!("read grammar {}", grammar_path.display()))?;
    let query = fs::read_to_string(&highlights_path)
        .with_context(|| format!("read highlights {}", highlights_path.display()))?;
    let injections = contribution
        .injections
        .as_ref()
        .map(|path| {
            let path = plugin_asset(plugin_root, path)?;
            fs::read_to_string(path).context("read grammar injections")
        })
        .transpose()?
        .unwrap_or_default();
    load_language(contribution, grammar_bytes, query, injections)
}

/// Owns the WASM store for as long as parsers can use its language handle.
pub(crate) struct LoadedGrammar {
    engine: Engine,
    language_id: String,
    bytes: Arc<[u8]>,
    /// Queries and allowed identities stay with the same generation-checked immutable grammar.
    injections: String,
    injection_languages: Vec<String>,
}

/// Validate bytes on a background worker without mutating the process-wide parser registry.
pub(crate) fn prepare_dynamic(
    provider: &super::providers::GrammarProvider,
) -> anyhow::Result<(Arc<LoadedGrammar>, String)> {
    load_plugin_language(&provider.root, &provider.declaration)
}

/// Only the UI owner may publish a generation-checked successful load.
pub(crate) fn publish_dynamic(language: &str, grammar: Arc<LoadedGrammar>, query: String) {
    let registry = LanguageRegistry::singleton();
    // A declared injection must never borrow an upstream native parser when its plugin is absent.
    let selected = super::providers::grammars();
    for dependency in &grammar.injection_languages {
        if !selected
            .iter()
            .any(|provider| &provider.declaration.language == dependency)
        {
            mask_language(dependency);
        }
    }
    let mut config = GrammarConfig::plain(language.to_owned());
    config.highlights = SharedString::from(query);
    config.injections = SharedString::from(grammar.injections.clone());
    config.injection_languages = grammar
        .injection_languages
        .iter()
        .cloned()
        .map(Into::into)
        .collect();
    registry.register(language, &config);
    registry.register_parser_factory(language, parser_factory(grammar));
}

/// Read and validate a grammar module and its highlight query from one plugin root.
fn load_language(
    contribution: &Highlighter,
    grammar_bytes: Vec<u8>,
    query: String,
    injections: String,
) -> anyhow::Result<(Arc<LoadedGrammar>, String)> {
    let expected_abi = contribution.tree_sitter_abi;

    let engine = Engine::default();
    let mut store = WasmStore::new(&engine).context("create Tree-sitter WASM store")?;
    let language = store
        .load_language(&contribution.grammar_name, &grammar_bytes)
        .with_context(|| format!("load WASM grammar for {}", contribution.grammar_name))?;
    ensure!(
        language.abi_version() == expected_abi as usize,
        "grammar ABI mismatch for {}: manifest declares {}, module exports {}",
        contribution.grammar_name,
        expected_abi,
        language.abi_version()
    );
    ensure!(
        (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION)
            .contains(&language.abi_version()),
        "grammar ABI {} for {} is unsupported by this Tree-sitter runtime",
        language.abi_version(),
        contribution.grammar_name
    );
    tree_sitter::Query::new(&language, &query)
        .with_context(|| format!("compile highlight query for {}", contribution.grammar_name))?;
    if !injections.is_empty() {
        let injection_query = tree_sitter::Query::new(&language, &injections)
            .context("compile grammar injections")?;
        // The upstream registry does not enforce its advertised injection allowlist. Restrict
        // targets here before publication so queries cannot borrow a built-in native parser.
        ensure!(
            !injection_query
                .capture_names()
                .contains(&"injection.language"),
            "dynamic injection language captures are unsupported; declare a static target"
        );
        for pattern in 0..injection_query.pattern_count() {
            let mut targets = injection_query
                .property_settings(pattern)
                .iter()
                .filter(|property| property.key.as_ref() == "injection.language");
            let target = targets
                .next()
                .and_then(|property| property.value.as_deref());
            ensure!(
                targets.next().is_none()
                    && target.is_some_and(|target| contribution
                        .injection_languages
                        .iter()
                        .any(|id| id == target)),
                "injection target must be explicitly declared in injection_languages"
            );
        }
    }

    let grammar = Arc::new(LoadedGrammar {
        engine,
        language_id: contribution.grammar_name.clone(),
        bytes: grammar_bytes.into(),
        injections,
        injection_languages: contribution.injection_languages.clone(),
    });
    let (mut parser, language) = create_parser(&grammar)?;
    parser
        .set_language(&language)
        .context("set dynamically loaded Tree-sitter grammar")?;
    // Creating a tree validates the parser bridge; some legitimate grammars require nonempty input.
    // Source syntax errors belong to document diagnostics, not package admission.
    let _tree = parser
        .parse("", None)
        .context("parse validation sample with WASM grammar")?;
    Ok((grammar, query))
}

/// Keep plugin assets inside the canonical plugin directory, including symlink targets.
fn plugin_asset(plugin_root: &Path, relative_path: &Path) -> anyhow::Result<PathBuf> {
    let path = plugin_root
        .join(relative_path)
        .canonicalize()
        .with_context(|| format!("resolve plugin asset {}", relative_path.display()))?;
    ensure!(
        path.starts_with(plugin_root),
        "plugin asset escapes its directory: {}",
        relative_path.display()
    );
    ensure!(
        path.is_file(),
        "plugin asset is not a file: {}",
        path.display()
    );
    Ok(path)
}

/// Build fresh parser state while sharing the compiled language module safely.
fn parser_factory(grammar: Arc<LoadedGrammar>) -> LanguageParserFactory {
    Arc::new(move || {
        let grammar = grammar.clone();
        // WASM store creation needs a deeper stack than the editor's UI call chain leaves free.
        std::thread::Builder::new()
            .name("tree-sitter-wasm-init".to_owned())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || create_parser(&grammar))
            .context("spawn Tree-sitter WASM parser initialization")?
            .join()
            .map_err(|_| anyhow::anyhow!("Tree-sitter WASM parser initialization panicked"))?
    })
}

/// Create a parser whose owned WASM store has loaded the grammar for its module.
fn create_parser(grammar: &LoadedGrammar) -> anyhow::Result<(Parser, tree_sitter::Language)> {
    // A parser owns its WASM store, so each editor receives a fresh store and language.
    let mut store = WasmStore::new(&grammar.engine)?;
    let language = store.load_language(&grammar.language_id, &grammar.bytes)?;
    let mut parser = Parser::new();
    parser.set_wasm_store(store)?;
    Ok((parser, language))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Source plugin assets still validate independently before packaging.
    #[test]
    fn source_plugins_load_independently() {
        for directory in ["rust", "toml", "html", "javascript"] {
            let name = directory;
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../plugins")
                .join(directory);
            register_plugin(&root)
                .unwrap_or_else(|error| panic!("{name} failed to load: {error:#}"));
        }
    }

    /// A missing grammar stays inert and reaches callers as a load error.
    #[test]
    fn missing_plugin_asset_reports_failure() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("plugin.toml"),
            r#"[plugin]
id = "missing-test"
name = "Missing test"
version = "0.1.0"
host_version = ">=0.1.0"

[[highlighters]]
id = "syntax"
language = "missing-test"
grammar_name = "missing-test"
grammar = "grammar/missing.wasm"
highlights = "queries/missing.scm"
tree_sitter_abi = 15
"#,
        )
        .unwrap();
        let error = register_plugin(directory.path()).unwrap_err();
        assert!(format!("{error:#}").contains("missing.wasm"));
    }

    /// The shipped HTML WASM and query must parse real markup and capture its tokens.
    #[test]
    fn bundled_html_plugin_parses_and_highlights_markup() {
        use tree_sitter::StreamingIterator as _;

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/html");
        let manifest =
            PluginManifest::parse(&fs::read_to_string(root.join("plugin.toml")).unwrap()).unwrap();
        let (grammar, query) =
            load_plugin_language(&root.canonicalize().unwrap(), &manifest.highlighters[0]).unwrap();
        let (mut parser, language) = create_parser(&grammar).unwrap();
        parser.set_language(&language).unwrap();
        let source = r#"<!DOCTYPE html>
<!-- 中文 comment -->
<html><head><style>body { color: red; }</style></head>
<body><div class="card" data-id=demo hidden>中文😀 &amp;<br><img src='x.png'/></div>
<script>if (a < b) { console.log("<tag>"); }</script></body></html>"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let query = tree_sitter::Query::new(&language, &query).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        let mut captures = Vec::new();
        while let Some(found) = matches.next() {
            for capture in found.captures {
                captures.push((
                    query.capture_names()[capture.index as usize],
                    &source[capture.node.byte_range()],
                ));
            }
        }
        for expected in [
            ("constant", "<!DOCTYPE html>"),
            ("comment", "<!-- 中文 comment -->"),
            ("tag", "div"),
            ("attribute", "class"),
            ("string", "\"card\""),
            ("string", "demo"),
            ("string.special", "&amp;"),
            ("operator", "="),
            ("punctuation.bracket", "/>"),
        ] {
            assert!(
                captures.contains(&expected),
                "missing capture: {expected:?}"
            );
        }
        // Optional closing tags and void elements are legal HTML, including fragments.
        for source in ["<ul><li>one<li>two</ul>", "<input disabled><br>", ""] {
            assert!(!parser.parse(source, None).unwrap().root_node().has_error());
        }
        // An unfinished quoted attribute must still yield a parser error for diagnostics.
        assert!(
            parser
                .parse("<div class=\"unfinished", None)
                .unwrap()
                .root_node()
                .has_error()
        );
    }

    /// Validate the shipped JS parser and captures across modules, modern syntax, and JSX.
    #[test]
    fn bundled_javascript_plugin_parses_and_highlights_source() {
        use tree_sitter::StreamingIterator as _;

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/javascript");
        let manifest =
            PluginManifest::parse(&fs::read_to_string(root.join("plugin.toml")).unwrap()).unwrap();
        let (grammar, query) =
            load_plugin_language(&root.canonicalize().unwrap(), &manifest.highlighters[0]).unwrap();
        let (mut parser, language) = create_parser(&grammar).unwrap();
        parser.set_language(&language).unwrap();
        let source = r#"// Unicode text and modern syntax remain valid in JavaScript.
import { readFile } from "node:fs/promises";
export async function greet(user) {
    const label = user?.name ?? "世界😀";
    await readFile(`./${label}.txt`);
    return <section title={label}>{label}<br /></section>;
}"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let query = tree_sitter::Query::new(&language, &query).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        let mut captures = Vec::new();
        while let Some(found) = matches.next() {
            for capture in found.captures {
                captures.push((
                    query.capture_names()[capture.index as usize],
                    &source[capture.node.byte_range()],
                ));
            }
        }
        for expected in [
            ("keyword", "export"),
            ("function", "greet"),
            ("variable.parameter", "user"),
            ("property", "name"),
            ("operator", "??"),
            ("string", "\"世界😀\""),
            ("tag", "section"),
            ("attribute", "title"),
        ] {
            assert!(
                captures.contains(&expected),
                "missing capture: {expected:?}"
            );
        }
        // CommonJS, classes, regex literals, and JSX fragments use the same parser.
        for source in [
            "module.exports = value => /hello/iu.test(value);",
            "class Counter { #value = 0; next() { return ++this.#value; } }",
            "const view = <><Widget {...props} /></>;",
            "",
        ] {
            assert!(!parser.parse(source, None).unwrap().root_node().has_error());
        }
        // Recovery nodes must expose invalid code to the editor's syntax diagnostics.
        for source in ["const value = ;", "function broken( {", "const x = <div>"] {
            assert!(parser.parse(source, None).unwrap().root_node().has_error());
        }
    }

    /// The bundled Rust plugin must supply a usable WASM parser and highlight query.
    #[test]
    fn bundled_rust_plugin_parses_source() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/rust");
        let manifest =
            PluginManifest::parse(&fs::read_to_string(root.join("plugin.toml")).unwrap()).unwrap();
        let contribution = manifest
            .highlighters
            .iter()
            .find(|language| language.language == "rust")
            .unwrap();
        let (grammar, _) =
            load_plugin_language(&root.canonicalize().unwrap(), contribution).unwrap();
        let (mut parser, language) = create_parser(&grammar).unwrap();
        parser.set_language(&language).unwrap();
        let tree = parser
            .parse("fn main() { let value: Option<u8> = None; }", None)
            .unwrap();
        assert!(!tree.root_node().has_error());
    }
}
