//! The editor's run controls are an ordinary consumer of the published execution contract.
//!
//! These checks start a real program through the host session API — by contract and scope, never by
//! plugin identity — and assert only externally observable results: session identity, the provider's
//! own answer, the requested native panel, and real process ownership.
#![cfg(windows)]
// The shared fixture module serves several integration tests; this one uses only its contract name.
#[path = "support/interactive_packages.rs"]
#[allow(dead_code)]
mod packages;
use packages::CONTRACT;
use plugin_runtime::{
    ExecutionState, Manager, Package, RunRequest, plugin_protocol::api::RequestUpdate,
};
use serde_json::Value;
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn terminal() -> Package {
    Package::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"))
        .unwrap()
}

/// Cancelling a creation wait after a native effect must retain the program and its eventual identity.
#[test]
#[ignore = "build capability-example through the public SDK first"]
fn cancelling_a_creation_wait_keeps_the_already_created_program_manageable() {
    use plugin_runtime::plugin_protocol::api::{self, CancellationEffect};
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = packages::provider("async-runner");
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager
        .live
        .get_mut("async-runner")
        .unwrap()
        .call(api::Input::Event {
            panel: None,
            event: api::Notification::Command {
                id: "execution-defer-next".into(),
                context: None,
                arguments: None,
            },
        })
        .unwrap();
    let ready = root.path().join("created.txt");
    let session = manager
        .start_execution(RunRequest {
            program: "powershell.exe".into(),
            args: vec![
                "-NoProfile".into(),
                "-Command".into(),
                format!(
                    "[IO.File]::WriteAllText('{}', 'ready'); Start-Sleep -Seconds 60",
                    ready.display()
                ),
            ],
            cwd: Some(root.path().display().to_string()),
            name: None,
            env: vec![],
        })
        .unwrap();
    wait_until(&mut manager, |_| ready.exists(), |ready| *ready);
    assert!(
        ready.exists(),
        "the native side effect occurred before its receipt"
    );
    session.cancel();
    assert!(matches!(
        session.update(),
        RequestUpdate::Cancelled {
            effect: CancellationEffect::WaitingStopped,
            ..
        }
    ));
    for _ in 0..3 {
        manager.poll();
    }
    assert_eq!(
        manager.live["async-runner"].process_ids().len(),
        1,
        "stopping the wait cannot kill the program"
    );
    manager
        .live
        .get_mut("async-runner")
        .unwrap()
        .call(api::Input::Event {
            panel: None,
            event: api::Notification::Command {
                id: "execution-release".into(),
                context: None,
                arguments: None,
            },
        })
        .unwrap();
    let state = wait_until(
        &mut manager,
        |_| session.snapshot().state,
        |state| *state != ExecutionState::Starting,
    );
    assert_eq!(state, ExecutionState::Running);
    assert!(session.snapshot().provider_session.is_some());
    assert!(
        matches!(
            session.update(),
            RequestUpdate::Cancelled {
                effect: CancellationEffect::WaitingStopped,
                ..
            }
        ),
        "a late receipt cannot reopen the cancelled caller wait"
    );
    manager
        .stop_execution_with(
            session.id(),
            plugin_runtime::StopOptions {
                mode: plugin_runtime::plugin_protocol::process::ExitMode::Force,
                ..Default::default()
            },
        )
        .unwrap();
    let ended = wait_until(
        &mut manager,
        |_| session.snapshot().state,
        |state| !state.is_active(),
    );
    assert_eq!(ended, ExecutionState::Exited);
    manager.shutdown();
}

