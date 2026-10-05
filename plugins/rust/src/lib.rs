//! Rust Analyzer policy lives in this independent guest; the host only executes generic language plans.
mod targets;
use plugin_protocol::{
    api::{self, ErrorCode, Failure},
    bindings::{Guest, export},
    language,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

struct RustLanguage;
impl Guest for RustLanguage {
    /// The SDK correlates lifecycle replies and typed host operations without exposing host implementation paths.
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |input| match input {
            // Dynamic target policy shares this guest without moving Cargo parsing into the host.
            input @ api::Input::Event {
                event: api::Notification::Service(_) | api::Notification::Process { .. },
                ..
            } => targets::dispatch(input),
            api::Input::Event {
                event: api::Notification::LanguageService(context),
                ..
            } => Ok(api::Output {
                language_service: Some(prepare_language(context)?),
                ..Default::default()
            }),
            // This stateless policy guest owns no private data, views, processes or subscriptions.
            api::Input::Snapshot => Ok(api::Output {
                snapshot: Some(Default::default()),
                ..Default::default()
            }),
            _ => Ok(Default::default()),
        })
    }
}
export!(RustLanguage);

/// Discover only paired plugin manifests plus the workspace root, preserving ignored-tree exclusions.
fn prepare_language(context: language::Context) -> Result<language::Proposal, Failure> {
    let sdk = match api::guest::request(api::Operation::DescribeSdk)? {
        api::Value::Sdk(sdk) => sdk,
        _ => {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Expected host SDK descriptor",
            ));
        }
    };
    let workspace = api::guest::open_workspace()?;
    let result = api::guest::request(api::Operation::FindFiles {
        handle: workspace.clone(),
        query: api::FileQuery {
            include: [
                "Cargo.toml",
                "manifest.json",
                "**/Cargo.toml",
                "**/manifest.json",
            ]
            .map(str::to_owned)
            .to_vec(),
            exclude: [
                "**/target/**",
                "**/vendor/**",
                "**/node_modules/**",
                "**/.git/**",
            ]
            .map(str::to_owned)
            .to_vec(),
            max_results: 4096,
        },
    });
    // Discovery handles never survive the pure preparation callback, including host request failures.
    let _ = api::guest::close_resource(workspace);
    let matches = match result? {
        api::Value::Files(files) => files,
        _ => {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Expected workspace file matches",
            ));
        }
    };
    // Unreadable unrelated directories are reported by the host but do not disable all language analysis.
    let options = initialization_options(&context.workspace, &sdk.cargo_config, &matches.paths);
    Ok(language::Proposal {
        configuration: Some(configuration_sections(&options)),
        initialization_options: Some(options),
        ..Default::default()
    })
}

/// Match Rust Analyzer's URI/VFS spelling even though the guest itself runs under WASI rather than Windows.
fn analysis_path(path: &str) -> String {
    let mut path = path.replace('\\', "/");
    if let Some(unc) = path.strip_prefix("//?/UNC/") {
        path = format!("//{unc}");
    } else if let Some(ordinary) = path.strip_prefix("//?/") {
        path = ordinary.to_owned();
    }
    if path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic() {
        path[..1].make_ascii_lowercase();
    }
    path
}

/// Root Cargo workspaces and paired independent plugin crates become linked projects without editing their files.
fn initialization_options(workspace: &str, cargo_config: &str, matches: &[String]) -> Value {
    let paths: BTreeSet<_> = matches.iter().map(String::as_str).collect();
    let mut projects = BTreeSet::new();
    for path in &paths {
        let Some(directory) = path.strip_suffix("Cargo.toml") else {
            continue;
        };
        // A same-directory manifest marks an independent plugin; arbitrary nested Cargo projects are omitted.
        if directory.is_empty() || paths.contains(format!("{directory}manifest.json").as_str()) {
            projects.insert(analysis_path(&format!(
                "{}/{}",
                workspace.trim_end_matches(['/', '\\']),
                path
            )));
        }
    }
    let mut options = json!({"cargo":{"configPath":analysis_path(cargo_config)},
        "diagnostics":{"enable":true,"experimental":{"enable":true}}});
    if !projects.is_empty() {
        options["linkedProjects"] = json!(projects);
    }
    options
}

/// Publish complete and dotted Rust Analyzer sections as opaque generic configuration keys.
fn configuration_sections(options: &Value) -> BTreeMap<String, Value> {
    fn visit(prefix: &str, value: &Value, sections: &mut BTreeMap<String, Value>) {
        sections.insert(prefix.to_owned(), value.clone());
        if let Some(fields) = value.as_object() {
            for (key, value) in fields {
                visit(&format!("{prefix}.{key}"), value, sections);
            }
        }
    }
    let mut sections = BTreeMap::from([(String::new(), options.clone())]);
    visit("rust-analyzer", options, &mut sections);
    sections
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Project pairing and Windows spelling preserve unsaved analysis for excluded standalone guests.
    #[test]
    fn paired_projects_and_configuration_remain_guest_policy() {
        let files = [
            "Cargo.toml",
            "plugins/demo/Cargo.toml",
            "plugins/demo/manifest.json",
            "unrelated/Cargo.toml",
        ]
        .map(str::to_owned);
        let options =
            initialization_options(r"\\?\C:\Projects\demo", r"\\?\C:\Cache\sdk.toml", &files);
        assert_eq!(
            options["linkedProjects"],
            json!([
                "c:/Projects/demo/Cargo.toml",
                "c:/Projects/demo/plugins/demo/Cargo.toml"
            ])
        );
        let sections = configuration_sections(&options);
        assert_eq!(
            sections["rust-analyzer.cargo.configPath"],
            "c:/Cache/sdk.toml"
        );
        assert_eq!(
            sections["rust-analyzer.diagnostics.experimental.enable"],
            true
        );
        assert_eq!(sections[""], options);
        assert_eq!(sections["rust-analyzer"], options);
    }
    /// A UNC root stays absolute and a non-Cargo folder never gets a synthetic linked project.
    #[test]
    fn unc_and_empty_discovery_are_preserved() {
        assert_eq!(
            analysis_path(r"\\?\UNC\server\share\Cargo.toml"),
            "//server/share/Cargo.toml"
        );
        assert!(
            initialization_options("/workspace", "/sdk/config.toml", &[])
                .get("linkedProjects")
                .is_none()
        );
    }
}
