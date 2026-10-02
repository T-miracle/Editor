//! Immutable provider snapshots separate recognition from grammar choice and stale task publication.
use plugin_runtime::plugin_protocol::settings::Scope;
use plugin_schema::{Highlighter, LanguageDefinition};
mod preferences;
use preferences::Saved;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{LazyLock, RwLock},
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
    /// Retain a valid selected provider when another package joins the candidate set.
    selected: BTreeMap<String, String>,
}
static REGISTRY: LazyLock<RwLock<Registry>> = LazyLock::new(|| RwLock::new(Registry::default()));

/// A refresh replaces all contributions together; disabled and failed packages are absent upstream.
pub(crate) fn refresh(
    root: &Path,
    entries: Vec<(String, PathBuf, Vec<LanguageDefinition>, Vec<Highlighter>)>,
) {
    let mut next = Registry::default();
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
    *registry = next;
}

impl Registry {
    fn resolve(&mut self) {
        self.selected.clear();
        for (key, candidates) in self.candidates() {
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
        rows
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
        ..Default::default()
    }
}

/// Settings rendering uses this immutable snapshot and never reads package files while drawing.
pub(crate) struct ProviderRow {
    pub key: String,
    pub candidates: Vec<String>,
    pub selected: Option<String>,
    pub source: &'static str,
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
            let source = if registry
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
    // A reset deliberately relinquishes remembered choice in this workspace before fallback resolution.
    saved
        .automatic
        .entry(registry.workspace.clone())
        .or_default()
        .remove(key);
    saved.write(&registry.root)?;
    registry.saved = saved;
    registry.resolve();
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

/// A removed or replaced provider cannot register a parser after its task completes.
pub(crate) fn is_current(provider: &GrammarProvider) -> bool {
    grammars().contains(provider)
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
    registry.recognizers.contains_key(&format!("file:{file}"))
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
