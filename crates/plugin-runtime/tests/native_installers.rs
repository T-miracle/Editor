//! Native installer acceptance at the public package/manager boundary, with isolated local fixtures.
use plugin_runtime::{InstallControl, Manager, Package, plugin_protocol::Environment};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Write},
    path::Path,
    time::{Duration, Instant},
};

/// The program arrives as verified package bytes, never a shell command or ambient PATH tool.
fn package(marker: &Path, mode: &str, sdk: bool) -> Package {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/examples/installer_fixture.exe");
    let bytes = std::fs::read(fixture).expect("build --example installer_fixture first");
    let manifest = json!({
        "id":"installer-example","name":"Installer example","version":"1.0.0","protocol":7,
        "api":{"base":"^1","required":{"language.lsp":"^1","process":"^1","dependencies":"^1"}},
        "contributions":"plugin.toml", "storage_limit":1024,
        "permissions":["dependencies.prepare","dependencies.install","process.service.analysis"],
        "language_servers":[{"id":"analysis","language":"arbitrary","service":"analysis"}],
        "services":{"analysis":{"program":"unused","installation":{
            "executable":"server/installed/tool.exe","artifacts":[{
                "id":"server","version":"1","platform":format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH),
                "sha256":format!("{:x}",Sha256::digest(&bytes)),"source":{"kind":"package","path":"installer.exe"},
                "format":{"kind":"file","path":"installer.exe"},"installer":{
                    "program":"installer.exe","args":["${target}",marker,mode],"target":"installed",
                    "purpose":"Prepare private test tool","kind":if sdk {"project_sdk"} else {"service"}
                }
            }]
        }}}
    });
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json",serde_json::to_vec(&manifest).unwrap()),
        ("installer.exe",bytes),
        ("plugin.toml",b"[plugin]\nid='installer-example'\nname='Installer example'\nversion='1.0.0'\nhost_version='^0.1'\n".to_vec())
    ] {
        archive.start_file(name,zip::write::SimpleFileOptions::default()).unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// SDK installation needs an explicit choice; the exact prompt can be consumed only once.
#[test]
#[ignore = "build installer_fixture first"]
fn concrete_approval_prepares_private_sdk_and_reuses_completed_cache() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("executed.txt");
    let package = package(&marker, "child-success", true);
    let mut manager = Manager::open(
        temp.path().join("plugins"),
        Environment {
            workspace: temp.path().display().to_string(),
            ..Environment::default()
        },
    )
    .unwrap();
    let control = InstallControl::default().with_installer_prompts();
    let ui = control.clone();
    let worker = std::thread::spawn(move || {
        manager
            .install_with_control(&package, package.manifest.permissions.clone(), &control)
            .unwrap();
        // No second execution or prompt is necessary for a complete immutable version.
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        assert!(manager.language_services()["installer-example/analysis"].is_ok());
        manager.uninstall("installer-example", false).unwrap();
        assert_eq!(manager.collect_dependency_cache().unwrap(), 1);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let prompt = loop {
        if let Some(prompt) = ui.installer_prompt() {
            break prompt;
        }
        assert!(
            Instant::now() < deadline,
            "installer did not request approval"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!marker.exists());
    assert!(
        prompt
            .target
            .starts_with(temp.path().canonicalize().unwrap())
    );
    assert_eq!(prompt.purpose, "Prepare private test tool");
    assert!(!ui.approve_installer(prompt.id, false));
    assert!(ui.approve_installer(prompt.id, true));
    assert!(!ui.approve_installer(prompt.id, true));
    let execution = wait_prompt(&ui);
    assert!(!execution.preparation_only);
    assert!(!marker.exists());
    assert!(ui.approve_installer(execution.id, false));
    worker.join().unwrap();
    assert!(marker.exists());
}

/// Partial native effects are not called rollback: the external marker remains, but staging is retired.
#[test]
#[ignore = "build installer_fixture first"]
fn failed_cancelled_and_refused_installers_can_retry_without_publishing_partial_cache() {
    for mode in ["fail-once", "wait-once", "child-cancel", "refuse"] {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("executed.txt");
        let package = package(&marker, mode, false);
        let manager = Manager::open(
            temp.path().join("plugins"),
            Environment {
                workspace: temp.path().display().to_string(),
                ..Environment::default()
            },
        )
        .unwrap();
        let control = InstallControl::default().with_installer_prompts();
        let ui = control.clone();
        let candidate = package.clone();
        let worker = std::thread::spawn(move || {
            let mut manager = manager;
            let result = manager.install_with_control(
                &candidate,
                candidate.manifest.permissions.clone(),
                &control,
            );
            (manager, result)
        });
        let prompt = wait_prompt(&ui);
        if mode == "refuse" {
            ui.cancel();
        } else {
            assert!(ui.approve_installer(prompt.id, false));
            if matches!(mode, "wait-once" | "child-cancel") {
                let deadline = Instant::now() + Duration::from_secs(10);
                let ready = if mode == "child-cancel" {
                    marker.with_extension("child")
                } else {
                    marker.clone()
                };
                while !ready.exists() {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(10));
                }
                ui.cancel();
            }
        }
        let (mut manager, result) = worker.join().unwrap();
        assert!(result.is_err());
        assert!(!manager.installed.contains_key("installer-example"));
        assert!(
            !prompt.target.exists(),
            "temporary native output must be cleaned after {mode}"
        );
        assert_eq!(marker.exists(), mode != "refuse");
        let control = InstallControl::default().with_installer_prompts();
        let ui = control.clone();
        let retry = std::thread::spawn(move || {
            manager
                .install_with_control(&package, package.manifest.permissions.clone(), &control)
                .unwrap();
            assert!(manager.language_services()["installer-example/analysis"].is_ok());
        });
        let prompt = wait_prompt(&ui);
        assert!(ui.approve_installer(prompt.id, false));
        retry.join().unwrap();
    }
}

/// Refusing a large SDK must not even contact its source, regardless of a general installation grant.
#[test]
#[ignore = "build installer_fixture first"]
fn sdk_refusal_performs_no_network_preparation() {
    let temp = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut files = package(&temp.path().join("never.txt"), "success", true).files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["services"]["analysis"]["installation"]["artifacts"][0]["source"] = json!({
        "kind":"url", "url":format!("http://{}/sdk.zip",listener.local_addr().unwrap())
    });
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    let package = Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap();
    let control = InstallControl::default().with_installer_prompts();
    let ui = control.clone();
    let root = temp.path().join("plugins");
    let workspace = temp.path().display().to_string();
    let worker = std::thread::spawn(move || {
        let mut manager = Manager::open(
            root,
            Environment {
                workspace,
                ..Environment::default()
            },
        )
        .unwrap();
        manager.install_with_control(&package, package.manifest.permissions.clone(), &control)
    });
    assert!(wait_prompt(&ui).preparation_only);
    assert!(
        matches!(listener.accept(), Err(error) if error.kind()==std::io::ErrorKind::WouldBlock)
    );
    ui.cancel();
    assert!(worker.join().unwrap().is_err());
    assert!(
        matches!(listener.accept(), Err(error) if error.kind()==std::io::ErrorKind::WouldBlock)
    );
}

/// Bounded waiting makes a missing consent publication fail visibly instead of hanging the test runner.
fn wait_prompt(control: &InstallControl) -> plugin_runtime::InstallerPrompt {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(prompt) = control.installer_prompt() {
            return prompt;
        }
        assert!(
            Instant::now() < deadline,
            "installer did not request approval"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
