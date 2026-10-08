//! Immutable provider snapshots separate recognition from grammar choice and stale task publication.
use plugin_runtime::plugin_protocol::settings::Scope;
use plugin_schema::{Highlighter, LanguageDefinition};
mod associations;
mod editing;
mod formatting;
mod preferences;
pub(crate) use associations::{associate_extension, file_associations};
pub(crate) use editing::{editing_preferences, has_editing_override, set_editing_preference};
use formatting::FormatterPreferenceError;
pub(crate) use formatting::{formatter_error, formatters};
use preferences::Saved;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        LazyLock, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

/// Package identity and content-addressed root travel with every background grammar load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GrammarProvider {
    pub owner: String,
    pub root: PathBuf,
    pub declaration: Highlighter,
}

#[derive(Default)]
struct Registry {
    root: PathBuf,
    workspace: String,
    saved: Saved,
    error: Option<String>,
    recognizers: BTreeMap<String, Vec<(String, LanguageDefinition)>>,
    highlighters: BTreeMap<String, Vec<GrammarProvider>>,
    language_servers: BTreeMap<String, Vec<String>>,
    /// Formatting and structure remain selectable without changing analysis or grammar choices.
    formatters: BTreeMap<String, Vec<String>>,
    /// Invalid explicit formatting values retain their scope without poisoning other preference edits.
    formatter_preference_errors: BTreeMap<String, FormatterPreferenceError>,
    structure_providers: BTreeMap<String, Vec<String>>,
    /// Retain a valid selected provider when another package joins the candidate set.
    selected: BTreeMap<String, String>,
    /// Never reuse an earlier selection generation, including disable/re-enable and A/B/A choices.
    highlight_epoch: u64,
}
static REGISTRY: LazyLock<RwLock<Registry>> = LazyLock::new(|| RwLock::new(Registry::default()));
static NEXT_HIGHLIGHT_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Configuration rebuilds cannot reset this process-wide generation counter.
fn next_highlight_epoch() -> u64 {
    NEXT_HIGHLIGHT_EPOCH
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
            epoch.checked_add(1)
        })
        .expect("language provider generation exhausted")
        + 1
}

/// A refresh replaces all contributions together; disabled and failed packages are absent upstream.
pub(crate) fn refresh(
    root: &Path,
    entries: Vec<(String, PathBuf, Vec<LanguageDefinition>, Vec<Highlighter>)>,
    services: Vec<(
        String,
        Vec<plugin_runtime::plugin_protocol::language::Provider>,
    )>,
) {
    refresh_with_structures(root, entries, services, Vec::new());
}

/// Publish independent language roles as one snapshot; callers without structure use `refresh`.
pub(crate) fn refresh_with_structures(
    root: &Path,
    entries: Vec<(String, PathBuf, Vec<LanguageDefinition>, Vec<Highlighter>)>,
    services: Vec<(
        String,
        Vec<plugin_runtime::plugin_protocol::language::Provider>,
    )>,
    structures: Vec<(
        String,
        Vec<plugin_runtime::plugin_protocol::structure::Provider>,
    )>,
) {
    let mut next = Registry::default();
    for (owner, providers) in services {
        for provider in providers {
            let key = format!("{owner}/{}", provider.id);
            if provider.primary {
                next.language_servers
                    .entry(provider.language.clone())
                    .or_default()
                    .push(key.clone());
            }
            if provider.formatting {
                next.formatters
                    .entry(provider.language)
                    .or_default()
                    .push(key);
            }
        }
    }
    for (owner, providers) in structures {
        for provider in providers {
            next.structure_providers
                .entry(provider.language)
                .or_default()
                .push(format!("{owner}/{}", provider.id));
        }
    }
    for (owner, root, definitions, highlighters) in entries {
        for definition in definitions {
            for key in definition
                .extensions
                .iter()
                .map(|s| format!("ext:{}", s.to_lowercase()))
                .chain(
                    definition
                        .filenames
                        .iter()
                        .map(|s| format!("file:{}", s.to_lowercase())),
                )
            {
                next.recognizers
                    .entry(key)
                    .or_default()
                    .push((owner.clone(), definition.clone()));
            }
        }
        for declaration in highlighters {
            next.highlighters
                .entry(declaration.language.clone())
                .or_default()
                .push(GrammarProvider {
                    owner: owner.clone(),
                    root: root.clone(),
                    declaration,
                });
        }
    }
    let mut registry = REGISTRY.write().unwrap();
    if !same_root(&registry.root, root) {
        *registry = configured(root, "");
    }
    next.root = registry.root.clone();
    next.workspace = registry.workspace.clone();
    next.saved = registry.saved.clone();
    next.error = registry.error.clone();
    let before = next.saved.clone();
    next.resolve();
    // Remember sole-provider adoption so a later install (or restart) cannot reorder choices.
    if next.saved != before && next.error.is_none() {
        if let Err(error) = next.saved.write(&next.root) {
            next.error = Some(format!("{error:#}"));
        }
    }
    // Alias declarations affect code-block selection just as much as the selected grammar does.
    // Equal refreshes preserve reusable work; every observable change invalidates old jobs.
    next.highlight_epoch = if registry.root == next.root
        && registry.workspace == next.workspace
        && registry.recognizers == next.recognizers
        && registry.highlighters == next.highlighters
        && registry.selected == next.selected
    {
        registry.highlight_epoch
    } else {
        next_highlight_epoch()
    };
    let epoch = next.highlight_epoch;
    *registry = next;
    drop(registry);
    super::code_highlighting::invalidate_prepared(epoch);
}

