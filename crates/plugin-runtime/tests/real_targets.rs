//! Real Cargo workspace -> independently built Rust WASM -> public target contract, without host Cargo rules.
#![cfg(windows)]
use plugin_runtime::{
    Manager, Package, TargetRequest,
    plugin_protocol::{Environment, api::RequestUpdate},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};
/// Poll only actual completion; stopping releases the request's resource root on the same manager.
fn answer(manager: &mut Manager, request: TargetRequest) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match request.status() {
            RequestUpdate::Completed { result } => {
                return result.unwrap_or_else(|error| panic!("target provider: {}", error.message));
            }
            RequestUpdate::Cancelled { reason, .. } => {
                panic!("target request cancelled: {reason:?}")
            }
            _ => {
                assert!(Instant::now() < deadline, "target wait did not complete");
                manager.poll();
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
/// A virtual workspace includes implicit and explicit bins and a non-default artifact directory.
fn project(root: &Path) {
    std::fs::create_dir_all(root.join("member/src/bin")).unwrap();
    std::fs::create_dir_all(root.join(".cargo")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers=[\"member\"]\nresolver=\"2\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".cargo/config.toml"),
        "[build]\ntarget-dir=\"custom-artifacts\"\n",
    )
    .unwrap();
    std::fs::write(root.join("member/Cargo.toml"),"[package]\nname=\"target-member\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[[bin]]\nname=\"explicit\"\npath=\"src/explicit.rs\"\n").unwrap();
    // The final executable writes its own marker. Preparation must never create this file.
    std::fs::write(root.join("member/src/explicit.rs"),"//! Native acceptance target.\nfn main() { std::fs::write(\"target-ran.txt\",\"explicit\").unwrap(); }\n").unwrap();
    std::fs::write(
        root.join("member/src/bin/implicit.rs"),
        "//! Implicit Cargo acceptance target.\nfn main() { println!(\"implicit\"); }\n",
    )
    .unwrap();
}
#[test]
#[ignore = "build the Rust package with the current embedded SDK first"]
fn virtual_workspace_profiles_and_real_artifacts_use_the_public_provider() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let package =
        Package::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/rust.zip"))
            .unwrap();
    let mut manager = Manager::open(
        root.path().join("private-plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert_eq!(manager.target_providers(), ["rust"]);
    let request = manager
        .begin_target_call(
            "rust",
            "discover",
            json!({"workspace":root.path().display().to_string()}),
        )
        .unwrap();
    let catalog = answer(&mut manager, request);
    assert_eq!(
        catalog["targets"].as_array().unwrap().len(),
        4,
        "both binary declarations times two profiles"
    );
    assert!(
        !root.path().join("target-ran.txt").exists(),
        "discovery never executes the target"
    );
    for profile in ["debug", "release"] {
        let binding = catalog["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|candidate| {
                let binding: Value = serde_json::from_str(candidate["binding"].as_str()?).ok()?;
                (binding["bin"] == "explicit" && binding["profile"] == profile)
                    .then(|| candidate["binding"].clone())
            })
            .unwrap();
        let request = manager
            .begin_target_call(
                "rust",
                "prepare",
                json!({"workspace":root.path().display().to_string(),"binding":binding,"env":[]}),
            )
            .unwrap();
        let artifact = answer(&mut manager, request);
        let program = Path::new(artifact["program"].as_str().unwrap());
        assert!(program.is_file());
        assert!(program.to_string_lossy().contains("custom-artifacts"));
        assert!(program.to_string_lossy().contains(profile));
        assert!(
            !root.path().join("target-ran.txt").exists(),
            "build never runs the final target"
        );
        let output = std::process::Command::new(program)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            std::fs::read_to_string(root.path().join("target-ran.txt")).unwrap(),
            "explicit"
        );
        std::fs::remove_file(root.path().join("target-ran.txt")).unwrap();
        manager.poll();
        assert_eq!(manager.live["rust"].process_count(), 0);
    }
    manager.shutdown();
}

/// A root package must remain selected even when Cargo's default members choose a same-named bin.
#[test]
#[ignore = "build the Rust package with the current embedded SDK first"]
fn a_root_candidate_never_executes_a_default_members_same_named_binary() {
    let root = tempfile::tempdir().unwrap();
    for directory in ["src", "member/src"] {
        std::fs::create_dir_all(root.path().join(directory)).unwrap();
    }
    std::fs::write(root.path().join("Cargo.toml"),"[package]\nname=\"selected-root\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[[bin]]\nname=\"same-name\"\npath=\"src/main.rs\"\n[workspace]\nmembers=[\"member\"]\ndefault-members=[\"member\"]\nresolver=\"2\"\n").unwrap();
    std::fs::write(root.path().join("member/Cargo.toml"),"[package]\nname=\"other-member\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[[bin]]\nname=\"same-name\"\npath=\"src/main.rs\"\n").unwrap();
    for (source, marker) in [
        ("src/main.rs", "SELECTED_ROOT"),
        ("member/src/main.rs", "WRONG_DEFAULT_MEMBER"),
    ] {
        std::fs::write(root.path().join(source),format!("//! Exact package selection acceptance target.\nfn main() {{println!(\"{marker}\");}}\n")).unwrap();
    }
    let package =
        Package::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/rust.zip"))
            .unwrap();
    let mut manager = Manager::open(
        root.path().join("private-plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let request = manager
        .begin_target_call(
            "rust",
            "discover",
            json!({"workspace":root.path().display().to_string()}),
        )
        .unwrap();
    let catalog = answer(&mut manager, request);
    let binding = catalog["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| {
            candidate["source"] == "Cargo.toml"
                && candidate["label"].as_str().unwrap().ends_with("Debug")
        })
        .unwrap()["binding"]
        .clone();
    let request = manager
        .begin_target_call(
            "rust",
            "prepare",
            json!({"workspace":root.path().display().to_string(),"binding":binding,"env":[]}),
        )
        .unwrap();
    let prepared = answer(&mut manager, request);
    let output = std::process::Command::new(prepared["program"].as_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "SELECTED_ROOT"
    );
    manager.shutdown();
}

/// Installing Rust beside other contributors cannot turn an unrelated project into a Cargo error.
#[test]
#[ignore = "build the Rust package with the current embedded SDK first"]
fn an_unrelated_workspace_gets_an_empty_rust_catalog_without_starting_cargo() {
    let root = tempfile::tempdir().unwrap();
    let package =
        Package::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/rust.zip"))
            .unwrap();
    let mut manager = Manager::open(
        root.path().join("private-plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let request = manager
        .begin_target_call(
            "rust",
            "discover",
            json!({"workspace":root.path().display().to_string()}),
        )
        .unwrap();
    assert_eq!(answer(&mut manager, request)["targets"], json!([]));
    assert_eq!(manager.live["rust"].process_count(), 0);
    manager.shutdown();
}
/// A compiler fixture creates a real descendant that inherits Cargo's job and output streams.
fn slow_preparation(root: &Path) -> (Manager, String) {
    project(root);
    std::fs::write(
        root.join("member/build.rs"),
        r#"//! Build preparation lifecycle acceptance fixture.
fn main() {
    let child=std::process::Command::new("powershell.exe")
        .args(["-NoProfile","-Command","Start-Sleep -Seconds 120"]).spawn().unwrap();
    let directory=std::env::var("CARGO_MANIFEST_DIR").unwrap();
    std::fs::write(std::path::Path::new(&directory).join("build-ready.pids"),
        format!("{} {}",std::process::id(),child.id())).unwrap();
    eprintln!("构建准备已启动 / native preparation ready");
    std::thread::sleep(std::time::Duration::from_secs(120));
}
"#,
    )
    .unwrap();
    let package =
        Package::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/rust.zip"))
            .unwrap();
    let mut manager = Manager::open(
        root.join("private-plugins"),
        Environment {
            workspace: root.display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let discovery = manager
        .begin_target_call(
            "rust",
            "discover",
            json!({"workspace":root.display().to_string()}),
        )
        .unwrap();
    let catalog = answer(&mut manager, discovery);
    let binding = catalog["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["label"].as_str().unwrap().ends_with("Debug"))
        .unwrap()["binding"]
        .as_str()
        .unwrap()
        .to_owned();
    (manager, binding)
}
/// Use the OS process state rather than the provider's already-revoked handle table.
fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0;
        let queried = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        queried && code == 259
    }
}
/// Observe a real build script/descendant before exercising revocation; no elapsed delay implies ready.
fn build_ready(manager: &mut Manager, root: &Path, request: &TargetRequest) -> Vec<u32> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        manager.poll();
        if let Ok(text) = std::fs::read_to_string(root.join("member/build-ready.pids")) {
            let pids = text
                .split_whitespace()
                .map(|value| value.parse().unwrap())
                .collect::<Vec<_>>();
            if pids.len() == 2 && pids.iter().all(|pid| process_alive(*pid)) {
                return pids;
            }
        }
        assert!(
            !request.status().is_terminal(),
            "preparation ended before its real child existed"
        );
        assert!(Instant::now() < deadline, "real build script did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// Normal stop/force upgrades seal the sequence immediately but publish completion only after EOF/tree.
#[test]
#[ignore = "build Rust through the current public SDK first"]
fn stopping_a_real_preparation_observes_its_tree_and_keeps_unrelated_work_alive() {
    use plugin_runtime::plugin_protocol::process::ExitMode;
    for force in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (mut manager, binding) = slow_preparation(root.path());
        let request = manager
            .begin_target_call(
                "rust",
                "prepare",
                json!({"workspace":root.path().display().to_string(),"binding":binding,"env":[]}),
            )
            .unwrap();
        let pids = build_ready(&mut manager, root.path(), &request);
        request.stop_with(ExitMode::Graceful);
        if force {
            request.stop_with(ExitMode::Force);
        }
        assert!(
            !request.status().is_terminal(),
            "accepting Stop cannot mean the native tree is already gone"
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        while !request.status().is_terminal() {
            manager.poll();
            assert!(Instant::now() < deadline, "preparation stop never settled");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            pids.iter().all(|pid| !process_alive(*pid)),
            "stop reported completion before the actual descendants exited"
        );
        assert_eq!(manager.live["rust"].process_count(), 0);
        assert!(
            !root.path().join("target-ran.txt").exists(),
            "a stopped build cannot run the final target"
        );
        // A fresh request may begin after actual retirement. Old output/history must not be overwritten.
        let old_output = request.snapshot().output;
        let discovery = manager
            .begin_target_call(
                "rust",
                "discover",
                json!({"workspace":root.path().display().to_string()}),
            )
            .unwrap();
        assert_eq!(
            answer(&mut manager, discovery)["targets"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(request.snapshot().output, old_output);
        manager.shutdown();
    }
}
/// Provider retirement captures observers before clearing roots: an already accepted build must settle.
#[test]
#[ignore = "build Rust through the current public SDK first"]
fn provider_retirement_settles_a_preparation_after_actual_process_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, binding) = slow_preparation(root.path());
    let request = manager
        .begin_target_call(
            "rust",
            "prepare",
            json!({"workspace":root.path().display().to_string(),"binding":binding,"env":[]}),
        )
        .unwrap();
    let pids = build_ready(&mut manager, root.path(), &request);
    manager.disable("rust").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !request.status().is_terminal() {
        manager.poll();
        assert!(
            Instant::now() < deadline,
            "retired preparation stayed Accepted forever"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(pids.iter().all(|pid| !process_alive(*pid)));
    assert!(!request.valid_for(&manager));
    assert!(!root.path().join("target-ran.txt").exists());
    manager.shutdown();
}
