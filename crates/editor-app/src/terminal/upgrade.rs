//! Finite import of retired terminal data before any runtime component is activated.
mod legacy;
mod storage;
use plugin_runtime::{Installed, Manager};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};
use storage::{Kind, Locations, Plan};

/// Resolve this application's existing roots before the configuration loader or runtime worker runs.
pub(crate) fn prepare(workspace: &Path) -> anyhow::Result<()> {
    let runtime = crate::extensions::runtime_root(workspace);
    let native = super::persistence::directory(workspace);
    let runs = editor_core::default_root()
        .ok_or_else(|| anyhow::anyhow!("Profile directory unavailable"))?;
    let session = crate::app::session::SessionState::for_workspace(workspace)
        .file_path()
        .ok_or_else(|| anyhow::anyhow!("Session directory unavailable"))?;
    perform(&runtime, workspace, &native, &runs, &session)
}

/// Prepare one workspace's logical state; no native program or guest code is launched by an import.
pub(super) fn perform(
    runtime: &Path,
    workspace: &Path,
    native: &Path,
    runs: &Path,
    session: &Path,
) -> anyhow::Result<()> {
    let old = retire_installation(runtime)?;
    // An installed package outside the retired format owns its existing references. Historical
    // aliases may migrate only when no current package still owns that identity.
    let registry = Manager::read_registry(runtime)?;
    let identities = ["terminal", "me.terminal"]
        .into_iter()
        .filter(|id| !registry.contains_key(*id))
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    let config_path = editor_core::configuration_path(runs, &workspace.display().to_string());
    let locations = Locations {
        native,
        configurations: &config_path,
        layout: session,
        runtime,
    };
    if let Some(bytes) = storage::read(&native.join("upgrade-complete.json"), 256)? {
        anyhow::ensure!(
            serde_json::from_slice::<Value>(&bytes)?["version"] == 1,
            "Invalid migration completion receipt"
        );
        // A crash after the receipt but before journal removal has already committed all targets.
        // Never compare or overwrite user edits made after that completed migration.
        match std::fs::remove_file(native.join("upgrade-pending.json")) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        return Ok(());
    }
    if storage::resume(&locations)? {
        return Ok(());
    }
    let mut plan = Plan::default();
    let native_exists = native.join("state.json").exists();
    if let Some(entry) = old.filter(|entry| identities.contains(&entry.manifest.id)) {
        let files = Manager::persisted_data_directory(
            runtime,
            &entry.manifest,
            &workspace.display().to_string(),
        )?;
        // The runtime allowed 40 MiB for the outer JSON file, while the retired package's
        // decoded Snapshot.data quota was 16 MiB. Conversion applies the new 8 MiB budget.
        let saved = storage::read(
            &files.parent().unwrap().join("state.json"),
            40 * 1024 * 1024,
        )?;
        let settings = storage::read(&files.join("settings.json"), 65536)?;
        if let Some(bytes) = &saved {
            storage::backup(&native.join("upgrade-backup/state.json"), bytes)?;
        }
        if let Some(bytes) = &settings {
            storage::backup(&native.join("upgrade-backup/settings.json"), bytes)?;
        }
        if !native_exists {
            if let Some(bytes) = saved {
                plan.add(
                    &locations,
                    Kind::State,
                    legacy::convert(&bytes, settings.as_deref())?,
                )?;
            }
            if let Some(bytes) = settings
                && !native.join("settings.json").exists()
            {
                super::Settings::parse(std::str::from_utf8(&bytes)?)?;
                plan.add(&locations, Kind::Settings, bytes)?;
            }
        }
    }
    if let Some(bytes) = storage::read(&config_path, 1024 * 1024)? {
        let mut set = editor_core::RunConfigSet::from_json(&bytes)?;
        let mut changed = false;
        for configuration in &mut set.configurations {
            if configuration
                .provider
                .as_deref()
                .is_some_and(|id| identities.contains(id))
            {
                configuration.provider = Some("nanobug.execution".into());
                changed = true;
            }
        }
        for data in set.plugin_configurations.values_mut() {
            if identities.contains(&data.provider) {
                data.provider = super::configurations::PROVIDER.into();
                data.validation = editor_core::ConfigurationValidation::Unchecked;
                data.revision = data
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("Configuration revision exhausted"))?;
                changed = true;
            }
        }
        if changed {
            storage::backup(&native.join("upgrade-backup/configurations.json"), &bytes)?;
            plan.add(&locations, Kind::Configurations, set.to_json()?)?;
        }
    }
    if let Some(bytes) = storage::read(session, 1024 * 1024)? {
        // Preserve even malformed layout bytes before parsing; later automatic saves are blocked
        // by the startup error so the user can repair the source and resume this exact migration.
        storage::backup(&native.join("upgrade-backup/layout.json"), &bytes)?;
        let mut value: Value = serde_json::from_slice(&bytes)?;
        let saved = value["workspace"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Session workspace missing"))?;
        anyhow::ensure!(
            Path::new(saved)
                .canonicalize()
                .unwrap_or_else(|_| Path::new(saved).into())
                == workspace
                    .canonicalize()
                    .unwrap_or_else(|_| workspace.into()),
            "Session belongs to another workspace"
        );
        if migrate_layout(&mut value, !native_exists, &identities) {
            plan.add(&locations, Kind::Layout, serde_json::to_vec(&value)?)?;
        }
    }
    if let Some(bytes) = storage::read(&runtime.join("service-providers.json"), 1024 * 1024)? {
        let mut value: Value = serde_json::from_slice(&bytes)?;
        if migrate_providers(&mut value, &identities) {
            storage::backup(&native.join("upgrade-backup/providers.json"), &bytes)?;
            plan.add(&locations, Kind::Providers, serde_json::to_vec(&value)?)?;
        }
    }
    // An empty new installation needs no marker. Otherwise even a state-less source commits the receipt.
    if !native_exists
        && !native.join("upgrade-backup").exists()
        && !runtime
            .join("builtin-terminal-backup/installation.json")
            .exists()
    {
        return Ok(());
    }
    plan.stage(native)?;
    storage::resume(&locations)?;
    Ok(())
}