impl Registry {
    fn resolve(&mut self) {
        self.selected.clear();
        self.formatter_preference_errors.clear();
        // Verification is tied to current explicit values, never a history of installed providers.
        self.saved
            .formatter_choices
            .prune(&self.saved.user, &self.saved.projects);
        for (key, candidates) in self.candidates() {
            if !self.validate_formatter_explicit(&key, &candidates) {
                continue;
            }
            let explicit = self
                .saved
                .projects
                .get(&self.workspace)
                .and_then(|m| m.get(&key))
                .or_else(|| self.saved.user.get(&key));
            let remembered = self
                .saved
                .automatic
                .get(&self.workspace)
                .and_then(|m| m.get(&key));
            let choice = explicit
                .filter(|id| candidates.contains(id))
                .or_else(|| remembered.filter(|id| candidates.contains(id)))
                .cloned()
                .or_else(|| (candidates.len() == 1).then(|| candidates[0].clone()));
            if let Some(choice) = choice {
                self.saved
                    .automatic
                    .entry(self.workspace.clone())
                    .or_default()
                    .insert(key.clone(), choice.clone());
                self.selected.insert(key, choice);
            }
        }
    }
    /// Selection keys separate file recognition from the language's highlight provider.
    fn candidates(&self) -> BTreeMap<String, Vec<String>> {
        let mut rows = BTreeMap::new();
        for (role, declarations) in [
            ("formatter", &self.formatters),
            ("structure", &self.structure_providers),
        ] {
            for (language, providers) in declarations {
                rows.insert(format!("{role}:{language}"), providers.clone());
            }
        }
        for (language, providers) in &self.language_servers {
            rows.insert(format!("lsp:{language}"), providers.clone());
        }
        for (key, definitions) in &self.recognizers {
            rows.insert(
                format!("recognition:{key}"),
                definitions
                    .iter()
                    .map(|(owner, definition)| format!("{owner}/{}", definition.id))
                    .collect(),
            );
        }
        for (language, providers) in &self.highlighters {
            rows.insert(
                format!("highlight:{language}"),
                providers
                    .iter()
                    .map(|provider| format!("{}/{}", provider.owner, provider.declaration.id))
                    .collect(),
            );
        }
        // A loaded invalid formatter must remain visible and resettable even with no live candidates.
        for key in self
            .saved
            .user
            .keys()
            .chain(
                self.saved
                    .projects
                    .get(&self.workspace)
                    .into_iter()
                    .flat_map(|layer| layer.keys()),
            )
            .chain(
                self.saved
                    .automatic
                    .get(&self.workspace)
                    .into_iter()
                    .flat_map(|layer| layer.keys()),
            )
            .filter(|key| key.starts_with("formatter:"))
        {
            rows.entry(key.clone()).or_default();
        }
        rows
    }

    /// Canonical IDs win over aliases; ambiguous display names never depend on install ordering.
    fn code_language(&self, language: &str) -> Option<String> {
        if language.len() > 128 {
            return None;
        }
        let language = language.trim().to_lowercase();
        if language.is_empty() {
            return None;
        }
        let definitions = || {
            self.recognizers
                .values()
                .flatten()
                .map(|(_, definition)| definition)
        };
        if self.highlighters.contains_key(&language)
            || definitions().any(|definition| definition.id == language)
        {
            return Some(language);
        }
        let mut candidates = definitions()
            .filter(|definition| definition.name.to_lowercase() == language)
            .map(|definition| definition.id.clone())
            .collect::<BTreeSet<_>>();
        let extension = format!("ext:{}", language.strip_prefix('.').unwrap_or(&language));
        if let Some(definitions) = self.recognizers.get(&extension) {
            // Recognition preferences resolve shared extensions independently of grammar choice.
            let owner = self.selected.get(&format!("recognition:{extension}"))?;
            let (_, definition) = definitions
                .iter()
                .find(|(owner_id, definition)| format!("{owner_id}/{}", definition.id) == *owner)?;
            candidates.insert(definition.id.clone());
        }
        (candidates.len() == 1).then(|| candidates.into_iter().next().unwrap())
    }