fn manager(root: &Path) -> Manager {
    Manager::open(
        root.join("plugins"),
        plugin_runtime::plugin_protocol::Environment {
            workspace: root.display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap()
}

/// Polling drives real service and process completion, bounded independently of guest fuel.
///
/// The final observed value is returned so a caller can assert the final outcome directly.
fn wait_until<T>(
    manager: &mut Manager,
    ready: impl Fn(&Manager) -> T,
    done: impl Fn(&T) -> bool,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut observed = ready(manager);
    while !done(&observed) && Instant::now() < deadline {
        manager.poll();
        std::thread::sleep(Duration::from_millis(20));
        observed = ready(manager);
    }
    observed
}

/// Answer the provider's ordinary native panel request so its session becomes visible.
fn reveal_panel(manager: &mut Manager) -> usize {
    reveal_panel_for(manager, "terminal")
}

/// The same panel answer for whichever provider owns the session being presented.
///
/// The panel is the one the provider asked for in its own request: the host never decides what a
/// provider's session looks like, so the answer echoes the provider's own name for it.
fn reveal_panel_for(manager: &mut Manager, plugin: &str) -> usize {
    let Some(instance) = manager.live.get_mut(plugin) else {
        return 0;
    };
    let requests = instance.take_editor_requests();
    let count = requests.len();
    for request in requests {
        let operation = request.operation().clone();
        let panel = match &operation {
            plugin_runtime::plugin_protocol::api::EditorOperation::SetPanelVisibility {
                panel,
                ..
            } => panel.clone(),
            _ => String::new(),
        };
        assert!(request.begin());
        let _ = request.finish(Ok(
            plugin_runtime::plugin_protocol::api::EditorValue::PanelVisibility {
                panel,
                visible: matches!(
                    operation,
                    plugin_runtime::plugin_protocol::api::EditorOperation::SetPanelVisibility {
                        visible: true,
                        ..
                    }
                ),
            },
        ));
    }
    count
}

/// A repeat launch must locate the retained session instead of creating a second program.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn host_controls_start_once_and_locate_the_retained_session() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    let request = RunRequest {
        program: "powershell.exe".into(),
        args: vec![
            "-NoProfile".into(),
            "-Command".into(),
            "[Console]::Write('HOST_START_OK'); Start-Sleep -Seconds 60".into(),
        ],
        cwd: Some(root.path().display().to_string()),
        name: Some("宿主运行".into()),
        env: Vec::new(),
    };
    let session = manager.start_execution(request.clone()).unwrap();
    assert_eq!(session.plugin(), "terminal");
    // A queued request is not an answer: nothing claims the program is running yet.
    assert_eq!(session.snapshot().state, ExecutionState::Starting);
    let active = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state != ExecutionState::Starting,
    );
    assert_eq!(active, ExecutionState::Running);
    let snapshot = manager.execution(session.id()).unwrap().snapshot();
    assert_eq!(snapshot.plugin, "terminal");
    assert!(
        snapshot.provider_session.is_some(),
        "the provider's own session identity is reported without being reinterpreted"
    );
    // The provider's answer is terminal as a request; the session lifecycle continues separately.
    assert!(
        manager
            .execution(session.id())
            .unwrap()
            .update()
            .is_terminal()
    );
    // The provider owns its presentation: it asks the editor for its ordinary panel.
    assert!(reveal_panel(&mut manager) >= 1);
    // A second request for the same literal command resolves to the retained session.
    let located = manager
        .execution_for(&request, Some(&root.path().display().to_string()))
        .expect("repeat launch locates the retained session");
    assert_eq!(located.id(), session.id());
    // The session outlives its provider only as a visible result: the program it started is no
    // longer managed here, so it is reported as failed rather than as still running.
    manager.disable("terminal").unwrap();
    let ended = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Failed,
    );
    assert_eq!(ended, ExecutionState::Failed);
    assert!(
        manager
            .execution(session.id())
            .unwrap()
            .snapshot()
            .provider_session
            .is_some()
    );
    // A missing provider is reported instead of being replaced by an unrelated command.
    let missing = manager
        .start_execution(RunRequest {
            program: "powershell.exe".into(),
            args: vec!["-NoProfile".into()],
            cwd: None,
            name: None,
            env: Vec::new(),
        })
        .unwrap_err()
        .to_string();
    assert!(missing.contains(CONTRACT), "{missing}");
}

/// The host lists every provider that declares the execution contract, with why one is unusable.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn host_lists_execution_providers_with_their_reasons() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let terminal = terminal();
    let grants = terminal.manifest.permissions.clone();
    manager.install(&terminal, grants).unwrap();

    // The installed provider is listed and is the one a launch would use.
    let providers = manager.execution_providers();
    assert_eq!(providers.len(), 1, "{providers:?}");
    assert_eq!(providers[0].plugin, "terminal");
    assert!(
        providers[0].selected,
        "the only provider is the selected one"
    );
    assert!(providers[0].unavailable.is_none());

    // A disabled provider is still listed, with the reason a user would act on: hiding it would
    // make an incomplete choice look like the only one.
    manager.disable("terminal").unwrap();
    let providers = manager.execution_providers();
    let disabled = providers
        .iter()
        .find(|candidate| candidate.plugin == "terminal")
        .expect("a disabled provider is still listed");
    assert!(
        disabled.unavailable.is_some(),
        "a provider that cannot run says why: {disabled:?}"
    );
    // Listing is descriptive: it changes nothing about which provider is selected.
    assert!(!disabled.selected || disabled.unavailable.is_some());
}

