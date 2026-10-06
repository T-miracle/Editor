//! Cargo command templates, editable native forms and debugger preparation are Rust guest policy.
use plugin_protocol::{
    api::{self, ErrorCode, Failure},
    configurations::{
        self as config,
        command_form::{self, Fields},
    },
    service, targets,
};
use serde_json::{Value, json};

/// Configuration service calls stay separate from target discovery but reuse owned asynchronous work.
pub(super) fn invoke(call: service::Invocation) -> Result<api::Output, Failure> {
    let args = &call.arguments;
    let workspace = args["workspace"].as_str().unwrap_or_default();
    let locale = args["locale"].as_str().unwrap_or("en");
    let payload = match call.method.as_str() {
        "catalog" => {
            let cargo = command_form::resolve("cargo");
            let available = workspace_manifest();
            let reason = cargo.err().map(|error| error.message).or_else(|| available.err().map(|error| error.message));
            let templates = [("run", "run", "play", vec!["run", "--release"]), ("build", "build", "build", vec!["build", "--release"]), ("debug", "debug", "debug", vec!["run"])]
                .into_iter().map(|(id, label, icon, argv)| config::Template { id: id.into(), group: "Cargo".into(), label: label.into(), icon: icon.into(), defaults: serde_json::to_string(&Fields::new(label, argv.into_iter().map(str::to_owned).collect())).unwrap(), unavailable: reason.clone() }).collect();
            serde_json::to_value(config::Catalog { templates })
        }
        "form" => {
            let mut fields = fields(args)?;
            fields.edit(args["event"].as_str().unwrap_or_default())?;
            let revision = event_revision(args);
            let document = fields.document("cargo", locale, revision);
            serde_json::to_value(config::Form { values: serde_json::to_string(&fields).unwrap(), name: fields.name, program: "cargo".into(), document })
        }
        "validate" => {
            let fields = fields(args)?;
            let problem = fields.problem(locale).or_else(|| if fields.args.first().is_none_or(|arg| arg.trim().is_empty()) { Some(if locale.starts_with("zh") { "请填写 Cargo 子命令" } else { "Enter a Cargo subcommand" }.into()) } else { None });
            if let Some(message) = problem { serde_json::to_value(config::Validation { valid: false, message, launch: None }) }
            else {
                workspace_manifest()?;
                let program = command_form::resolve("cargo")?;
                if args["intent"] == "debug" {
                    if !matches!(fields.args[0].as_str(), "run" | "build") { return Ok(output(config::Validation { valid: false, message: if locale.starts_with("zh") { "调试需要 run 或 build 子命令" } else { "Debug requires run or build" }.into(), launch: None })?) }
                    // Metadata holds this invocation's own native handles. No build or final process
                    // starts during Save; Debug later prepares the selected actual Cargo artifact.
                    return super::targets::begin_configuration(call, fields);
                }
                let mut launch = fields.launch(&program, workspace);
                let mut build = fields.args.clone();
                if build[0] == "run" { build[0] = "build".into(); if let Some(end) = build.iter().position(|arg| arg == "--") { build.truncate(end); } }
                if args["intent"] != "run" { launch.build.push(json!({"name":"Cargo build","target":{"kind":"action","target":{"mode":"program","program":program,"args":build}}})); }
                serde_json::to_value(config::Validation { valid: true, message: String::new(), launch: Some(launch) })
            }
        }
        _ => return Err(failure("Unknown configuration method")),
    }.map_err(|error| failure(&error.to_string()))?;
    Ok(api::Output {
        service_reply: Some(Ok(json!({"payload":payload.to_string()}))),
        ..Default::default()
    })
}

/// Decode only this plugin's own canonical fields; unknown template identities never execute.
fn fields(args: &Value) -> Result<Fields, Failure> {
    if !matches!(args["template"].as_str(), Some("run" | "build" | "debug")) {
        return Err(failure("Unknown Cargo template"));
    }
    serde_json::from_str(args["values"].as_str().unwrap_or_default())
        .map_err(|error| failure(&error.to_string()))
}
/// Capability-checked root lookup distinguishes non-Cargo projects from inaccessible workspaces.
fn workspace_manifest() -> Result<(), Failure> {
    let root = api::guest::open_workspace()?;
    let result = api::guest::request(api::Operation::ReadFile {
        handle: root.clone(),
        path: "Cargo.toml".into(),
    });
    let _ = api::guest::close_resource(root);
    result.map(|_| ()).map_err(|error| {
        Failure::new(
            error.code,
            if error.code == ErrorCode::NotFound {
                "此工作区没有 Cargo.toml / No Cargo.toml in this workspace".into()
            } else {
                error.message
            },
        )
    })
}
/// Native scene revisions correlate events, independently from values reset/IME revisions.
fn event_revision(args: &Value) -> u64 {
    serde_json::from_str::<config::FormEvent>(args["event"].as_str().unwrap_or_default())
        .ok()
        .and_then(|event| {
            if let config::FormEvent::Native(event) = event {
                Some(event.revision.saturating_add(1))
            } else {
                None
            }
        })
        .unwrap_or(0)
}
fn output(validation: config::Validation) -> Result<api::Output, Failure> {
    Ok(api::Output {
        service_reply: Some(Ok(
            json!({"payload":serde_json::to_string(&validation).map_err(|error| failure(&error.to_string()))?}),
        )),
        ..Default::default()
    })
}
fn failure(message: &str) -> Failure {
    Failure::new(ErrorCode::InvalidRequest, message)
}

