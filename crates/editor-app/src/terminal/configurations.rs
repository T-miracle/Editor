//! Built-in Shell templates reuse the public configuration form and literal execution shapes.
use plugin_runtime::plugin_protocol::{
    api::{ErrorCode, Failure},
    configurations::{self as config, command_form::Fields},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Values {
    shell: String,
    fields: Fields,
}
struct Shell {
    id: &'static str,
    program: &'static str,
    args: Vec<&'static str>,
}

/// Enumerate supported interpreters for the supplied host OS, never the WASI compilation OS.
fn shells(os: &str) -> Vec<Shell> {
    match os {
        "windows" => vec![
            Shell {
                id: "PowerShell",
                program: "powershell.exe",
                args: vec!["-NoLogo", "-NoProfile", "-Command"],
            },
            Shell {
                id: "PowerShell 7",
                program: "pwsh.exe",
                args: vec!["-NoLogo", "-NoProfile", "-Command"],
            },
            Shell {
                id: "cmd",
                program: "cmd.exe",
                args: vec!["/D", "/S", "/C"],
            },
        ],
        "linux" | "macos" => ["sh", "bash", "zsh"]
            .into_iter()
            .map(|program| Shell {
                id: program,
                program,
                args: vec!["-c"],
            })
            .collect(),
        _ => vec![],
    }
}
/// Handle configuration calls without opening, replacing or borrowing an interactive terminal tab.
pub(crate) fn invoke(method: &str, arguments: &Value) -> Result<Value, Failure> {
    let os = arguments["os"].as_str().unwrap_or_default();
    let locale = arguments["locale"].as_str().unwrap_or("en");
    let workspace = arguments["workspace"].as_str().unwrap_or_default();
    let supported = shells(os);
    let payload = match method {
        "catalog" => {
            let mut templates = vec![];
            for shell in supported {
                match resolve(shell.program) {
                    Ok(_) => {
                        let mut fields = Fields::new(shell.id, shell.args.iter().map(|arg| (*arg).to_owned()).collect());
                        fields.script = Some(String::new());
                        templates.push(config::Template { id: shell.id.into(), label: shell.id.into(), group: rust_i18n::t!("terminal.shell_scripts", locale = locale).into(), icon: "terminal".into(), defaults: serde_json::to_string(&Values { shell: shell.id.into(), fields }).unwrap(), unavailable: None });
                    }
                    Err(error) if error.code == ErrorCode::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            serde_json::to_value(config::Catalog { templates })
        }
        "form" | "validate" => {
            let mut values: Values = serde_json::from_str(arguments["values"].as_str().unwrap_or_default()).map_err(|error| failure(&error.to_string()))?;
            let shell = supported.into_iter().find(|shell| shell.id == values.shell && arguments["template"] == shell.id).ok_or_else(|| failure(&rust_i18n::t!("terminal.shell_unavailable", locale = locale)))?;
            if method == "form" {
                let event = arguments["event"].as_str().unwrap_or_default();
                values.fields.edit(event)?;
                let revision = serde_json::from_str::<config::FormEvent>(event).ok().and_then(|event| if let config::FormEvent::Native(event) = event { Some(event.revision.saturating_add(1)) } else { None }).unwrap_or(0);
                let document = values.fields.document(shell.program, locale, revision);
                serde_json::to_value(config::Form { values: serde_json::to_string(&values).unwrap(), name: values.fields.name, program: shell.program.into(), document })
            } else {
                let program = resolve(shell.program)?;
                let message = values.fields.problem(locale).or_else(|| if values.fields.script.as_ref().is_none_or(|script| script.trim().is_empty()) { Some(rust_i18n::t!("terminal.script_required", locale = locale).into()) } else if arguments["intent"] == "debug" { Some(rust_i18n::t!("terminal.shell_no_debugger", locale = locale).into()) } else { None });
                let launch = if message.is_none() {
                    let mut fields = values.fields.clone();
                    fields.args.push(fields.script.clone().unwrap_or_default());
                    let mut launch = fields.launch(&program, workspace);
                    if arguments["intent"] != "run" { launch.build.push(json!({"name":values.fields.name,"target":{"kind":"action","target":launch.target}})); }
                    Some(launch)
                } else { None };
                serde_json::to_value(config::Validation { valid: message.is_none(), message: message.unwrap_or_default(), launch })
            }
        }
        _ => return Err(failure("Unknown configuration method")),
    }.map_err(|error| failure(&error.to_string()))?;
    Ok(json!({"payload":payload.to_string()}))
}
/// Read-only interpreter discovery uses the same host tool locator as controlled execution.
fn resolve(program: &str) -> Result<String, Failure> {
    plugin_runtime::native_processes::resolve_program(program)
        .map(|path| path.display().to_string())
        .map_err(|error| Failure::new(ErrorCode::NotFound, error.to_string()))
}

/// Reserved built-in identity is independent of installed package registrations.
pub(crate) const PROVIDER: &str = "$nanobug.shell";

/// Available interpreters contribute templates to the existing native configuration catalog.
pub(crate) fn templates(workspace: &str) -> Vec<(String, config::Template)> {
    invoke("catalog", &json!({"workspace":workspace,"os":std::env::consts::OS,"locale":rust_i18n::locale().to_string()}))
        .map_err(|error| error.message).and_then(config::decode::<config::Catalog>)
        .map(|catalog| catalog.templates.into_iter().map(|template| (PROVIDER.into(), template)).collect())
        .unwrap_or_default()
}

fn failure(message: &str) -> Failure {
    Failure::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// OS policy never exposes another system's templates, even when those tools happen to exist.
    #[test]
    fn shell_candidates_follow_the_host_os() {
        assert!(
            shells("windows")
                .iter()
                .all(|shell| shell.program.ends_with(".exe"))
        );
        assert!(
            shells("linux")
                .iter()
                .all(|shell| !shell.program.ends_with(".exe"))
        );
        assert_eq!(shells("unknown").len(), 0);
    }
}
