//! User-selected file associations change recognition without granting plugin execution.
use super::*;

/// Persist one bounded extension-to-language mapping before refreshing any open editor.
///
/// A leading dot and case are normalized. Removing a mapping uses `None`.
/// Unknown languages, unsafe extensions or unreadable preferences leave the current mapping intact.
pub(crate) fn associate_extension(extension: &str, language: Option<&str>) -> anyhow::Result<()> {
    let extension = extension.trim();
    let extension = extension
        .strip_prefix('.')
        .unwrap_or(extension)
        .to_lowercase();
    anyhow::ensure!(valid_extension(&extension), "Invalid file extension");
    let mut registry = REGISTRY.write().unwrap();
    anyhow::ensure!(
        registry.error.is_none(),
        "Provider preferences are unavailable"
    );
    let mut saved = registry.saved.clone();
    if let Some(language) = language {
        let language = registry
            .code_language(language)
            .ok_or_else(|| anyhow::anyhow!("Language is unavailable"))?;
        saved.associations.insert(extension, language);
        anyhow::ensure!(
            saved.associations.len() <= 512,
            "Too many file associations"
        );
    } else {
        saved.associations.remove(&extension);
    }
    saved.write(&registry.root)?;
    registry.saved = saved;
    registry.highlight_epoch = next_highlight_epoch();
    let epoch = registry.highlight_epoch;
    drop(registry);
    super::super::code_highlighting::invalidate_prepared(epoch);
    Ok(())
}

/// The native settings view displays stable user mappings even when a plugin is unavailable.
pub(crate) fn file_associations() -> BTreeMap<String, String> {
    REGISTRY.read().unwrap().saved.associations.clone()
}

/// Match declarative selector constraints, including non-ASCII file extensions.
pub(super) fn valid_extension(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= 128
        && !extension.contains(['.', '/', '\\', '*', '?', ':', '[', ']'])
        && !extension.chars().any(char::is_control)
}