/// This exact identity/version range belongs only to import. Newer packages are not a host whitelist.
fn old_id(id: &str) -> bool {
    matches!(id, "terminal" | "me.terminal")
}
fn retired(entry: &Installed) -> bool {
    old_id(&entry.manifest.id)
        && semver::Version::parse(&entry.manifest.version)
            .is_ok_and(|version| version <= semver::Version::new(0, 12, 2))
        && entry.manifest.component.as_deref() == Some("terminal.wasm")
}

/// Archive ownership metadata once, then prevent old code from activating even if data is damaged.
fn retire_installation(runtime: &Path) -> anyhow::Result<Option<Installed>> {
    let mut registry = Manager::read_registry(runtime)?;
    let archive = runtime.join("builtin-terminal-backup/installation.json");
    let mut sources = match storage::read(&archive, 1024 * 1024)? {
        Some(bytes) => serde_json::from_slice::<BTreeMap<String, Installed>>(&bytes)?,
        None => BTreeMap::new(),
    };
    anyhow::ensure!(
        sources.values().all(retired),
        "Invalid terminal upgrade ownership archive"
    );
    let ids = registry
        .iter()
        .filter(|(_, entry)| retired(entry))
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    if !ids.is_empty() {
        if let Some(bytes) = storage::read(&runtime.join("registry.json"), 1024 * 1024)? {
            storage::backup(
                &runtime.join("builtin-terminal-backup/registry.json"),
                &bytes,
            )?;
        }
        for id in ids {
            let entry = registry.remove(&id).unwrap();
            sources.entry(id).or_insert(entry);
        }
        storage::atomic(&archive, &serde_json::to_vec(&sources)?)?;
        // Data and original installation remain in the private archive; runtime contributions disappear.
        storage::atomic(
            &runtime.join("registry.json"),
            &serde_json::to_vec_pretty(&registry)?,
        )?;
    }
    Ok(sources
        .remove("terminal")
        .or_else(|| sources.remove("me.terminal")))
}

/// Old execution preferences follow the native provider while unrelated contracts/choices are intact.
fn migrate_providers(value: &mut Value, identities: &std::collections::BTreeSet<String>) -> bool {
    let mut changed = false;
    if let Some(map) = value.as_object_mut() {
        for (key, child) in map {
            if (key == "interactive.execute" || key.ends_with("/interactive.execute"))
                && child.as_str().is_some_and(|id| identities.contains(id))
            {
                *child = Value::String("nanobug.execution".into());
                changed = true;
            } else {
                changed |= migrate_providers(child, identities);
            }
        }
    }
    changed
}

/// Rename only the retired terminal leaf; preserve split dimensions, tab order and other plugin leaves.
fn migrate_layout(
    value: &mut Value,
    adopt_visibility: bool,
    identities: &std::collections::BTreeSet<String>,
) -> bool {
    let mut changed = rename_panels(value, identities);
    let mut visibility = None;
    if let Some(map) = value
        .get_mut("plugin_panel_visibility")
        .and_then(Value::as_object_mut)
    {
        for id in ["terminal/terminal", "me.terminal/terminal"] {
            if !identities.contains(id.split_once('/').unwrap().0) {
                continue;
            }
            if let Some(visible) = map.remove(id) {
                if adopt_visibility {
                    visibility = Some(visible);
                }
                changed = true;
            }
        }
    }
    if let Some(visible) = visibility {
        value["native_terminal_visible"] = visible;
    }
    if adopt_visibility && changed && value.get("native_terminal_visible").is_none() {
        value["native_terminal_visible"] = value
            .get("terminal_visible")
            .cloned()
            .unwrap_or(Value::Bool(true));
    }
    changed
}
fn rename_panels(value: &mut Value, identities: &std::collections::BTreeSet<String>) -> bool {
    match value {
        Value::Object(map) => {
            let mut changed = false;
            for (key, value) in map {
                if key == "panel_name"
                    && matches!(
                        value.as_str(),
                        Some("plugin:terminal/terminal" | "plugin:me.terminal/terminal")
                    )
                    && value
                        .as_str()
                        .and_then(|name| name.strip_prefix("plugin:"))
                        .and_then(|name| name.split_once('/'))
                        .is_some_and(|(id, _)| identities.contains(id))
                {
                    *value = Value::String("NativeTerminal".into());
                    changed = true;
                } else {
                    changed |= rename_panels(value, identities);
                }
            }
            changed
        }
        Value::Array(values) => values.iter_mut().fold(false, |changed, value| {
            rename_panels(value, identities) || changed
        }),
        _ => false,
    }
}

#[cfg(all(test, windows))]
mod acceptance;
#[cfg(test)]
mod tests;