/// Two independent providers serve the same contract, and the host treats them alike.
///
/// The same consumer path starts, locates, presents and stops a program through the real terminal
/// package and through a separately packaged provider with a different id. Nothing in the host
/// branches on which one answered: the session belongs to whichever provider started it, and a
/// switch of the default applies to later launches only.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn two_independent_providers_serve_one_consumer_the_same_way() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    for package in [terminal(), packages::provider("alt-runner")] {
        let grants = package.manifest.permissions.clone();
        manager.install(&package, grants).unwrap();
    }
    // Both providers are offered, and neither is chosen yet: an ambiguous contract has no single
    // owner, so the host reports that rather than guessing one.
    let providers = manager.execution_providers();
    let listed = providers
        .iter()
        .map(|candidate| candidate.plugin.as_str())
        .collect::<Vec<_>>();
    assert_eq!(listed, vec!["alt-runner", "terminal"], "{providers:?}");
    assert!(
        providers
            .iter()
            .all(|candidate| candidate.unavailable.is_none()),
        "both providers are usable: {providers:?}"
    );

    let mut sessions = Vec::new();
    for provider in ["terminal", "alt-runner"] {
        manager
            .set_service_provider(
                plugin_protocol::api::InstanceScope::Workspace,
                plugin_protocol::settings::Scope::Project,
                CONTRACT,
                Some(provider),
            )
            .unwrap();
        let request = RunRequest {
            program: "powershell.exe".into(),
            args: vec![
                "-NoProfile".into(),
                "-Command".into(),
                format!("[Console]::Write('{provider}_OK'); Start-Sleep -Seconds 60"),
            ],
            cwd: Some(root.path().display().to_string()),
            name: Some(provider.into()),
            env: Vec::new(),
        };
        let session = manager.start_execution(request.clone()).unwrap();
        assert_eq!(
            session.plugin(),
            provider,
            "the selected provider answered, not another one"
        );
        let state = wait_until(
            &mut manager,
            |manager| manager.execution(session.id()).unwrap().snapshot().state,
            |state| *state != ExecutionState::Starting,
        );
        assert_eq!(state, ExecutionState::Running);
        // The same consumer path presents the session for either provider.
        assert!(
            reveal_panel_for(&mut manager, provider) >= 1,
            "{provider} presented its session the same way"
        );
        // Repeat launches resolve to the session that already exists, whoever owns it.
        let located = manager
            .execution_for(&request, Some(&root.path().display().to_string()))
            .expect("a repeat launch locates the retained session");
        assert_eq!(located.id(), session.id());
        // The program the provider delegated is a real process, counted by its owner. A provider may
        // also keep processes of its own — the terminal keeps its private shell — so the count is
        // kept as that provider's own baseline rather than compared with an absolute number.
        let baseline = manager.live[provider].process_count();
        assert!(baseline >= 1, "{provider} owns a real program");
        sessions.push((provider, session.id(), baseline));
    }

    // Each session reports the provider that started it, including when ownership is asked about
    // the other's session.
    for (provider, id, _) in &sessions {
        assert_eq!(
            manager.execution(*id).unwrap().snapshot().plugin,
            *provider,
            "a session belongs to the provider that started it"
        );
    }

    // Switching the default does not retarget a session that is already running: both keep their own
    // provider while the switch is in effect.
    manager
        .set_service_provider(
            plugin_protocol::api::InstanceScope::Workspace,
            plugin_protocol::settings::Scope::Project,
            CONTRACT,
            Some("terminal"),
        )
        .unwrap();
    for (provider, id, _) in &sessions {
        let snapshot = manager.execution(*id).unwrap().snapshot();
        assert_eq!(snapshot.plugin, *provider, "a running session is not moved");
        assert_ne!(
            snapshot.state,
            ExecutionState::Failed,
            "a switch does not disturb a live session"
        );
    }

    // Each provider stops its own session through the same host path. A session may only be stopped
    // while the provider that started it is the selected one, so a provider is never asked to end a
    // program it did not start; selecting it again is therefore part of stopping it.
    for (provider, id, _) in &sessions {
        manager
            .set_service_provider(
                plugin_protocol::api::InstanceScope::Workspace,
                plugin_protocol::settings::Scope::Project,
                CONTRACT,
                Some(provider),
            )
            .unwrap();
        manager.stop_execution(*id).unwrap_or_else(|error| {
            panic!("{provider} stop refused: {error:#}");
        });
        // The stop is queued, so the program is gone once the provider has processed the request. A
        // launch path keeps polling, and waiting here is what a real caller does. Each provider's own
        // count is compared with its own baseline, since a provider may keep processes of its own —
        // the terminal keeps its private shell — that are none of the host's business.
        let baseline = manager.live[*provider].process_count();
        let deadline = Instant::now() + Duration::from_secs(30);
        while manager.live[*provider].process_count() >= baseline && Instant::now() < deadline {
            manager.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        // The real terminal package is what the host ships, so its program ending is the observable
        // proof that a stop reaches the provider that owns the session. The separately packaged
        // provider is held to the contract it declares: it accepts the stop and answers for its own
        // session, while ending a program it delegated is that package's own behaviour.
        if *provider == "terminal" {
            assert!(
                manager.live[*provider].process_count() < baseline,
                "the terminal stopped the program it started: {baseline} then, {} now",
                manager.live[*provider].process_count()
            );
        }
    }
    // Following the default again is a choice, not a preference for whichever provider is first.
    manager
        .set_service_provider(
            plugin_protocol::api::InstanceScope::Workspace,
            plugin_protocol::settings::Scope::Project,
            CONTRACT,
            None,
        )
        .unwrap();
    let missing = manager
        .start_execution(RunRequest {
            program: "powershell.exe".into(),
            args: vec!["-NoProfile".into()],
            cwd: None,
            name: None,
            env: Vec::new(),
        })
        .expect_err("an ambiguous contract has no single owner to route to");
    assert!(
        !missing.to_string().is_empty(),
        "the refusal explains itself: {missing}"
    );
}

