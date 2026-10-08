//! Two explicitly host-owned templates reuse the existing native form and draft transactions.
use plugin_runtime::plugin_protocol::{configurations as contract, ui};
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Reserved host identity is not an installed plugin ID and cannot be contributed by a package.
pub(crate) const PROVIDER: &str = "$nanobug.plugin-development";
/// Editable local overrides; shared asset/build rules remain in each project's description.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Values {
    pub name: String,
    pub projects: String,
    pub output: String,
    pub workspace: String,
    pub watch: bool,
    /// Only picker replacements advance input epochs; ordinary typing retains composition/selection.
    #[serde(default)]
    pub picker_versions: std::collections::BTreeMap<String, u64>,
}
impl Values {
    pub fn projects(&self, workspace: &str) -> Vec<PathBuf> {
        self.projects
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let path = PathBuf::from(line.trim());
                if path.is_absolute() {
                    path
                } else {
                    PathBuf::from(workspace).join(path)
                }
            })
            .collect()
    }
    /// Prepare literal argv for the shared CLI. No string is ever passed to a shell.
    pub fn args(
        &self,
        template: &str,
        workspace: &str,
        build_only: bool,
        id: &str,
    ) -> anyhow::Result<Vec<String>> {
        let mut args = vec![
            if build_only {
                "--plugin-build"
            } else if template == "package" {
                "--plugin-package"
            } else {
                "--plugin-dev"
            }
            .into(),
        ];
        for project in self.projects(workspace) {
            args.push(project.display().to_string());
        }
        if !self.output.is_empty() {
            let output = PathBuf::from(&self.output);
            let output = if output.is_absolute() {
                output
            } else {
                PathBuf::from(workspace).join(output)
            };
            args.extend(["--output".into(), output.display().to_string()]);
        }
        if template == "development" {
            let root = super::profile(workspace, id)?;
            let target = if self.workspace.is_empty() {
                self.projects(workspace)
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Missing project"))?
            } else {
                let path = PathBuf::from(&self.workspace);
                if path.is_absolute() {
                    path
                } else {
                    PathBuf::from(workspace).join(path)
                }
            };
            args.extend([
                "--profile".into(),
                root.display().to_string(),
                "--workspace".into(),
                target.display().to_string(),
            ]);
            if self.watch {
                args.push("--watch".into());
            }
        } else {
            args.push("--release".into());
        }
        Ok(args)
    }
}
/// Host templates stay present even with every installed plugin disabled.
pub(crate) fn templates(workspace: &str) -> Vec<(String, contract::Template)> {
    ["package", "development"]
        .into_iter()
        .map(|id| {
            let name = if id == "package" {
                t!("plugin_dev.package")
            } else {
                t!("plugin_dev.development")
            }
            .to_string();
            let values = Values {
                name: name.clone(),
                projects: workspace.into(),
                output: String::new(),
                workspace: String::new(),
                watch: false,
                picker_versions: Default::default(),
            };
            (
                PROVIDER.into(),
                contract::Template {
                    id: id.into(),
                    group: "Nanobug".into(),
                    label: name,
                    icon: if id == "package" { "build" } else { "debug" }.into(),
                    defaults: serde_json::to_string(&values).unwrap(),
                    unavailable: None,
                },
            )
        })
        .collect()
}
/// Compose/update a host form without involving a plugin instance or pretending to have its origin.
pub(crate) fn form(arguments: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut values: Values =
        serde_json::from_str(arguments["values"].as_str().ok_or("Missing values")?)
            .map_err(|error| error.to_string())?;
    let event = arguments["event"].as_str().unwrap_or_default();
    if !event.is_empty() {
        match serde_json::from_str::<contract::FormEvent>(event)
            .map_err(|error| error.to_string())?
        {
            contract::FormEvent::Rename { configuration_name } => values.name = configuration_name,
            contract::FormEvent::Native(ui::UiEvent { node, action, .. }) => match action {
                ui::Action::Change(value) | ui::Action::Submit(value) => {
                    let field = node.strip_prefix("picked-").unwrap_or(&node);
                    if node.starts_with("picked-") {
                        let revision = values.picker_versions.entry(field.into()).or_default();
                        *revision = revision
                            .checked_add(1)
                            .ok_or("Input replacement revision exhausted")?;
                    }
                    match field {
                        "name" => values.name = value,
                        "projects" => values.projects = value,
                        "output" => values.output = value,
                        "workspace" => values.workspace = value,
                        _ => {}
                    }
                }
                ui::Action::Toggle(value) if node == "watch" => values.watch = value,
                _ => {}
            },
        }
    }
    let input = |id: &str, value: &str| {
        ui::Node::input(
            id,
            ui::Input {
                value: value.into(),
                value_revision: values.picker_versions.get(id).copied().unwrap_or_default(),
                placeholder: String::new(),
            },
        )
    };
    let mut rows = vec![
        ui::Node::text("name-label", t!("plugin_dev.name")),
        input("name", &values.name),
        ui::Node::text("command-label", t!("plugin_dev.command")),
        ui::Node::text("command", "Nanobug"),
        ui::Node::text("projects-label", t!("plugin_dev.projects")),
        ui::Node::textarea(
            "projects",
            ui::Input {
                value: values.projects.clone(),
                value_revision: values
                    .picker_versions
                    .get("projects")
                    .copied()
                    .unwrap_or_default(),
                placeholder: t!("plugin_dev.projects_hint").into(),
            },
        ),
        ui::Node::button("projects-browse", t!("plugin_dev.choose_projects")),
    ];
    if arguments["template"] == "package" {
        rows.extend([
            ui::Node::text("output-label", t!("plugin_dev.output")),
            input("output", &values.output),
            ui::Node::text("output-hint", t!("plugin_dev.output_hint")),
            ui::Node::button("output-browse", t!("plugin_dev.choose_output")),
        ]);
    } else {
        rows.extend([
            ui::Node::text("workspace-label", t!("plugin_dev.workspace")),
            input("workspace", &values.workspace),
            ui::Node::button("workspace-browse", t!("plugin_dev.choose_workspace")),
            ui::Node::checkbox("watch", t!("plugin_dev.watch"), values.watch),
            ui::Node::button("reset-profile", t!("plugin_dev.reset")),
            ui::Node::text("development-hint", t!("plugin_dev.development_hint")),
        ]);
    }
    // The root scroller needs a bounded flex height in the native dialog; an auto-height
    // wrapper around percentage-height scroll content otherwise collapses to zero.
    let document = ui::Document::new(
        ui::Node::scroll(
            "plugin-development-form",
            ui::Node::column("fields", rows).gap(6.),
        )
        .grow(),
    );
    let reply = contract::Form {
        values: serde_json::to_string(&values).unwrap(),
        name: values.name,
        program: "Nanobug".into(),
        document,
    };
    Ok(serde_json::json!({"payload":serde_json::to_string(&reply).unwrap()}))
}
/// Validation reads bounded descriptions only. Execution repeats validation and enforces trust.
pub(crate) fn validate(arguments: &serde_json::Value) -> Result<serde_json::Value, String> {
    let result = (|| -> anyhow::Result<contract::Validation> {
        let values: Values = serde_json::from_str(
            arguments["values"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing values"))?,
        )?;
        let workspace = arguments["workspace"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing workspace"))?;
        let template = arguments["template"].as_str().unwrap_or("");
        anyhow::ensure!(
            ["package", "development"].contains(&template),
            "Unknown host template"
        );
        let projects = values.projects(workspace);
        anyhow::ensure!(
            !values.name.trim().is_empty() && !projects.is_empty() && projects.len() <= 64,
            "Name and 1–64 plugin projects are required"
        );
        anyhow::ensure!(
            template != "development" || projects.len() == 1,
            "A development instance runs one plugin project"
        );
        for project in &projects {
            if let Err(error) = plugin_runtime::development::Project::read(project) {
                // Missing descriptions mean the selected checkout is not ready for this template,
                // not that a compiler failed. Retain real I/O causes for every other failure.
                if let Some(description) = error
                    .downcast_ref::<plugin_runtime::development::DescriptionReadError>()
                    .filter(|description| description.source.kind() == std::io::ErrorKind::NotFound)
                {
                    anyhow::bail!(
                        "{}",
                        t!(
                            "plugin_dev.missing_description",
                            path = description.path.display()
                        )
                    );
                }
                return Err(error.context(
                    t!("plugin_dev.project_read_failed", path = project.display()).to_string(),
                ));
            }
        }
        if template == "development" && !values.workspace.is_empty() {
            let path = PathBuf::from(&values.workspace);
            anyhow::ensure!(
                (if path.is_absolute() {
                    path
                } else {
                    PathBuf::from(workspace).join(path)
                })
                .is_dir(),
                "Test workspace does not exist"
            );
        }
        anyhow::ensure!(
            arguments["intent"] != "debug",
            "This template provides development runs, not WASM source debugging"
        );
        Ok(contract::Validation {
            valid: true,
            message: String::new(),
            launch: Some(contract::Launch {
                target: serde_json::json!({"mode":"program","program":"Nanobug","args":[]}),
                directory: None,
                env: Default::default(),
                tool_paths: vec![],
                build: vec![
                    serde_json::json!({"name":"Plugin build","target":{"kind":"action","target":{"mode":"program","program":"Nanobug","args":[]}}}),
                ],
                prelaunch: vec![],
                provider: None,
            }),
        })
    })();
    let validation = result.unwrap_or_else(|error| contract::Validation {
        valid: false,
        message: format!("{error:#}"),
        launch: None,
    });
    Ok(serde_json::json!({"payload":serde_json::to_string(&validation).unwrap()}))
}
