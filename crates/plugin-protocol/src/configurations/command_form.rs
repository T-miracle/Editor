//! Optional guest-side form composition. Plugins keep executable policy and may replace this layout.
use super::{FormEvent, Launch};
#[cfg(feature = "guest")]
use crate::{api, process};
use crate::{
    api::{ErrorCode, Failure},
    ui,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Ordinary command fields owned and serialized by a provider, never interpreted by the host.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fields {
    /// Provider-controlled display name, independently edited from the immutable executable.
    pub name: String,
    /// Literal argv entries, including editable subcommands, empty values and options.
    pub args: Vec<String>,
    /// Optional absolute or workspace-relative working directory; empty uses the workspace.
    #[serde(default)]
    pub directory: String,
    /// Ordered editable environment rows; validation rejects duplicate names before projection.
    #[serde(default)]
    pub env: Vec<EnvironmentEntry>,
    /// None omits the script editor. The plugin supplies interpreter switches separately.
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub expanded: bool,
    /// Deliberate list structural changes reset shifted inputs; normal echoes retain composition.
    #[serde(default)]
    pub input_revision: u64,
}
/// One provider-owned environment override, transferred without Shell expansion.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentEntry {
    /// Nonempty key without `=` or NUL; uniqueness is validated by the provider.
    pub name: String,
    /// Literal value. Empty values are allowed.
    pub value: String,
}

