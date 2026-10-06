//! Independent native configuration policy used through the same public SDK as external plugins.
use plugin_protocol::{
    api::{self, ErrorCode, Failure},
    bindings::{Guest, export},
    configurations as config, service, ui,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Values {
    name: String,
    program: String,
    arguments: Vec<String>,
    horizontal: bool,
    /// Verification fault modes are plugin values, never host branches or special host APIs.
    #[serde(default)]
    failure: String,
}
struct ConfigurationExample;
impl Guest for ConfigurationExample {
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |input| match input {
            // Installation and replacement use the same snapshot lifecycle as every other package.
            api::Input::Snapshot => Ok(api::Output {
                snapshot: Some(Default::default()),
                ..Default::default()
            }),
            api::Input::Event {
                event: api::Notification::Service(service::Notification::Invoke(call)),
                ..
            } => {
                // Retaining the invocation exercises the real deadline, not a synthetic timer.
                if call.method == "validate" && marker(".configuration-validation")?.as_deref() == Some("timeout") {
                    return Ok(Default::default());
                }
                Ok(api::Output {
                    service_reply: Some(invoke(call)),
                    ..Default::default()
                })
            }
            _ => Ok(Default::default()),
        })
    }
}
export!(ConfigurationExample);

/// Service replies use bounded public envelopes; programs and domain values are not interpreted by the host.
fn invoke(call: service::Invocation) -> Result<Value, Failure> {
    let result = match call.method.as_str() {
        "catalog" => {
            let root = call.arguments["workspace"].as_str().unwrap_or_default();
            let program = format!(
                "{}{separator}{}",
                root.trim_end_matches(['/', '\\']),
                if call.arguments["os"] == "windows" {
                    "probe.exe"
                } else {
                    "probe"
                },
                separator = if call.arguments["os"] == "windows" {
                    "\\"
                } else {
                    "/"
                }
            );
            let templates = [("program", false), ("compact", true)]
                .into_iter()
                .map(|(id, horizontal)| {
                    let values = Values {
                        name: format!("Example {id}"),
                        program: program.clone(),
                        arguments: vec!["default".into()],
                        horizontal,
                        failure: String::new(),
                    };
                    config::Template {
                        id: id.into(),
                        group: "Example commands".into(),
                        label: values.name.clone(),
                        icon: "code".into(),
                        defaults: serde_json::to_string(&values).unwrap(),
                        unavailable: None,
                    }
                })
                .collect();
            serde_json::to_string(&config::Catalog { templates })
        }
        "form" => {
            if marker(".configuration-form-offline")?.is_some() {
                return Err(Failure::new(ErrorCode::OperationFailed, "Example form is offline"));
            }
            let mut values: Values =
                serde_json::from_str(call.arguments["values"].as_str().unwrap_or_default())
                    .map_err(invalid)?;
            let event = call.arguments["event"]
                .as_str()
                .filter(|event| !event.is_empty())
                .map(serde_json::from_str::<config::FormEvent>)
                .transpose()
                .map_err(invalid)?;
            if let Some(event) = &event {
                match event {
                    config::FormEvent::Rename { configuration_name } => {
                        values.name = configuration_name.clone()
                    }
                    config::FormEvent::Native(event) => apply_event(&mut values, event),
                }
            }
            let document = form(
                &values,
                call.arguments["locale"].as_str().unwrap_or("en"),
                event.as_ref().map_or(0, |event| match event {
                    config::FormEvent::Native(event) => event.revision + 1,
                    _ => 0,
                }),
            );
            serde_json::to_string(&config::Form {
                values: serde_json::to_string(&values).unwrap(),
                name: values.name,
                program: values.program,
                document,
            })
        }
        "validate" => {
            let values: Values =
                serde_json::from_str(call.arguments["values"].as_str().unwrap_or_default())
                    .map_err(invalid)?;
            let environment = marker(".configuration-validation")?.unwrap_or_default();
            if values.failure == "error" || environment == "error" {
                return Err(Failure::new(
                    ErrorCode::OperationFailed,
                    "Example provider failure",
                ));
            }
            if environment == "malformed" { return Ok(json!({"payload":"{\"valid\":true}"})); }
            let valid = !values.name.trim().is_empty()
                && values.arguments.len() <= 128
                && values
                    .arguments
                    .iter()
                    .all(|value| value.len() <= 4096 && !value.contains('\0'))
                && values.failure.is_empty() && environment.is_empty();
            let launch = valid.then(|| config::Launch {
                target: json!({"mode":"program","program":values.program,"args":values.arguments}),
                directory: Some(
                    call.arguments["workspace"]
                        .as_str()
                        .unwrap_or_default()
                        .into(),
                ),
                env: Default::default(),
                tool_paths: vec![],
                build: vec![],
                prelaunch: vec![],
                provider: None,
            });
            serde_json::to_string(&config::Validation {
                valid,
                message: if valid {
                    String::new()
                } else {
                    "Example validation rejected the configuration".into()
                },
                launch,
            })
        }
        _ => {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Unknown configuration method",
            ));
        }
    }
    .map_err(invalid)?;
    Ok(json!({"payload":result}))
}