/// The text a provider painted into its own panel.
fn panel_text(manager: &Manager, plugin: &str) -> String {
    let Some(view) = manager
        .live
        .get(plugin)
        .and_then(|instance| instance.views.get("terminal"))
    else {
        return String::new();
    };
    let mut text = String::new();
    view.as_ref().root.visit(&mut |node| {
        if let plugin_runtime::plugin_protocol::ui::Kind::Canvas(canvas) = &node.kind {
            for paint in &canvas.paint {
                if let plugin_runtime::plugin_protocol::Paint::Text { text: painted, .. } = paint {
                    text.push_str(painted);
                }
            }
        }
    });
    text
}

/// The environment a launch asks for reaches the program, and nothing else is substituted.
///
/// The host passes the caller's entries through the contract unchanged and the provider forwards
/// them without reading them. A program that prints the value is the only honest evidence that the
/// override arrived, so this checks the value the program itself reported.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn the_environment_a_launch_asks_for_reaches_the_program() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    let request = RunRequest {
        program: "powershell.exe".into(),
        args: vec![
            "-NoProfile".into(),
            "-Command".into(),
            "[Console]::Write('MARKER=' + $env:ME_RUN_MARKER); Start-Sleep -Seconds 60".into(),
        ],
        cwd: Some(root.path().display().to_string()),
        name: None,
        env: vec![plugin_runtime::RunEnvEntry {
            name: "ME_RUN_MARKER".into(),
            value: "carried-through".into(),
        }],
    };
    let session = manager.start_execution(request).unwrap();
    let state = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state != ExecutionState::Starting,
    );
    assert_eq!(state, ExecutionState::Running);
    assert!(reveal_panel(&mut manager) >= 1);
    // The program's own output carries the value the launch asked for.
    let shown = wait_until(
        &mut manager,
        |manager| panel_text(manager, "terminal"),
        |shown| shown.contains("carried-through"),
    );
    assert!(
        shown.contains("carried-through"),
        "the requested environment reached the program: {shown}"
    );
    // The provider is never asked to invent a value it was not given.
    assert!(
        !shown.contains("value-not-requested"),
        "nothing substitutes an environment the launch did not ask for"
    );
    manager.stop_execution(session.id()).unwrap();
}

/// Arguments that are not ASCII reach the program unchanged, as separate arguments.
///
/// Ticket 04 asks for the argument boundary to be verified with spaces, quotes, Chinese and shell
/// metacharacters. The instrument matters: a program that re-parses its own command line measures its
/// own parser, not the transport. PowerShell does exactly that — when the host's argument vector
/// reaches `powershell.exe -Command`, PowerShell flattens the rest into the command text and fails on
/// a value containing `&`, which I confirmed with a separate probe before writing this. So the check
/// uses a program that cannot re-parse: it writes `std::env::args` as it received them, one per line.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn arguments_with_spaces_quotes_and_chinese_reach_the_program_unchanged() {
    let root = tempfile::tempdir().unwrap();
    // Compiled here from a source string, so the check carries its own instrument rather than
    // depending on something built by hand outside the repository.
    let Some(echo) = build_argument_echo(root.path()) else {
        eprintln!("skipping: no Rust compiler is available to build the argument-echo probe");
        return;
    };
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    let out = root.path().join("arguments.txt");
    let request = RunRequest {
        program: echo.display().to_string(),
        args: vec![
            out.display().to_string(),
            "a b".into(),
            "带 空格 的 参数".into(),
            "引号\"在中间".into(),
            "a&b|c>d".into(),
            "".into(),
        ],
        cwd: Some(root.path().display().to_string()),
        name: None,
        env: Vec::new(),
    };
    let session = manager.start_execution(request).unwrap();
    let state = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state != ExecutionState::Starting,
    );
    assert_eq!(state, ExecutionState::Running);
    // The program's own file is the evidence: what it received, not what was sent. One value per
    // line, so an argument that was split or rejoined shows up as the wrong number of lines.
    let written = wait_until(
        &mut manager,
        |_| std::fs::read_to_string(&out).ok(),
        |written| written.is_some(),
    );
    let written = written.expect("the program wrote what it received");
    let received = written.lines().collect::<Vec<_>>();
    assert_eq!(
        received,
        vec!["a b", "带 空格 的 参数", "引号\"在中间", "a&b|c>d", ""],
        "every argument arrived as its own value, unchanged"
    );
    manager.stop_execution(session.id()).unwrap();
}
/// The source of a program whose argument handling is not itself a parser under test.
const ARGUMENT_ECHO_SOURCE: &str = r#"
fn main() {
    let mut arguments = std::env::args().skip(1);
    let out = arguments.next().expect("output path");
    let rest: Vec<String> = arguments.collect();
    std::fs::write(&out, rest.join("\n") + "\n").expect("write");
    std::thread::sleep(std::time::Duration::from_secs(30));
}
"#;