    /// Resolve one language's selected package without consulting the upstream parser registry.
    fn grammar(&self, language: &str) -> Option<&GrammarProvider> {
        let selected = self.selected.get(&format!("highlight:{language}"))?;
        self.highlighters
            .get(language)?
            .iter()
            .find(|provider| format!("{}/{}", provider.owner, provider.declaration.id) == *selected)
    }
}

/// Workspace context is host-owned; reading repository files cannot set these preferences.
pub(crate) fn configure(root: &Path, workspace: &Path) {
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.into())
        .display()
        .to_string();
    let mut registry = REGISTRY.write().unwrap();
    if !same_root(&registry.root, root) || registry.workspace != workspace {
        *registry = configured(root, &workspace);
        let epoch = registry.highlight_epoch;
        drop(registry);
        super::code_highlighting::invalidate_prepared(epoch);
    }
}

/// Windows verbatim and normal paths identify the same host store, preserving its active workspace.
fn same_root(a: &Path, b: &Path) -> bool {
    a.canonicalize().unwrap_or_else(|_| a.into()) == b.canonicalize().unwrap_or_else(|_| b.into())
}

fn configured(root: &Path, workspace: &str) -> Registry {
    let (saved, error) = match Saved::read(root) {
        Ok(saved) => (saved, None),
        Err(error) => (Saved::default(), Some(format!("{error:#}"))),
    };
    Registry {
        root: root.into(),
        workspace: workspace.into(),
        saved,
        error,
        highlight_epoch: next_highlight_epoch(),
        ..Default::default()
    }
}

/// Settings rendering uses this immutable snapshot and never reads package files while drawing.
pub(crate) struct ProviderRow {
    pub key: String,
    pub candidates: Vec<String>,
    pub selected: Option<String>,
    pub source: &'static str,
    /// Only never-validated explicit values are errors; normally withdrawn choices retain fallback rows.
    pub configuration_error: Option<FormatterPreferenceError>,
}

pub(crate) fn rows() -> Vec<ProviderRow> {
    let registry = REGISTRY.read().unwrap();
    let mut rows = registry.candidates();
    for key in registry
        .saved
        .automatic
        .get(&registry.workspace)
        .into_iter()
        .flatten()
        .map(|(key, _)| key)
    {
        rows.entry(key.clone()).or_default();
    }
    rows.into_iter()
        .map(|(key, candidates)| {
            let selected = registry.selected.get(&key).cloned();
            let configuration_error = registry.formatter_preference_errors.get(&key).cloned();
            let source = if let Some(error) = &configuration_error {
                error.source
            } else if registry
                .saved
                .projects
                .get(&registry.workspace)
                .and_then(|m| m.get(&key))
                .is_some_and(|id| Some(id) == selected.as_ref())
            {
                "project"
            } else if registry
                .saved
                .user
                .get(&key)
                .is_some_and(|id| Some(id) == selected.as_ref())
            {
                "user"
            } else {
                "automatic"
            };
            ProviderRow {
                key,
                candidates,
                selected,
                source,
                configuration_error,
            }
        })
        .collect()
}

pub(crate) fn error() -> Option<String> {
    REGISTRY.read().unwrap().error.clone()
}

/// Explicit native UI selection persists atomically before changing the active provider snapshot.
pub(crate) fn choose(scope: Scope, key: &str, provider: Option<&str>) -> anyhow::Result<()> {
    let mut registry = REGISTRY.write().unwrap();
    anyhow::ensure!(
        registry.error.is_none(),
        "Provider preferences could not be loaded or saved"
    );
    let candidates = registry.candidates();
    anyhow::ensure!(candidates.contains_key(key), "Unknown provider role");
    if let Some(provider) = provider {
        anyhow::ensure!(
            candidates[key].iter().any(|id| id == provider),
            "Provider is unavailable"
        );
    }
    let mut saved = registry.saved.clone();
    let layer = match scope {
        Scope::User => &mut saved.user,
        Scope::Project => {
            anyhow::ensure!(!registry.workspace.is_empty(), "No active workspace");
            saved
                .projects
                .entry(registry.workspace.clone())
                .or_default()
        }
    };
    if let Some(provider) = provider {
        layer.insert(key.into(), provider.into());
    } else {
        layer.remove(key);
    }
    // This UI entry has just checked live candidates. Persist its exact value with the same
    // atomic snapshot; resetting a layer also removes its proof rather than retaining history.
    saved
        .formatter_choices
        .record(scope, &registry.workspace, key, provider);
    // A reset deliberately relinquishes remembered choice in this workspace before fallback resolution.
    saved
        .automatic
        .entry(registry.workspace.clone())
        .or_default()
        .remove(key);
    saved.write(&registry.root)?;
    let previous = registry.selected.clone();
    registry.saved = saved;
    registry.resolve();
    if registry.selected != previous {
        registry.highlight_epoch = next_highlight_epoch();
    }
    let epoch = registry.highlight_epoch;
    drop(registry);
    super::code_highlighting::invalidate_prepared(epoch);
    Ok(())
}

