//! Loads declarative language plugins into GPUI Kit's Tree-sitter registry.

use anyhow::{Context as _, ensure};
use gpui_kit::{
    SharedString,
    component::highlighter::{GrammarConfig, LanguageParserFactory, LanguageRegistry},
};
use plugin_schema::{LanguageContribution, PluginManifest};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tree_sitter::{Parser, WasmStore, wasmtime::Engine};

/// Finds the plugin contribution responsible for a source file's extension.
pub fn language_for_path(path: &Path) -> Option<LanguageContribution> {
    crate::extensions::contributions::language_for_path(path).map(|(_, language)| language)
}

/// The bundled plugins are individually tracked so one failure cannot hide another result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundledPlugin {
    Rust,
    Toml,
}

impl BundledPlugin {
    pub const ALL: [Self; 2] = [Self::Rust, Self::Toml];

    pub fn name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Toml => "TOML",
        }
    }

    pub fn language_id(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Toml => "toml",
        }
    }

    pub fn manifest_id(self) -> &'static str {
        match self {
            Self::Rust => "me.rust",
            Self::Toml => "me.toml",
        }
    }
}

/// Mask host grammars before restored files can request a parser.
pub fn prepare_bundled_plugins() {
    let registry = LanguageRegistry::singleton();
    for plugin in BundledPlugin::ALL {
        // Explicit IDs keep host grammars inert even if a manifest cannot be parsed.
        registry.register(
            plugin.language_id(),
            &GrammarConfig::plain(plugin.language_id().to_owned()),
        );
    }
}

/// Validate and register one installed grammar without delaying the initial window.
pub fn load_bundled_plugin(plugin: BundledPlugin) -> anyhow::Result<()> {
    let root = crate::extensions::contributions::plugin_root(plugin.manifest_id())
        .ok_or_else(|| anyhow::anyhow!("{} plugin is not installed and enabled", plugin.name()))?;
    let source = fs::read_to_string(root.join("plugin.toml"))?;
    let manifest = PluginManifest::parse(&source)?;
    let contribution = manifest
        .languages
        .iter()
        .find(|language| language.id == plugin.language_id())
        .ok_or_else(|| anyhow::anyhow!("{} grammar is missing from its package", plugin.name()))?;
    let (grammar, query) = load_plugin_language(&root, contribution)?;
    // A disabled or replaced package must not publish a parser after its worker finishes.
    ensure!(
        crate::extensions::contributions::plugin_root(plugin.manifest_id()).as_deref()
            == Some(root.as_path()),
        "plugin changed while its grammar was loading"
    );
    register_language_config(LanguageRegistry::singleton(), contribution, grammar, query);
    Ok(())
}

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

/// Load every grammar declared by a plugin directory and register its parser and query.
pub fn register_plugin(plugin_root: &Path) -> anyhow::Result<()> {
    let root = plugin_root
        .canonicalize()
        .with_context(|| format!("resolve plugin directory {}", plugin_root.display()))?;
    let manifest_source = fs::read_to_string(root.join("plugin.toml"))
        .with_context(|| format!("read plugin manifest in {}", root.display()))?;
    let manifest =
        PluginManifest::parse(&manifest_source).context("parse language plugin manifest")?;
    let registry = LanguageRegistry::singleton();
    let mut failures = Vec::new();

    for contribution in &manifest.languages {
        let result = load_plugin_language(root.as_path(), contribution);
        match result {
            Ok((grammar, query)) => {
                register_language_config(registry, contribution, grammar, query)
            }
            Err(error) => {
                register_plain_language(registry, contribution);
                // The plain registration intentionally prevents fallback to a host grammar.
                tracing::warn!(
                    language = %contribution.id,
                    plugin = %manifest.plugin.id,
                    %error,
                    "language plugin grammar could not be loaded"
                );
                failures.push(format!("{}: {error:#}", contribution.id));
            }
        }
    }

    ensure!(
        failures.is_empty(),
        "plugin {} failed to load: {}",
        manifest.plugin.id,
        failures.join("; ")
    );
    Ok(())
}