impl Fields {
    /// Create literal defaults without executing, resolving a tool, or persisting host state.
    pub fn new(name: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            name: name.into(),
            args,
            directory: String::new(),
            env: vec![],
            script: None,
            expanded: false,
            input_revision: 0,
        }
    }
    /// Apply a serialized form event. Each argument has its own native input, preserving empty
    /// values, whitespace and metacharacters. Malformed events return InvalidRequest unchanged.
    pub fn edit(&mut self, event: &str) -> Result<(), Failure> {
        if event.is_empty() {
            return Ok(());
        }
        let event: FormEvent = serde_json::from_str(event)
            .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
        match event {
            FormEvent::Rename { configuration_name } => {
                self.advance_revision()?;
                self.name = configuration_name;
            }
            FormEvent::Native(ui::UiEvent { node, action, .. }) => match action {
                ui::Action::Change(value) | ui::Action::Submit(value) => match node.as_str() {
                    "name" => self.name = value,
                    "directory" => self.directory = value,
                    "script" if self.script.is_some() => self.script = Some(value),
                    _ => {
                        if let Some(index) = node
                            .strip_prefix("arg-")
                            .and_then(|index| index.parse::<usize>().ok())
                        {
                            if let Some(argument) = self.args.get_mut(index) {
                                *argument = value.clone();
                            }
                        }
                        for (prefix, key) in [("env-name-", true), ("env-value-", false)] {
                            if let Some(index) = node
                                .strip_prefix(prefix)
                                .and_then(|index| index.parse::<usize>().ok())
                            {
                                if let Some(entry) = self.env.get_mut(index) {
                                    if key {
                                        entry.name = value.clone();
                                    } else {
                                        entry.value = value.clone();
                                    }
                                }
                            }
                        }
                    }
                },
                ui::Action::Click => {
                    if node == "more" {
                        self.expanded = !self.expanded;
                    } else if node == "arg-add" && self.args.len() < 128 {
                        self.args.push(String::new());
                    } else if node == "env-add" && self.env.len() < 64 {
                        self.env.push(EnvironmentEntry {
                            name: String::new(),
                            value: String::new(),
                        });
                    } else if let Some(index) = node
                        .strip_prefix("arg-remove-")
                        .and_then(|index| index.parse::<usize>().ok())
                        .filter(|index| *index < self.args.len())
                    {
                        self.advance_revision()?;
                        self.args.remove(index);
                    } else if let Some(index) = node
                        .strip_prefix("env-remove-")
                        .and_then(|index| index.parse::<usize>().ok())
                        .filter(|index| *index < self.env.len())
                    {
                        self.advance_revision()?;
                        self.env.remove(index);
                    }
                }
                _ => {}
            },
        }
        Ok(())
    }
    /// Refuse exhausted reset identities before changing values instead of wrapping onto an old input.
    fn advance_revision(&mut self) -> Result<(), Failure> {
        self.input_revision = self
            .input_revision
            .checked_add(1)
            .ok_or_else(|| Failure::new(ErrorCode::LimitExceeded, "Input revision exhausted"))?;
        Ok(())
    }
    /// Compose a native provider panel using ui.native 1.1 when script is present.
    /// Program is read-only. Locale controls labels, revision correlates events; the host owns Save.
    pub fn document(&self, program: &str, locale: &str, revision: u64) -> ui::Document {
        let zh = locale.starts_with("zh");
        let label = |zh_text: &str, en_text: &str| {
            if zh {
                zh_text.to_owned()
            } else {
                en_text.to_owned()
            }
        };
        let input = |id: String, value: &str, placeholder: &str| {
            ui::Node::input(
                id,
                ui::Input {
                    value: value.into(),
                    value_revision: self.input_revision,
                    placeholder: placeholder.into(),
                },
            )
            .grow()
        };
        let mut rows = vec![
            ui::Node::text("name-label", label("配置名称", "Name")),
            input("name".into(), &self.name, ""),
            ui::Node::text("program-label", label("命令", "Command")),
            ui::Node::text("program", program)
                .tooltip(label("命令由插件提供", "Command supplied by the plugin")),
            ui::Node::text("args-label", label("参数", "Arguments")),
        ];
        for (index, argument) in self.args.iter().enumerate() {
            rows.push(ui::Node::row(
                format!("arg-row-{index}"),
                vec![
                    input(format!("arg-{index}"), argument, ""),
                    ui::Node::button(format!("arg-remove-{index}"), "−")
                        .tooltip(label("删除参数", "Remove argument")),
                ],
            ));
        }
        rows.push(ui::Node::button(
            "arg-add",
            label("＋ 添加参数", "+ Add argument"),
        ));
        if let Some(script) = &self.script {
            rows.push(ui::Node::text("script-label", label("脚本", "Script")));
            rows.push(ui::Node::textarea(
                "script",
                ui::Input {
                    value: script.clone(),
                    value_revision: self.input_revision,
                    placeholder: label("输入脚本内容", "Enter script"),
                },
            ));
        }
        rows.push(ui::Node::button(
            "more",
            label(
                if self.expanded {
                    "⌄ 更多设置"
                } else {
                    "› 更多设置"
                },
                if self.expanded {
                    "⌄ More settings"
                } else {
                    "› More settings"
                },
            ),
        ));
        if self.expanded {
            rows.push(ui::Node::text(
                "directory-label",
                label("工作目录", "Working directory"),
            ));
            rows.push(input(
                "directory".into(),
                &self.directory,
                &label("默认工作区目录", "Workspace directory"),
            ));
            rows.push(ui::Node::text(
                "environment-label",
                label("环境变量", "Environment"),
            ));
            for (index, entry) in self.env.iter().enumerate() {
                rows.push(ui::Node::row(
                    format!("env-row-{index}"),
                    vec![
                        input(
                            format!("env-name-{index}"),
                            &entry.name,
                            &label("名称", "Name"),
                        ),
                        input(
                            format!("env-value-{index}"),
                            &entry.value,
                            &label("值", "Value"),
                        ),
                        ui::Node::button(format!("env-remove-{index}"), "−")
                            .tooltip(label("删除变量", "Remove variable")),
                    ],
                ));
            }
            rows.push(ui::Node::button(
                "env-add",
                label("＋ 添加环境变量", "+ Add variable"),
            ));
        }
        // The body owns scrolling; long argument/environment lists cannot consume window footer space.
        let mut document = ui::Document::new(
            ui::Node::scroll(
                "form-scroll",
                ui::Node::column("command-form", rows).gap(6.),
            )
            .grow(),
        );
        document.revision = revision;
        document
    }
    /// Basic editable field validation remains in the guest. Domain commands add their own rules.
    /// Returns a localized reason without changing the original values.
    pub fn problem(&self, locale: &str) -> Option<String> {
        let zh = locale.starts_with("zh");
        let message = if self.name.trim().is_empty() {
            if zh {
                "请填写配置名称"
            } else {
                "Enter a configuration name"
            }
        } else if self.name.len() > 256
            || self.name.contains('\0')
            || self.args.len() > 128
            || self
                .args
                .iter()
                .any(|arg| arg.len() > 4096 || arg.contains('\0'))
        {
            if zh {
                "名称或参数超过限制"
            } else {
                "Name or argument limit exceeded"
            }
        } else if self.env.len() > 64
            || self.env.iter().any(|entry| {
                entry.name.is_empty()
                    || entry.name.len() > 128
                    || entry.name.contains(['=', '\0'])
                    || entry.value.len() > 32768
                    || entry.value.contains('\0')
            })
            || self.env.iter().enumerate().any(|(index, entry)| {
                self.env[..index]
                    .iter()
                    .any(|previous| previous.name == entry.name)
            })
        {
            if zh {
                "请检查环境变量名称和取值"
            } else {
                "Check environment names and values"
            }
        } else if self.directory.len() > 4096 || self.directory.contains('\0') {
            if zh {
                "工作目录无效"
            } else {
                "Invalid working directory"
            }
        } else {
            return None;
        };
        Some(message.into())
    }
    /// Resolve an optional user directory relative to the supplied workspace without Shell parsing.
    pub fn directory(&self, workspace: &str) -> Option<String> {
        if self.directory.is_empty() {
            return None;
        }
        let path = &self.directory;
        Some(
            if path.starts_with(['/', '\\']) || path.as_bytes().get(1) == Some(&b':') {
                path.clone()
            } else {
                format!("{}/{path}", workspace.trim_end_matches(['/', '\\']))
            },
        )
    }
    /// Create a literal program projection; the provider must validate it before returning success.
    pub fn launch(&self, program: &str, workspace: &str) -> Launch {
        Launch {
            target: json!({"mode":"program","program":program,"args":self.args}),
            directory: self.directory(workspace),
            env: self
                .env
                .iter()
                .map(|entry| (entry.name.clone(), entry.value.clone()))
                .collect(),
            tool_paths: vec![],
            build: vec![],
            prelaunch: vec![],
            provider: None,
        }
    }
}