/// Recognition is available independently of whether the grammar exists or loaded successfully.
pub(crate) fn language_for_path(path: &Path) -> Option<String> {
    let registry = REGISTRY.read().unwrap();
    let filename = path.file_name()?.to_str()?.to_lowercase();
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    // An explicit association wins while its language exists; a missing provider stays plain.
    if let Some(language) = registry.saved.associations.get(&extension) {
        return registry.code_language(language);
    }
    let key = if registry
        .recognizers
        .contains_key(&format!("file:{filename}"))
    {
        format!("file:{filename}")
    } else {
        format!("ext:{extension}")
    };
    let selected = registry.selected.get(&format!("recognition:{key}"))?;
    registry
        .recognizers
        .get(&key)?
        .iter()
        .find(|(owner, definition)| format!("{owner}/{}", definition.id) == *selected)
        .map(|(_, definition)| definition.id.clone())
}

pub(crate) fn grammars() -> Vec<GrammarProvider> {
    let registry = REGISTRY.read().unwrap();
    registry
        .highlighters
        .iter()
        .filter_map(|(language, list)| {
            let selected = registry.selected.get(&format!("highlight:{language}"))?;
            list.iter()
                .find(|p| format!("{}/{}", p.owner, p.declaration.id) == *selected)
                .cloned()
        })
        .collect()
}

/// LSP selection shares preference precedence but never depends on the highlighter provider.
pub(crate) fn language_servers() -> BTreeMap<String, Option<String>> {
    let registry = REGISTRY.read().unwrap();
    registry
        .language_servers
        .keys()
        .map(|language| {
            (
                language.clone(),
                registry.selected.get(&format!("lsp:{language}")).cloned(),
            )
        })
        .collect()
}

/// Structure follows the same stable choice and user/project precedence as every language role.
pub(crate) fn structure_provider(language: &str) -> Option<String> {
    REGISTRY
        .read()
        .unwrap()
        .selected
        .get(&format!("structure:{language}"))
        .cloned()
}

/// A removed or replaced provider cannot register a parser after its task completes.
pub(crate) fn is_current(provider: &GrammarProvider) -> bool {
    grammars().contains(provider)
}

/// Snapshot identity and generation together so a worker cannot observe a torn provider choice.
pub(super) fn code_provider(language: &str) -> Option<(GrammarProvider, u64)> {
    let registry = REGISTRY.read().unwrap();
    let language = registry.code_language(language)?;
    Some((
        registry.grammar(&language)?.clone(),
        registry.highlight_epoch,
    ))
}

/// Native owners observe this generation even when no grammar is currently available.
pub(super) fn code_epoch() -> u64 {
    REGISTRY.read().unwrap().highlight_epoch
}

/// Equality alone cannot reject results from a provider that was removed then reinstalled.
pub(super) fn code_is_current(provider: &GrammarProvider, epoch: u64) -> bool {
    let registry = REGISTRY.read().unwrap();
    registry.highlight_epoch == epoch
        && registry.grammar(&provider.declaration.language) == Some(provider)
}

/// Dynamic recognition claims this path even when ambiguity prevents choosing a language.
pub(crate) fn handles_path(path: &Path) -> bool {
    let registry = REGISTRY.read().unwrap();
    let file = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    registry.saved.associations.contains_key(&ext)
        || registry.recognizers.contains_key(&format!("file:{file}"))
        || registry.recognizers.contains_key(&format!("ext:{ext}"))
}

pub(crate) fn languages() -> std::collections::BTreeSet<String> {
    let registry = REGISTRY.read().unwrap();
    registry
        .recognizers
        .values()
        .flatten()
        .map(|(_, definition)| definition.id.clone())
        .chain(registry.highlighters.keys().cloned())
        .collect()
}