/// Convert Cargo's real metadata to one explicit artifact binding. Multiple binaries require --bin;
/// package/profile/manifest flags remain plugin-owned and literal runtime arguments follow --.
pub(super) fn debug_validation(
    bytes: &[u8],
    fields: &Fields,
    workspace: &str,
    locale: &str,
) -> Result<config::Validation, Failure> {
    // Runtime arguments after `--` cannot change Cargo package, profile or artifact selection.
    let split = fields
        .args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(fields.args.len());
    let cargo = &fields.args[..split];
    let option = |long: &str, short: &str| {
        cargo.iter().enumerate().find_map(|(index, arg)| {
            if arg == long || (!short.is_empty() && arg == short) {
                cargo.get(index + 1).cloned()
            } else {
                arg.strip_prefix(&format!("{long}=")).map(str::to_owned)
            }
        })
    };
    let requested_profile = option("--profile", "");
    if requested_profile
        .as_deref()
        .is_some_and(|profile| !matches!(profile, "dev" | "release"))
    {
        return Ok(config::Validation {
            valid: false,
            message: if locale.starts_with("zh") {
                "调试当前支持 dev 和 release 构建配置"
            } else {
                "Debug currently supports dev and release profiles"
            }
            .into(),
            launch: None,
        });
    }
    let profile = if cargo.iter().any(|arg| arg == "--release" || arg == "-r")
        || requested_profile.as_deref() == Some("release")
    {
        "release"
    } else {
        "debug"
    };
    let package = option("--package", "-p");
    let binary = option("--bin", "");
    let candidates = super::targets::candidates(bytes, workspace)?;
    let mut selected = candidates
        .into_iter()
        .filter(|candidate| {
            let binding: Value = serde_json::from_str(&candidate.binding).unwrap_or(Value::Null);
            binding["profile"] == profile
                && package
                    .as_ref()
                    .is_none_or(|package| binding["package"] == *package)
                && binary
                    .as_ref()
                    .is_none_or(|binary| binding["bin"] == *binary)
        })
        .collect::<Vec<targets::Candidate>>();
    if selected.len() != 1 {
        return Ok(config::Validation {
            valid: false,
            message: if locale.starts_with("zh") {
                "请使用 --package 和 --bin 选择一个可调试程序"
            } else {
                "Select one debuggable program with --package and --bin"
            }
            .into(),
            launch: None,
        });
    }
    let target = selected.remove(0);
    let mut build = fields.args[..split].to_vec();
    build[0] = "build".into();
    // Structured diagnostics are mandatory; user display format cannot suppress artifact receipts.
    build.retain(|arg| !arg.starts_with("--message-format="));
    if let Some(index) = build.iter().position(|arg| arg == "--message-format") {
        build.drain(index..(index + 2).min(build.len()));
    }
    build.push("--message-format=json-render-diagnostics".into());
    if binary.is_none() {
        build.extend([
            "--bin".into(),
            serde_json::from_str::<Value>(&target.binding).unwrap()["bin"]
                .as_str()
                .unwrap()
                .into(),
        ]);
    }
    let mut binding: Value =
        serde_json::from_str(&target.binding).map_err(|error| failure(&error.to_string()))?;
    binding["build_args"] = json!(build);
    binding["directory"] = json!(fields.directory(workspace));
    let binding = binding.to_string();
    if binding.len() > 4096 {
        return Err(failure("Cargo debug binding exceeds its byte budget"));
    }
    let mut launch = fields.launch("cargo", workspace);
    launch.target = json!({"mode":"provided","provider":"$self","binding":binding,"label":target.label,"args":if split < fields.args.len() { fields.args[split + 1..].to_vec() } else { vec![] }});
    // A provided target is a placeholder until its matching public preparation action supplies
    // an exact executable receipt. Debug must never pass the display label to the adapter.
    launch.build.push(
        json!({"name":"Cargo debug build","target":{"kind":"action","target":launch.target}}),
    );
    Ok(config::Validation {
        valid: true,
        message: String::new(),
        launch: Some(launch),
    })
}
