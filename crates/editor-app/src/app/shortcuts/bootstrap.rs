//! Initializes the shared shortcut profile before an editor window can receive input.

use super::{catalog, engine::BindingEngine};
use crate::ui::controls::Button;
use gpui_kit::{App, BorrowAppContext as _, Global};
use std::path::PathBuf;

/// A failed load still retains its selected path; ensure must not silently switch profiles.
struct ProfilePath(PathBuf);
impl Global for ProfilePath {}

/// Resolve only the shortcut profile, never the workspace, plugin registry or other settings.
/// ME_EDITOR_PROFILE_HOME is an optional directory for isolated runs and application tests.
pub(crate) fn default_path() -> Result<PathBuf, String> {
    if let Some(directory) =
        std::env::var_os("ME_EDITOR_PROFILE_HOME").filter(|directory| !directory.is_empty())
    {
        return Ok(PathBuf::from(directory).join("shortcuts.json"));
    }
    dirs::config_dir()
        .map(|directory| directory.join("MeEditor").join("shortcuts.json"))
        .ok_or_else(|| "The user configuration directory is unavailable".to_owned())
}

/// Load and apply all native overrides before opening a window, including Input Copy bindings.
/// Call after gpui_kit::init and existing host registration; this never initializes plugins.
/// One App uses one profile. Repeated calls for that profile synchronize late default additions.
/// Malformed data and write-independent application failures are returned without overwriting it.
pub(crate) fn configure(cx: &mut App, path: PathBuf) -> Result<(), String> {
    if cx.has_global::<ProfilePath>() && cx.global::<ProfilePath>().0 != path {
        return Err("This application already uses a different shortcut profile".to_owned());
    }
    if cx.has_global::<BindingEngine>() {
        return ensure(cx);
    }
    cx.set_global(ProfilePath(path.clone()));
    super::init(cx);
    Button::init_keys(cx);
    let defaults = cx.key_bindings().borrow().bindings().cloned().collect();
    let mut engine = BindingEngine::load(path, defaults).map_err(|error| format!("{error:?}"))?;
    let operations = catalog::all_operations(engine.defaults(), &[], cx);
    engine.register(operations);
    engine.apply(cx).map_err(|error| format!("{error:?}"))?;
    cx.set_global(engine);
    Ok(())
}

/// Reuse the current App's profile across workspace windows and controlled application fixtures.
/// Additional native controls are absorbed before rebuilding the complete default catalog, so
/// the effective keymap never becomes the source of a later window's apparent defaults.
pub(crate) fn ensure(cx: &mut App) -> Result<(), String> {
    if !cx.has_global::<BindingEngine>() {
        let path = if cx.has_global::<ProfilePath>() {
            cx.global::<ProfilePath>().0.clone()
        } else {
            default_path()?
        };
        return configure(cx, path);
    }
    super::init(cx);
    Button::init_keys(cx);
    cx.update_global::<BindingEngine, _>(|engine, cx| {
        let defaults_changed = engine.synchronize_defaults(cx);
        let operations = catalog::all_operations(engine.defaults(), &[], cx);
        // A late same-key/same-predicate registration can supersede an old native target.
        // Retire absent catalog entries too, so resolver candidates cannot retain that target.
        let removed = engine
            .active_operations()
            .filter(|operation| {
                matches!(operation.target, catalog::Target::Native { .. })
                    && !operations.iter().any(|next| next.id == operation.id)
            })
            .map(|operation| operation.id.clone())
            .collect::<Vec<_>>();
        let removed_operations = !removed.is_empty();
        for id in removed {
            engine.unregister(&id);
        }
        let operations_changed = engine.register(operations);
        if defaults_changed || operations_changed || removed_operations {
            engine.apply(cx).map_err(|error| format!("{error:?}"))?;
        }
        Ok(())
    })
}