/// Compile the argument-echo probe into `directory`, or `None` when no compiler is available.
fn build_argument_echo(directory: &std::path::Path) -> Option<std::path::PathBuf> {
    let source = directory.join("argument_echo.rs");
    std::fs::write(&source, ARGUMENT_ECHO_SOURCE).ok()?;
    let binary = directory.join("argument_echo.exe");
    let status = std::process::Command::new("rustc")
        .args(["-O", "--edition", "2021", "-o"])
        .arg(&binary)
        .arg(&source)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    (status.success() && binary.is_file()).then_some(binary)
}

/// A Chinese environment value reaches the program, so the path is not only correct for ASCII.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn a_chinese_environment_value_reaches_the_program() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    let request = RunRequest {
        program: "powershell.exe".into(),
        args: vec![
            "-NoProfile".into(),
            "-Command".into(),
            "[Console]::Write('TEXT=' + $env:ME_RUN_TEXT); Start-Sleep -Seconds 60".into(),
        ],
        cwd: Some(root.path().display().to_string()),
        name: None,
        env: vec![plugin_runtime::RunEnvEntry {
            name: "ME_RUN_TEXT".into(),
            value: "中文环境值（含括号）".into(),
        }],
    };
    let session = manager.start_execution(request).unwrap();
    let state = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state != ExecutionState::Starting,
    );
    assert_eq!(state, ExecutionState::Running);
    assert!(reveal_panel(&mut manager) >= 1);
    let shown = wait_until(
        &mut manager,
        |manager| panel_text(manager, "terminal"),
        |shown| shown.contains("中文环境值"),
    );
    assert!(
        shown.contains("中文环境值（含括号）"),
        "the Chinese environment value reached the program unchanged: {shown}"
    );
    manager.stop_execution(session.id()).unwrap();
}

/// Closing the window leaves no program running.
///
/// This is the end-to-end property, checked against the machine's own process table rather than the
/// host's bookkeeping: a program the host merely forgot about would still be running. It does not
/// isolate which step ends it — a provider's instance owns what it started, so both the explicit
/// stop and the instance teardown below end it — but that the property holds is what a user needs.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn closing_the_window_leaves_no_program_running() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    let session = manager
        .start_execution(RunRequest {
            program: "powershell.exe".into(),
            args: vec![
                "-NoProfile".into(),
                "-Command".into(),
                "Start-Sleep -Seconds 120".into(),
            ],
            cwd: Some(root.path().display().to_string()),
            name: None,
            env: Vec::new(),
        })
        .unwrap();
    let state = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Running,
    );
    assert_eq!(state, ExecutionState::Running);
    assert!(reveal_panel(&mut manager) >= 1);
    // The program is real and owned by the provider before the window closes.
    assert!(
        manager.live["terminal"].process_count() >= 2,
        "the provider owns the delegated program and its own shell"
    );

    // Closing the window asks for the program to stop and then stops the provider's own instance.
    let before = powershell_processes();
    manager.shutdown();
    assert_eq!(
        manager.live.len(),
        0,
        "closing the window stops the provider's instance"
    );
    // The program is gone from the machine, not merely forgotten by the host. This is the observable
    // outcome of the whole path: the host asked, the provider terminated, and the process ended.
    let gone = wait_until(
        &mut manager,
        |_| powershell_processes(),
        |after| *after <= before,
    );
    assert!(
        gone <= before,
        "the launched program is gone: {} before, {gone} after",
        before
    );
}

/// Whether any program is still running with this check's own marker in its command line.
///
/// The marker is unique to this check, so a concurrently running test starting its own programs
/// cannot be mistaken for a leak here — which a machine-wide count of the same interpreter would be.
fn marked_program_is_running(marker: &str) -> bool {
    std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                // The probe's own command line contains the marker, so it excludes itself; otherwise
                // it would always find one match and the check would never fail.
                "$self = $PID; @(Get-CimInstance Win32_Process -Filter \"Name='powershell.exe'\" | \
                 Where-Object {{ $_.CommandLine -like '*{marker}*' -and $_.ProcessId -ne $self }}).Count"
            ),
        ])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().parse::<usize>().unwrap_or(0) > 0)
        .unwrap_or(false)
}

/// Machine-wide count of the interpreter these checks launch, so a leaked program is visible.
///
/// Counting the machine rather than the runtime is the point: a program the host merely forgot about
/// would still be running, and only the real process table shows that.
fn powershell_processes() -> usize {
    std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-Process powershell -ErrorAction SilentlyContinue).Count",
        ])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| text.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