/// Use public process 1.6 to observe tool availability without starting it. Permission, negotiation,
/// invalid-path and missing-tool failures remain typed; a resolved path does not grant execution.
#[cfg(feature = "guest")]
pub fn resolve(program: &str) -> Result<String, Failure> {
    match api::guest::request(api::Operation::Process {
        operation: process::Operation::Resolve {
            program: program.into(),
        },
    })? {
        api::Value::ResolvedProgram { program } => Ok(program),
        _ => Err(Failure::new(
            ErrorCode::InvalidRequest,
            "Expected resolved executable",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A provider acknowledgement preserves literal argument boundaries; reset exhaustion is atomic.
    #[test]
    fn literal_edits_preserve_empty_arguments_and_exhausted_resets_preserve_values() {
        let mut fields = Fields::new(
            "Cargo",
            vec!["run".into(), String::new(), "two words".into()],
        );
        let event = ui::UiEvent {
            revision: 1,
            node: "arg-2".into(),
            action: ui::Action::Change("中文 & ; \"quotes\"".into()),
        };
        fields
            .edit(&serde_json::to_string(&event).unwrap())
            .unwrap();
        assert_eq!(fields.args, ["run", "", "中文 & ; \"quotes\""]);
        fields.input_revision = u64::MAX;
        let before = serde_json::to_string(&fields).unwrap();
        assert!(
            fields
                .edit(
                    &serde_json::to_string(&FormEvent::Rename {
                        configuration_name: "Copy".into()
                    })
                    .unwrap()
                )
                .is_err()
        );
        assert_eq!(serde_json::to_string(&fields).unwrap(), before);
    }
}