/// The plugin alone maps native field events to domain values; readonly program has no edit branch.
fn apply_event(values: &mut Values, event: &ui::UiEvent) {
    match &event.action {
        ui::Action::Change(value) if event.node == "name" => values.name = value.clone(),
        ui::Action::Change(value) if event.node.starts_with("argument-") => {
            if let Ok(index) = event.node[9..].parse::<usize>()
                && let Some(argument) = values.arguments.get_mut(index)
            {
                *argument = value.clone();
            }
        }
        ui::Action::Click if event.node == "add-argument" => values.arguments.push(String::new()),
        _ => {}
    }
}

/// Two different layouts share ordinary native inputs, including stable identities and IME handling.
fn form(values: &Values, locale: &str, revision: u64) -> ui::Document {
    let chinese = locale.starts_with("zh");
    let input = |id: String, value: String| {
        ui::Node::input(
            id,
            ui::Input {
                value,
                value_revision: 0,
                placeholder: String::new(),
            },
        )
    };
    let field = |id: &str, label: &str, node: ui::Node| {
        if values.horizontal {
            ui::Node::row(
                id,
                vec![ui::Node::text(format!("{id}-label"), label), node.grow()],
            )
        } else {
            ui::Node::column(id, vec![ui::Node::text(format!("{id}-label"), label), node])
        }
    };
    let mut nodes = vec![
        field(
            "name-field",
            if chinese {
                "配置名称"
            } else {
                "Configuration name"
            },
            input("name".into(), values.name.clone()),
        ),
        field(
            "program-field",
            if chinese { "命令" } else { "Program" },
            ui::Node::text("program", values.program.clone()),
        ),
        ui::Node::text(
            "arguments-label",
            if chinese { "参数" } else { "Arguments" },
        ),
    ];
    for (index, value) in values.arguments.iter().enumerate() {
        nodes.push(input(format!("argument-{index}"), value.clone()));
    }
    nodes.push(ui::Node::button(
        "add-argument",
        if chinese {
            "添加参数"
        } else {
            "Add argument"
        },
    ));
    ui::Document::new(
        ui::Node::scroll("form-scroll", ui::Node::column("configuration-root", nodes)).grow(),
    )
    .revision(revision)
}

fn invalid(error: impl std::fmt::Display) -> Failure {
    Failure::new(ErrorCode::InvalidRequest, error.to_string())
}

/// Fixture-local environment faults use ordinary workspace read authority and close every handle.
fn marker(path: &str) -> Result<Option<String>, Failure> {
    let api::Value::Resource(root) = api::guest::request(api::Operation::OpenWorkspace)? else { return Err(invalid("Workspace unavailable")); };
    let result = api::guest::request(api::Operation::ReadFile { handle: root.clone(), path: path.into() });
    let _ = api::guest::close_resource(root);
    match result {
        Ok(api::Value::Bytes(bytes)) => String::from_utf8(bytes).map(Some).map_err(invalid),
        Err(error) if error.code == ErrorCode::NotFound => Ok(None),
        Err(error) => Err(error),
        _ => Err(invalid("Unexpected workspace read result")),
    }
}
