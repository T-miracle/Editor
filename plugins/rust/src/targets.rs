//! Cargo discovery and artifact interpretation stay entirely in the independent Rust guest.
use plugin_protocol::{
    api::{self, ErrorCode, Failure},
    process, service, targets,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cell::RefCell;

const MAX_METADATA: usize = 4 * 1024 * 1024;
const MAX_LINE: usize = 1024 * 1024;
/// Shared bindings contain only paths relative to the workspace and explicit supported profiles.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    version: u32,
    manifest: String,
    package: String,
    bin: String,
    profile: String,
}
enum Kind {
    Discover {
        workspace: String,
    },
    Build {
        binding: Binding,
        manifest: String,
        artifact: Option<String>,
    },
}
struct Operation {
    reply: api::ResourceHandle,
    handle: api::ResourceHandle,
    kind: Kind,
    stdout: Vec<u8>,
    diagnostics: String,
}
#[derive(Default)]
struct State {
    operations: Vec<Operation>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}

/// Process notifications are consumed at full fidelity; bounded UI history never determines an artifact.
pub fn dispatch(input: api::Input) -> Result<api::Output, Failure> {
    STATE.with(|state| state.borrow_mut().dispatch(input))
}
impl State {
    fn dispatch(&mut self, input: api::Input) -> Result<api::Output, Failure> {
        let mut reply = None;
        match input {
            api::Input::Event {
                event: api::Notification::Service(service::Notification::Invoke(call)),
                ..
            } => {
                if let Err(error) = self.start(call) {
                    reply = Some(Err(error));
                }
            }
            api::Input::Event {
                event: api::Notification::Process { handle, update },
                ..
            } => {
                if let Some(index) = self
                    .operations
                    .iter()
                    .position(|operation| operation.handle == handle)
                {
                    if let Some(result) = self.update(index, update) {
                        let operation = self.operations.remove(index);
                        let _ = api::guest::close_resource(operation.handle);
                        answer(operation.reply, result);
                    }
                }
            }
            api::Input::Event {
                event:
                    api::Notification::Service(service::Notification::InvocationCancelled {
                        request,
                        ..
                    }),
                ..
            } => {
                if let Some(index) = self
                    .operations
                    .iter()
                    .position(|operation| operation.reply == request)
                {
                    // Unlike final run sessions, a preparation's explicit lifetime ends with its wait.
                    let operation = self.operations.remove(index);
                    let _ = api::guest::close_resource(operation.handle);
                }
            }
            _ => {}
        }
        // Native output belongs to the invocation's generic host preparation history; parallel
        // configurations must never share a guest-global console or presentation lifetime.
        Ok(api::Output {
            service_reply: reply,
            ..Default::default()
        })
    }
    /// Installation never executes Cargo. Explicit discovery/build calls hold their own handles.
    fn start(&mut self, call: service::Invocation) -> Result<(), Failure> {
        if self.operations.len() >= 8 {
            return Err(limit("Cargo preparation quota exceeded"));
        }
        let reply = call
            .reply
            .ok_or_else(|| failure("plugin.services 1.1 is required"))?;
        let workspace = call.arguments["workspace"]
            .as_str()
            .ok_or_else(|| failure("Missing workspace"))?
            .to_owned();
        let (kind, args) = if call.method == "discover" {
            // Absence means this provider is inapplicable, not that other providers must fail.
            let api::Value::Resource(root) = api::guest::request(api::Operation::OpenWorkspace)?
            else {
                return Err(failure("Workspace root unavailable"));
            };
            let manifest = api::guest::request(api::Operation::ReadFile {
                handle: root.clone(),
                path: "Cargo.toml".into(),
            });
            let _ = api::guest::close_resource(root);
            if matches!(manifest,Err(ref error) if error.code==ErrorCode::NotFound) {
                answer(reply, Ok(json!({"targets":[]})));
                return Ok(());
            }
            manifest?;
            (
                Kind::Discover {
                    workspace: workspace.clone(),
                },
                vec![
                    "metadata".into(),
                    "--no-deps".into(),
                    "--format-version".into(),
                    "1".into(),
                    "--offline".into(),
                ],
            )
        } else if call.method == "prepare" {
            let binding: Binding =
                serde_json::from_str(call.arguments["binding"].as_str().unwrap_or_default())
                    .map_err(|_| failure("Invalid Rust target binding; rediscover and repair"))?;
            validate_binding(&binding)?;
            let manifest = format!(
                "{}/{}",
                workspace.trim_end_matches(['/', '\\']),
                binding.manifest
            );
            let mut args = vec![
                "build".into(),
                "--manifest-path".into(),
                manifest.clone(),
                "--package".into(),
                binding.package.clone(),
                "--bin".into(),
                binding.bin.clone(),
                "--message-format=json-render-diagnostics".into(),
                "--offline".into(),
            ];
            if binding.profile == "release" {
                args.push("--release".into());
            }
            (
                Kind::Build {
                    binding,
                    manifest,
                    artifact: None,
                },
                args,
            )
        } else {
            return Err(failure("Unknown run.targets method"));
        };
        let env = call.arguments["env"]
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        Some((
                            entry["name"].as_str()?.into(),
                            entry["value"].as_str()?.into(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let api::Value::Resource(handle)=api::guest::request(api::Operation::Process {operation:process::Operation::Execute {program:"cargo".into(),args,transport:process::Transport::Stdio,cwd:Some(workspace),env}}).map_err(|error|Failure::new(error.code,format!("Cargo unavailable or preparation failed: {}. Configure a local tool path; no compiler is installed automatically.",error.message)))? else {return Err(failure("Expected Cargo process handle"));};
        self.operations.push(Operation {
            reply,
            handle,
            kind,
            stdout: Vec::new(),
            diagnostics: String::new(),
        });
        Ok(())
    }
    /// Native exit follows stdout/stderr EOF. Nonzero exit and incomplete JSON are explicit failures.
    fn update(&mut self, index: usize, update: process::Update) -> Option<Result<Value, Failure>> {
        let operation = &mut self.operations[index];
        match update {
            process::Update::Output { stream, bytes } => {
                if stream == process::Stream::Stderr {
                    append_text(
                        &mut operation.diagnostics,
                        &String::from_utf8_lossy(&bytes),
                        4096,
                    );
                    return None;
                }
                let bound = if matches!(operation.kind, Kind::Discover { .. }) {
                    MAX_METADATA
                } else {
                    MAX_LINE
                };
                if operation.stdout.len() + bytes.len() > bound {
                    return Some(Err(limit("Cargo output exceeds the declared bound")));
                }
                operation.stdout.extend(bytes);
                if let Kind::Build {
                    binding,
                    manifest,
                    artifact,
                } = &mut operation.kind
                {
                    while let Some(end) = operation.stdout.iter().position(|byte| *byte == b'\n') {
                        let line: Vec<_> = operation.stdout.drain(..=end).collect();
                        if let Err(error) = artifact_line(
                            &line,
                            binding,
                            manifest,
                            artifact,
                            &mut operation.diagnostics,
                        ) {
                            return Some(Err(error));
                        }
                    }
                }
                None
            }
            process::Update::Exited { code } => {
                if code != 0 {
                    return Some(Err(failure(&format!(
                        "Cargo exited {code}: {}",
                        operation.diagnostics
                    ))));
                }
                Some(match &mut operation.kind {
                    Kind::Discover { workspace } => candidates(&operation.stdout, workspace)
                        .map(|targets| json!({"targets":targets})),
                    Kind::Build {
                        binding,
                        manifest,
                        artifact,
                    } => {
                        if !operation.stdout.is_empty() {
                            if let Err(error) = artifact_line(
                                &operation.stdout,
                                binding,
                                manifest,
                                artifact,
                                &mut operation.diagnostics,
                            ) {
                                return Some(Err(error));
                            }
                        }
                        artifact.clone().ok_or_else(||failure("Cargo succeeded without the selected executable; rediscover or repair the target")).map(|program|json!({"program":program}))
                    }
                })
            }
            process::Update::Terminated => Some(Err(Failure::new(
                ErrorCode::Cancelled,
                "Cargo preparation stopped",
            ))),
        }
    }
}
/// Version and path bounds are enforced before any project process starts.
fn validate_binding(binding: &Binding) -> Result<(), Failure> {
    if binding.version != 1
        || !matches!(binding.profile.as_str(), "debug" | "release")
        || binding.package.is_empty()
        || binding.package.len() > 256
        || binding.package.starts_with('-')
        || binding.package.contains('\0')
        || binding.bin.is_empty()
        || binding.bin.len() > 256
        || binding.bin.starts_with('-')
        || binding.manifest.is_empty()
        || binding.manifest.len() > 4096
        || binding.manifest.starts_with(['/', '\\'])
        || binding.manifest.contains([':', '\0'])
        || binding
            .manifest
            .split(['/', '\\'])
            .any(|part| matches!(part, ".." | ""))
    {
        return Err(failure(
            "Invalid or missing Rust target; rediscover and explicitly repair it",
        ));
    }
    Ok(())
}
/// Ordinary spelling is retained in bindings; Cargo requires the literal Cargo.toml filename.
fn path_spelling(path: &str) -> String {
    let path = if let Some(tail) = path.strip_prefix(r"\\?\UNC\") {
        format!("//{tail}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).into()
    };
    path.replace('\\', "/")
}
/// Only a comparison key folds case on Windows; Linux member paths remain case sensitive.
fn normalized(path: &str) -> String {
    let path = path_spelling(path);
    if path.starts_with("//") || path.as_bytes().get(1) == Some(&b':') {
        path.to_lowercase()
    } else {
        path
    }
}
/// Cargo's workspace member table handles virtual manifests and both explicit/implicit bin targets.
fn candidates(bytes: &[u8], workspace: &str) -> Result<Vec<targets::Candidate>, Failure> {
    let metadata: Value =
        serde_json::from_slice(bytes).map_err(|_| failure("Cargo metadata is malformed"))?;
    let members = metadata["workspace_members"]
        .as_array()
        .ok_or_else(|| failure("Missing Cargo workspace members"))?;
    let root = normalized(workspace);
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let mut targets = Vec::new();
    for package in metadata["packages"]
        .as_array()
        .ok_or_else(|| failure("Missing Cargo packages"))?
    {
        if !members.iter().any(|id| id == &package["id"]) {
            continue;
        }
        let spelling = path_spelling(package["manifest_path"].as_str().unwrap_or_default());
        let path = normalized(&spelling);
        path.strip_prefix(&prefix)
            .ok_or_else(|| failure("Cargo member is outside this workspace"))?;
        // Case folding may change byte lengths (for example İ). Strip complete path components,
        // never byte counts taken from the folded comparison key.
        let root_components = prefix.trim_end_matches('/').split('/').count();
        let relative_spelling = spelling
            .split('/')
            .skip(root_components)
            .collect::<Vec<_>>()
            .join("/");
        let relative = relative_spelling.as_str();
        for target in package["targets"]
            .as_array()
            .ok_or_else(|| failure("Missing Cargo targets"))?
        {
            if !target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
            {
                continue;
            }
            let bin = target["name"]
                .as_str()
                .ok_or_else(|| failure("Missing Cargo binary name"))?;
            for profile in ["debug", "release"] {
                if targets.len() >= 128 {
                    return Err(limit("More than 128 Rust target candidates"));
                }
                let binding = Binding {
                    version: 1,
                    manifest: relative.into(),
                    package: package["name"]
                        .as_str()
                        .ok_or_else(|| failure("Missing package name"))?
                        .into(),
                    bin: bin.into(),
                    profile: profile.into(),
                };
                validate_binding(&binding)?;
                // JSON tuples distinguish delimiters in names; the host additionally namespaces providers.
                let identity = serde_json::to_string(&(relative, bin, profile)).unwrap();
                if identity.len() > 256 {
                    return Err(limit("Rust target identity exceeds 256 bytes"));
                }
                let label = format!(
                    "{} · {} · {}",
                    package["name"].as_str().unwrap_or(bin),
                    bin,
                    if profile == "debug" {
                        "Debug"
                    } else {
                        "Release"
                    }
                );
                if label.len() > 256 {
                    return Err(limit("Rust target label exceeds 256 bytes"));
                }
                targets.push(targets::Candidate {
                    identity,
                    label,
                    source: relative.into(),
                    target_type: "rust-binary".into(),
                    type_version: 1,
                    binding: serde_json::to_string(&binding).unwrap(),
                });
            }
        }
    }
    Ok(targets)
}
/// Only the chosen binary's real compiler-artifact receipt can fill the final executable.
fn artifact_line(
    line: &[u8],
    binding: &Binding,
    manifest: &str,
    artifact: &mut Option<String>,
    output: &mut String,
) -> Result<(), Failure> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(());
    }
    let message: Value =
        serde_json::from_slice(line).map_err(|_| failure("Malformed Cargo JSON build message"))?;
    if message["reason"] == "compiler-message" {
        if let Some(rendered) = message["message"]["rendered"].as_str() {
            append_text(output, rendered, 32768);
        }
    }
    // Same-named binaries in another workspace member cannot fill this exact selected package.
    if message["reason"] == "compiler-artifact"
        && normalized(message["manifest_path"].as_str().unwrap_or_default()) == normalized(manifest)
        && message["target"]["name"] == binding.bin
        && message["target"]["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        && let Some(program) = message["executable"].as_str()
    {
        if program.len() > 4096 || program.contains('\0') {
            return Err(limit("Executable path exceeds the contract"));
        }
        if artifact
            .as_deref()
            .is_some_and(|existing| existing != program)
        {
            return Err(failure("Cargo reported ambiguous executable artifacts"));
        }
        *artifact = Some(program.into());
    }
    Ok(())
}
/// Keep diagnostics visible while returning bounded results or explicit errors, never a timeout.
fn answer(handle: api::ResourceHandle, result: Result<Value, Failure>) {
    let result = if result
        .as_ref()
        .is_ok_and(|value| serde_json::to_vec(value).unwrap_or_default().len() > 60 * 1024)
    {
        Err(limit("Target discovery reply exceeds 60 KiB"))
    } else {
        result
    };
    if let Err(error) = api::guest::request(api::Operation::Service {
        operation: service::Operation::Reply {
            request: handle.clone(),
            result,
        },
    }) {
        if error.code != ErrorCode::InvalidHandle {
            let failure = Failure::new(
                ErrorCode::LimitExceeded,
                "Target response was rejected by the declared reply bounds",
            );
            if api::guest::request(api::Operation::Service {
                operation: service::Operation::Reply {
                    request: handle.clone(),
                    result: Err(failure),
                },
            })
            .is_err()
            {
                let _ = api::guest::close_resource(handle);
            }
        }
    }
}
fn append_text(target: &mut String, text: &str, max: usize) {
    target.push_str(text);
    if target.len() > max {
        let mut begin = target.len() - max;
        while !target.is_char_boundary(begin) {
            begin += 1;
        }
        target.drain(..begin);
    }
}
fn failure(message: &str) -> Failure {
    Failure::new(ErrorCode::OperationFailed, message)
}
fn limit(message: &str) -> Failure {
    Failure::new(ErrorCode::LimitExceeded, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Cargo and host spelling may differ on Windows without describing a different workspace.
    #[test]
    fn metadata_paths_match_windows_case_and_unc_prefixes() {
        let data = json!({"workspace_members":["pkg"],"packages":[{"id":"pkg","name":"app","manifest_path":"C:/WORK/member/Cargo.toml","targets":[{"kind":["bin"],"name":"app"}]}]});
        let discovered = candidates(&serde_json::to_vec(&data).unwrap(), "c:/Work").unwrap();
        assert_eq!(discovered.len(), 2);
        let binding: Value = serde_json::from_str(&discovered[0].binding).unwrap();
        assert_eq!(
            binding["manifest"], "member/Cargo.toml",
            "binding keeps Cargo's original spelling for actual execution"
        );
        assert_eq!(
            normalized(r"\\?\UNC\server\share\Work"),
            normalized(r"\\server\share\Work")
        );
    }
    /// Windows case folding can expand Unicode; relative paths must keep whole original components.
    #[test]
    fn unicode_members_preserve_literal_path_components() {
        let data = json!({"workspace_members":["pkg"],"packages":[{"id":"pkg","name":"app","manifest_path":"C:/work/İ例/Cargo.toml","targets":[{"kind":["bin"],"name":"app"}]}]});
        let discovered = candidates(&serde_json::to_vec(&data).unwrap(), "C:/work").unwrap();
        let binding: Value = serde_json::from_str(&discovered[0].binding).unwrap();
        assert_eq!(binding["manifest"], "İ例/Cargo.toml");
    }
    /// The complete label may exceed the schema even when each Cargo name and identity is legal.
    #[test]
    fn long_candidate_labels_finish_with_an_explicit_error() {
        let name = "a".repeat(140);
        let data = json!({"workspace_members":["pkg"],"packages":[{"id":"pkg","name":name,"manifest_path":"C:/work/Cargo.toml","targets":[{"kind":["bin"],"name":name}]}]});
        assert!(candidates(&serde_json::to_vec(&data).unwrap(), "C:/work").is_err());
    }
    /// Bounded output retains the latest failure rather than freezing at an earlier success.
    #[test]
    fn bounded_output_retains_the_latest_build_diagnostics() {
        let mut output = "a".repeat(32768);
        append_text(&mut output, "FINAL ERROR", 32768);
        assert!(output.ends_with("FINAL ERROR"));
        assert_eq!(output.len(), 32768);
    }
}