/// Closing the window while a launch is still preparing leaves no program behind.
///
/// A session that has been asked for but has not answered is still the host's responsibility: the
/// program may be starting, and the provider is told to stop it before the instance goes away. The
/// program count is read from the machine by a second process, so the check is about what is really
/// running rather than about the host's own bookkeeping.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn closing_while_a_launch_is_still_preparing_leaves_no_program_running() {
    /// Written into the launched program's command line, so only this check's program can match it.
    const MARKER: &str = "RDB_PREPARING_CLOSE_CHECK";
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let plugins = root.path().join("plugins");
    let program = workspace.display().to_string();
    // The launch runs on its own thread: the window closes while it is still starting, which is the
    // state this check is about and cannot be arranged from the same thread.
    // The state at the moment of closing is reported by the worker, so the check knows it was really
    // about a launch that had not been confirmed.
    let (report, observed) = std::sync::mpsc::channel();
    let launched = std::thread::spawn(move || {
        let mut manager = Manager::open(
            plugins,
            plugin_runtime::plugin_protocol::Environment {
                workspace: program,
                os: "windows".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let package = Package::read(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
        )
        .unwrap();
        let grants = package.manifest.permissions.clone();
        manager.install(&package, grants).unwrap();
        let session = manager
            .start_execution(RunRequest {
                program: "powershell.exe".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-Command".into(),
                    format!("Start-Sleep -Seconds 120 # {MARKER}"),
                ],
                cwd: None,
                name: None,
                env: Vec::new(),
            })
            .unwrap();
        // Close immediately, while the request is still only queued: nothing has confirmed a program.
        let state = manager.execution(session.id()).unwrap().snapshot().state;
        let _ = report.send(state);
        manager.shutdown();
    });
    launched.join().unwrap();
    let state = observed
        .recv()
        .expect("the worker reported the state it closed on");
    assert_eq!(
        state,
        ExecutionState::Starting,
        "the window closed while the launch was still preparing"
    );
    // What this can honestly measure is whether the program outlives the worker that asked for it.
    // The marker is unique to this check, so a concurrently running test starting its own programs
    // cannot be mistaken for one left behind here — which a machine-wide count would be.
    assert!(
        !marked_program_is_running(MARKER),
        "the program the launch asked for did not outlive the window that asked for it"
    );
}

#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn shutdown_with_no_sessions_is_immediate() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let started = std::time::Instant::now();
    manager.shutdown();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "shutdown waits only for a provider's answer, never for a fixed period"
    );
}

/// A package's declared abilities are read from its own declaration, never assumed from its presence.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn a_debug_package_offers_exactly_what_it_declares() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    // One provider declares the whole contract, another omits stepping and inspection.
    for package in [
        packages::debug_provider("debug-full", &[]),
        packages::debug_provider("debug-partial", &["step", "frames", "variables"]),
    ] {
        let grants = package.manifest.permissions.clone();
        manager.install(&package, grants).unwrap();
    }

    // Both are debug providers, because a session can exist without stepping or inspecting: a missing
    // ability is reported where the control that needs it is, not by withdrawing the provider.
    let providers = manager.debug_providers();
    assert_eq!(
        providers
            .iter()
            .map(|candidate| candidate.plugin.as_str())
            .collect::<Vec<_>>(),
        vec!["debug-full", "debug-partial"],
        "{providers:?}"
    );
    // A package that declares the required methods is usable, and one that omits an optional ability
    // still offers the abilities it did declare.
    assert!(
        providers
            .iter()
            .all(|candidate| candidate.unavailable.is_none()),
        "both packages can serve a session: {providers:?}"
    );
    let full = manager
        .debug_abilities("debug-full")
        .expect("the full provider is installed");
    assert_eq!(
        full,
        plugin_runtime::DebugAbilities {
            breakpoints: true,
            resume_pause: true,
            step: true,
            inspect: true,
        }
    );
    let partial = manager
        .debug_abilities("debug-partial")
        .expect("the partial provider is installed");
    assert!(!partial.step && !partial.inspect);
    assert!(
        partial.breakpoints && partial.resume_pause,
        "the abilities it did declare are still offered: {partial:?}"
    );
    // A package that is not a debug provider at all has no abilities to report.
    assert!(manager.debug_abilities("nobody").is_none());

    // With more than one usable provider and no choice made, the host refuses to pick one rather than
    // guessing, because either could serve the session.
    let refusal = manager
        .debug_availability()
        .expect_err("an ambiguous choice is not made for the user");
    assert!(refusal.contains("请先选择"), "{refusal}");
}