/// Load a grammar and query from a plugin directory before registering them.
fn load_plugin_language(
    plugin_root: &Path,
    contribution: &LanguageContribution,
) -> anyhow::Result<(Arc<LoadedGrammar>, String)> {
    let grammar_path = plugin_asset(plugin_root, &contribution.grammar)?;
    let highlights_path = plugin_asset(plugin_root, &contribution.highlights)?;
    let grammar_bytes = fs::read(&grammar_path)
        .with_context(|| format!("read grammar {}", grammar_path.display()))?;
    let query = fs::read_to_string(&highlights_path)
        .with_context(|| format!("read highlights {}", highlights_path.display()))?;
    load_language(contribution, grammar_bytes, query)
}

/// Replace a static language entry with an inert one before plugin validation.
fn register_plain_language(registry: &LanguageRegistry, contribution: &LanguageContribution) {
    registry.register(
        &contribution.id,
        &GrammarConfig::plain(contribution.id.clone()),
    );
}

/// Attach the plugin query and WASM-backed parser factory to the host registry.
fn register_language_config(
    registry: &LanguageRegistry,
    contribution: &LanguageContribution,
    grammar: Arc<LoadedGrammar>,
    query: String,
) {
    let mut config = GrammarConfig::plain(contribution.id.clone());
    config.highlights = SharedString::from(query);
    registry.register(&contribution.id, &config);
    registry.register_parser_factory(&contribution.id, parser_factory(grammar));
}

/// Owns the WASM store for as long as parsers can use its language handle.
struct LoadedGrammar {
    engine: Engine,
    language_id: String,
    bytes: Arc<[u8]>,
}

/// Read and validate a grammar module and its highlight query from one plugin root.
fn load_language(
    contribution: &LanguageContribution,
    grammar_bytes: Vec<u8>,
    query: String,
) -> anyhow::Result<(Arc<LoadedGrammar>, String)> {
    let expected_abi = contribution.tree_sitter_abi;

    let engine = Engine::default();
    let mut store = WasmStore::new(&engine).context("create Tree-sitter WASM store")?;
    let language = store
        .load_language(&contribution.id, &grammar_bytes)
        .with_context(|| format!("load WASM grammar for {}", contribution.id))?;
    ensure!(
        language.abi_version() == expected_abi as usize,
        "grammar ABI mismatch for {}: manifest declares {}, module exports {}",
        contribution.id,
        expected_abi,
        language.abi_version()
    );
    ensure!(
        (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION)
            .contains(&language.abi_version()),
        "grammar ABI {} for {} is unsupported by this Tree-sitter runtime",
        language.abi_version(),
        contribution.id
    );
    tree_sitter::Query::new(&language, &query)
        .with_context(|| format!("compile highlight query for {}", contribution.id))?;

    let grammar = Arc::new(LoadedGrammar {
        engine,
        language_id: contribution.id.clone(),
        bytes: grammar_bytes.into(),
    });
    let (mut parser, language) = create_parser(&grammar)?;
    parser
        .set_language(&language)
        .context("set dynamically loaded Tree-sitter grammar")?;
    let tree = parser
        .parse("", None)
        .context("parse validation sample with WASM grammar")?;
    ensure!(
        !tree.root_node().has_error(),
        "WASM grammar rejected the validation sample for {}",
        contribution.id
    );
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
        prepare_bundled_plugins();
        for (name, directory) in [("Rust", "rust"), ("TOML", "toml")] {
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
id = "me.missing-test"
name = "Missing test"
version = "0.1.0"
host_version = ">=0.1.0"

[[languages]]
id = "missing-test"
extensions = ["missing-test"]
grammar = "grammar/missing.wasm"
highlights = "queries/missing.scm"
tree_sitter_abi = 15
"#,
        )
        .unwrap();
        let error = register_plugin(directory.path()).unwrap_err();
        assert!(error.to_string().contains("me.missing-test failed to load"));
    }

    /// The bundled Rust plugin must supply a usable WASM parser and highlight query.
    #[test]
    fn bundled_rust_plugin_parses_source() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/rust");
        let manifest =
            PluginManifest::parse(&fs::read_to_string(root.join("plugin.toml")).unwrap()).unwrap();
        let contribution = manifest
            .languages
            .iter()
            .find(|language| language.id == "rust")
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