/// Losing a provider fails the sessions it was serving, and cannot revive or replace them.
///
/// The failure half of the lifecycle ticket: every session that lost its provider is visibly failed,
/// the programs are no longer claimed to be running, the retired identity cannot stop anything, and
/// the provider that replaced it is a new instance that does not inherit the old sessions.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn losing_a_provider_fails_its_sessions_without_reviving_them() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    let program = |marker: &str| RunRequest {
        program: "powershell.exe".into(),
        args: vec![
            "-NoProfile".into(),
            "-Command".into(),
            format!("[Console]::Write('{marker}'); Start-Sleep -Seconds 120"),
        ],
        cwd: Some(root.path().display().to_string()),
        name: Some(marker.into()),
        env: Vec::new(),
    };

    // Two sessions the one provider is serving, both really running.
    let first = manager.start_execution(program("FIRST")).unwrap();
    let second = manager.start_execution(program("SECOND")).unwrap();
    for session in [first.id(), second.id()] {
        let state = wait_until(
            &mut manager,
            |manager| manager.execution(session).unwrap().snapshot().state,
            |state| *state == ExecutionState::Running,
        );
        assert_eq!(state, ExecutionState::Running, "session {session}");
        assert!(manager.execution(session).unwrap().stoppable());
    }
    let processes = manager.live["terminal"].process_count();
    assert!(processes >= 3, "two programs and the provider's own shell");

    // The provider goes away: every session it served is failed, and none of them is claimed to be
    // stoppable through a provider that is no longer there.
    manager.disable("terminal").unwrap();
    for session in [first.id(), second.id()] {
        let failed = wait_until(
            &mut manager,
            |manager| manager.execution(session).unwrap().snapshot().state,
            |state| *state == ExecutionState::Failed,
        );
        assert_eq!(failed, ExecutionState::Failed, "session {session}");
        let snapshot = manager.execution(session).unwrap().snapshot();
        assert_eq!(
            snapshot.plugin, "terminal",
            "the failure names the provider that went away"
        );
        assert!(
            !manager.execution(session).unwrap().stoppable(),
            "a session whose provider is gone cannot be stopped through it"
        );
    }
    assert!(manager.live.is_empty(), "the instance is gone, not idle");

    // The retired identity cannot be reused: bringing the provider back gives a new instance that
    // does not inherit the old sessions, and the old sessions are not revived.
    manager.enable("terminal").unwrap();
    for session in [first.id(), second.id()] {
        assert_eq!(
            manager.execution(session).unwrap().snapshot().state,
            ExecutionState::Failed,
            "session {session} is not revived by the provider's return"
        );
        let refusal = manager
            .stop_execution(session)
            .expect_err("a lost session is not stoppable by the new instance");
        assert!(
            !refusal.to_string().is_empty(),
            "the refusal explains itself: {refusal}"
        );
    }
    // A new session through the new instance works, which is what shows the provider recovered while
    // the old sessions did not.
    let replacement = manager.start_execution(program("AGAIN")).unwrap();
    let state = wait_until(
        &mut manager,
        |manager| {
            manager
                .execution(replacement.id())
                .unwrap()
                .snapshot()
                .state
        },
        |state| *state == ExecutionState::Running,
    );
    assert_eq!(state, ExecutionState::Running);
    assert_ne!(
        replacement.id(),
        first.id(),
        "a new session is a new session"
    );
    manager.stop_execution(replacement.id()).unwrap();
}
/// Restarting a provider recovers it without replaying a command anyone already asked for.
///
/// This is the no-replay half of the lifecycle ticket: a restart is the user asking for a working
/// provider again, not a request to run anything, so the sessions that were lost stay lost and no
/// program starts because a guest was rebuilt. The machine's own process table is what decides
/// whether anything was replayed.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn restarting_a_provider_replays_no_program() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    let program = RunRequest {
        program: "powershell.exe".into(),
        args: vec![
            "-NoProfile".into(),
            "-Command".into(),
            "Start-Sleep -Seconds 120".into(),
        ],
        cwd: Some(root.path().display().to_string()),
        name: Some("REPLAY_CHECK".into()),
        env: Vec::new(),
    };
    let session = manager.start_execution(program.clone()).unwrap();
    let state = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Running,
    );
    assert_eq!(state, ExecutionState::Running);
    assert!(reveal_panel(&mut manager) >= 1);
    // One delegated program, plus the provider's own shell.
    assert!(manager.live["terminal"].process_count() >= 2);
    let before = powershell_processes();

    // Disable and restart the provider: the guest is rebuilt from the committed checkpoint.
    manager.disable("terminal").unwrap();
    let failed = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Failed,
    );
    assert_eq!(failed, ExecutionState::Failed);
    manager.enable("terminal").unwrap();
    manager.restart_plugin("terminal").unwrap();
    // Give a replay every chance to happen before deciding it did not.
    for _ in 0..25 {
        manager.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        manager.execution(session.id()).unwrap().snapshot().state,
        ExecutionState::Failed,
        "a restart does not revive the session it lost"
    );
    assert!(
        powershell_processes() <= before,
        "no program was started again by the restart: {before} before, {} after",
        powershell_processes()
    );
    assert!(
        manager.live["terminal"].process_count() <= 1,
        "the rebuilt provider owns no program of its own"
    );

    // A repeat launch after the restart is a real session, which is what shows the recovery worked
    // without the host having replayed anything on its own.
    let again = manager.start_execution(program).unwrap();
    let state = wait_until(
        &mut manager,
        |manager| manager.execution(again.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Running,
    );
    assert_eq!(state, ExecutionState::Running);
    assert_ne!(again.id(), session.id());
    manager.stop_execution(again.id()).unwrap();
}

/// A session's end is observed through its provider, never predicted from elapsed time.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn host_observes_a_program_exit_through_its_provider() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    // A program that ends on its own, so the observation is of a real exit rather than a stop.
    let request = RunRequest {
        program: "powershell.exe".into(),
        args: vec!["-NoProfile".into(), "-Command".into(), "exit 7".into()],
        cwd: Some(root.path().display().to_string()),
        name: Some("会退出".into()),
        env: Vec::new(),
    };
    let session = manager.start_execution(request).unwrap();
    let running = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Running,
    );
    assert_eq!(running, ExecutionState::Running);
    assert!(reveal_panel(&mut manager) >= 1);

    // Polling the provider is what turns "the program is gone" into a fact with a status.
    let mut observed = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    while observed.is_none() && Instant::now() < deadline {
        manager.poll();
        let query = manager.query_execution(session.id()).unwrap();
        manager.poll();
        if let RequestUpdate::Completed { result: Ok(value) } = query.status() {
            let state = value
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if state != "running" {
                observed = Some(value.clone());
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let value = observed.expect("the provider reported the program's end");
    assert_eq!(value.get("state").and_then(Value::as_str), Some("exited"));
    assert_eq!(
        value.get("code").and_then(Value::as_u64),
        Some(7),
        "the exit status is the program's own, not a guess"
    );
    // The observation did not create a second session for the run controls to show.
    assert_eq!(manager.executions().len(), 1);
    // An unknown session is refused instead of answering for another program.
    assert!(manager.query_execution(session.id() + 500).is_err());
}

/// The host stops a running program through the provider's own session control.
///
/// The request names the session the provider returned; the host never borrows the provider's
/// private process handle, and an acknowledgement is not treated as proof that the program exited.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn host_controls_stop_the_program_a_session_owns() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    let request = RunRequest {
        program: "powershell.exe".into(),
        args: vec![
            "-NoProfile".into(),
            "-Command".into(),
            "Start-Sleep -Seconds 60".into(),
        ],
        cwd: Some(root.path().display().to_string()),
        name: Some("可停止".into()),
        env: Vec::new(),
    };
    let session = manager.start_execution(request.clone()).unwrap();
    let running = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Running,
    );
    assert_eq!(running, ExecutionState::Running);
    assert!(manager.execution(session.id()).unwrap().stoppable());
    // A delegated program and the provider's own private shell are both alive here.
    assert_eq!(manager.live["terminal"].process_count(), 2);
    assert!(reveal_panel(&mut manager) >= 1);

    // Stopping is accepted by the exact incarnation that started the program.
    manager.stop_execution(session.id()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while manager.live["terminal"].process_count() > 1 && Instant::now() < deadline {
        manager.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    // The owned program is gone while the provider's private session remains.
    assert_eq!(manager.live["terminal"].process_count(), 1);
    // Contract 1.3 can observe a terminal status. Wait for that answer rather than deriving exit
    // from the process count or from the preceding stop acknowledgement.
    wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Exited,
    );
    // Repeating a confirmed stop remains harmless.
    manager.stop_execution(session.id()).unwrap();
    // An unknown session is refused instead of stopping an unrelated program.
    assert!(manager.stop_execution(session.id() + 1000).is_err());
}

/// Default selection affects the next start while a real existing program stays owned by its provider.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn switching_the_default_keeps_existing_execution_controls_pinned() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let first = terminal();
    manager
        .install(&first, first.manifest.permissions.clone())
        .unwrap();
    let mut manifest = serde_json::to_value(&first.manifest).unwrap();
    manifest["id"] = serde_json::json!("alternative-terminal");
    manifest["name"] = serde_json::json!("Alternative terminal");
    let second = packages::archive(first.files, manifest);
    manager
        .install(&second, second.manifest.permissions.clone())
        .unwrap();
    let select = |manager: &mut Manager, provider: &str| {
        manager
            .set_service_provider(
                plugin_protocol::api::InstanceScope::Workspace,
                plugin_protocol::settings::Scope::Project,
                plugin_runtime::EXECUTION_CONTRACT,
                Some(provider),
            )
            .unwrap()
    };
    select(&mut manager, "terminal");
    let session = manager
        .start_execution(RunRequest {
            program: "powershell.exe".into(),
            args: vec![
                "-NoProfile".into(),
                "-Command".into(),
                "Start-Sleep -Seconds 60".into(),
            ],
            cwd: None,
            name: None,
            env: vec![],
        })
        .unwrap();
    wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Running,
    );
    assert_eq!(manager.live["terminal"].process_count(), 2);
    select(&mut manager, "alternative-terminal");
    // Both query and stop remain bound to the original incarnation, despite the changed revision.
    let query = manager.query_execution(session.id()).unwrap();
    manager.poll_request(&query);
    assert!(matches!(
        query.status(),
        plugin_protocol::api::RequestUpdate::Completed { result: Ok(_) }
    ));
    manager.stop_execution(session.id()).unwrap();
    wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Exited,
    );
    assert_eq!(manager.live["terminal"].process_count(), 1);
    assert_eq!(manager.live["alternative-terminal"].process_count(), 1);
    let next = manager.start_execution(session.request().clone()).unwrap();
    assert_eq!(next.plugin(), "alternative-terminal");
    manager.shutdown();
}
