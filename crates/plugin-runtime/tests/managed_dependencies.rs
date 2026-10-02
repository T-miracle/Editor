//! Install dependency plans through the same package/manager boundary used by the editor worker.
use plugin_runtime::{Manager, Package, plugin_protocol::Environment};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Write};

/// The archive is independent of host language names and supplies its own dependency bytes.
fn package(source: Value, bytes: &[u8]) -> anyhow::Result<Package> {
    package_with(source, bytes, |_| {})
}
/// Mutations are serialized and inspected again, so malformed fixtures cannot bypass package validation.
fn package_with(
    source: Value,
    bytes: &[u8],
    edit: impl FnOnce(&mut Value),
) -> anyhow::Result<Package> {
    let mut manifest = json!({
        "id":"private-analysis", "name":"Private analysis", "version":"1.0.0", "protocol":7,
        "api":{"base":"^1", "required":{"language.lsp":"^1", "process":"^1", "dependencies":"^1"}},
        "contributions":"plugin.toml", "storage_limit":1024,
        "services":{"analysis":{"program":"never-on-path", "installation": {
            "executable":"server/tool.exe", "artifacts":[{
                "id":"server", "version":"1.0", "platform":format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH),
                "sha256":format!("{:x}",Sha256::digest(bytes)), "source":source,
                "format":{"kind":"file","path":"tool.exe"}
            }]
        }}},
        "permissions":["process.service.analysis","dependencies.prepare"],
        "language_servers":[{"id":"analysis", "language":"unknown-language", "service":"analysis"}]
    });
    edit(&mut manifest);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest)?),
        ("tool.exe", bytes.to_vec()),
        ("plugin.toml", b"[plugin]\nid = \"private-analysis\"\nname = \"Private analysis\"\nversion = \"1.0.0\"\nhost_version = \"^0.1\"\n".to_vec()),
    ] {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    Package::from_bytes(&archive.finish()?.into_inner())
}

/// A package-contained dependency is prepared offline without searching or modifying global PATH.
#[test]
fn bundled_service_is_prepared_in_a_private_cache() {
    let temp = tempfile::tempdir().unwrap();
    let before = std::env::var_os("PATH");
    let package = package(json!({"kind":"package", "path":"tool.exe"}), b"fixture").unwrap();
    let mut manager = Manager::open(
        temp.path().join("plugins"),
        Environment {
            workspace: temp.path().display().to_string(),
            ..Environment::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(manager.language_services()["private-analysis/analysis"].is_ok());
    assert_eq!(std::env::var_os("PATH"), before);
}

/// A local HTTP fixture exercises actual download, then the same immutable version works offline.
#[test]
fn downloaded_service_is_verified_and_reused_offline() {
    use std::io::Read;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/tool", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        stream.read(&mut request).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nfixture")
            .unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let package = package(json!({"kind":"url", "url":url}), b"fixture").unwrap();
    let mut manager = Manager::open(
        temp.path().join("plugins"),
        Environment {
            workspace: temp.path().display().to_string(),
            ..Environment::default()
        },
    )
    .unwrap();
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = events.clone();
    let control =
        plugin_runtime::InstallControl::new(move |stage| recorded.lock().unwrap().push(stage));
    manager
        .install_with_control(&package, package.manifest.permissions.clone(), &control)
        .unwrap();
    server.join().unwrap();
    assert!(
        events
            .lock()
            .unwrap()
            .contains(&plugin_runtime::InstallStage::Verifying("server".into()))
    );
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(manager.language_services()["private-analysis/analysis"].is_ok());
}

/// Garbage collection must distinguish a removed installation from a still-borrowed service version.
#[test]
fn another_workspace_lease_protects_uninstalled_dependency_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("plugins");
    let env = Environment {
        workspace: temp.path().display().to_string(),
        ..Environment::default()
    };
    let package = package(json!({"kind":"package", "path":"tool.exe"}), b"fixture").unwrap();
    let mut first = Manager::open(root.clone(), env.clone()).unwrap();
    first
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let mut second = Manager::open(root, env).unwrap();
    let borrowed = second.language_services()["private-analysis/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    first.uninstall("private-analysis", false).unwrap();
    assert_eq!(first.collect_dependency_cache().unwrap(), 0);
    assert!(borrowed.is_active());
    drop(second);
    drop(borrowed);
    assert_eq!(first.collect_dependency_cache().unwrap(), 1);
}

/// Failed preparation never replaces the installed record or revokes the existing service lease.
#[test]
fn checksum_network_and_cancel_failures_leave_the_old_version_available() {
    use std::io::Read;
    let temp = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        temp.path().join("plugins"),
        Environment {
            workspace: temp.path().display().to_string(),
            ..Environment::default()
        },
    )
    .unwrap();
    let original = package(json!({"kind":"package", "path":"tool.exe"}), b"fixture").unwrap();
    manager
        .install(&original, original.manifest.permissions.clone())
        .unwrap();
    let old = manager.language_services()["private-analysis/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    let corrupt = temp.path().join("corrupt.exe");
    std::fs::write(&corrupt, b"incorrect").unwrap();
    let candidate = package(json!({"kind":"local", "path":corrupt}), b"expected").unwrap();
    assert!(
        manager
            .install(&candidate, candidate.manifest.permissions.clone())
            .unwrap_err()
            .to_string()
            .contains("checksum mismatch")
    );
    for cancel in [false, true] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/dependency", listener.local_addr().unwrap());
        let control = plugin_runtime::InstallControl::default();
        let interrupt = control.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0; 4096];
            stream.read(&mut buffer).unwrap();
            if cancel {
                interrupt.cancel();
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            let _ = stream.write_all(
                b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        });
        let candidate = package(json!({"kind":"url", "url":url}), b"expected").unwrap();
        let error = manager
            .install_with_control(&candidate, candidate.manifest.permissions.clone(), &control)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(if cancel { "cancelled" } else { "503" }),
            "{error}"
        );
        server.join().unwrap();
        assert_eq!(
            manager.installed["private-analysis"].digest,
            original.digest
        );
        assert!(old.is_active());
        assert!(std::sync::Arc::ptr_eq(
            &old,
            manager.language_services()["private-analysis/analysis"]
                .as_ref()
                .unwrap()
        ));
    }
}

/// Recursive plans and escape paths fail at inspection, before any network or installation mutation.
#[test]
fn dependency_cycles_and_escaping_entries_are_rejected() {
    let source = json!({"kind":"package", "path":"tool.exe"});
    assert!(
        package_with(source.clone(), b"fixture", |manifest| {
            manifest["services"]["analysis"]["installation"]["artifacts"][0]["requires"] =
                json!(["server"]);
        })
        .err()
        .unwrap()
        .to_string()
        .contains("cycle")
    );
    assert!(
        package_with(source, b"fixture", |manifest| {
            manifest["services"]["analysis"]["installation"]["executable"] =
                json!("server/../escape.exe");
        })
        .is_err()
    );
}

/// User/project executable overrides bypass preparation, including an offline or unavailable remote source.
#[test]
fn explicit_local_executable_wins_and_bad_explicit_paths_do_not_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        temp.path().join("plugins"),
        Environment {
            workspace: temp.path().display().to_string(),
            ..Environment::default()
        },
    )
    .unwrap();
    let configure = |manifest: &mut Value| {
        manifest["settings"] = json!({"tool":{"title":"Service executable","value_type":{"kind":"string"},"default":"","scope":"project"}});
        manifest["language_servers"][0]["executable_setting"] = json!("tool");
    };
    let original = package_with(
        json!({"kind":"package","path":"tool.exe"}),
        b"fixture",
        configure,
    )
    .unwrap();
    manager
        .install(&original, original.manifest.permissions.clone())
        .unwrap();
    manager
        .update_setting(
            "private-analysis",
            plugin_runtime::plugin_protocol::settings::Scope::User,
            "tool",
            Some(json!(std::env::current_exe().unwrap())),
        )
        .unwrap();
    let offline = package_with(
        json!({"kind":"url","url":"http://127.0.0.1:1/unavailable"}),
        b"uncached",
        configure,
    )
    .unwrap();
    manager
        .install(&offline, offline.manifest.permissions.clone())
        .unwrap();
    assert!(manager.language_services()["private-analysis/analysis"].is_ok());
    manager
        .update_setting(
            "private-analysis",
            plugin_runtime::plugin_protocol::settings::Scope::Project,
            "tool",
            Some(json!(temp.path().join("missing.exe"))),
        )
        .unwrap();
    assert!(
        manager.language_services()["private-analysis/analysis"]
            .as_ref()
            .err()
            .unwrap()
            .contains("Native tool not found")
    );
    assert!(
        manager
            .install(&offline, offline.manifest.permissions.clone())
            .unwrap_err()
            .to_string()
            .contains("Native tool not found")
    );
}

/// Cancellation must work even while another window holds the shared preparation lock during a download.
#[test]
fn waiting_for_another_installation_is_cancellable() {
    use std::io::Read;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("plugins");
    let environment = Environment {
        workspace: temp.path().display().to_string(),
        ..Environment::default()
    };
    let first = Manager::open(root.clone(), environment.clone()).unwrap();
    let mut second = Manager::open(root, environment).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/tool", listener.local_addr().unwrap());
    let package = package(json!({"kind":"url","url":url}), b"fixture").unwrap();
    let (started, ready) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        stream.read(&mut request).unwrap();
        started.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nfixture")
            .unwrap();
    });
    let first_package = package.clone();
    let install = std::thread::spawn(move || {
        let mut first = first;
        first
            .install(&first_package, first_package.manifest.permissions.clone())
            .unwrap();
    });
    ready
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let (waiting, notified) = std::sync::mpsc::channel();
    let control = plugin_runtime::InstallControl::new(move |stage| {
        if stage == plugin_runtime::InstallStage::Preparing {
            let _ = waiting.send(());
        }
    });
    let cancel = control.clone();
    let second_install = std::thread::spawn(move || {
        second.install_with_control(&package, package.manifest.permissions.clone(), &control)
    });
    notified
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let begin = std::time::Instant::now();
    cancel.cancel();
    assert!(
        second_install
            .join()
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert!(begin.elapsed() < std::time::Duration::from_millis(500));
    install.join().unwrap();
    server.join().unwrap();
}

/// Reinstalling an identical WASM package with a different resolved plan must preserve inactive rollback pins.
#[test]
#[ignore = "build capability-example through the host SDK first"]
fn hook_plan_receipts_accumulate_and_dynamic_program_skips_unused_downloads() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&root).unwrap().files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["api"]["required"]["language.lsp"] = json!("^1");
    manifest["api"]["required"]["process"] = json!("^1");
    manifest["api"]["required"]["dependencies"] = json!("^1");
    manifest["permissions"].as_array_mut().unwrap().extend([
        json!("process.service.analysis"),
        json!("process.exec"),
        json!("dependencies.prepare"),
    ]);
    manifest["settings_hook"] = json!(false);
    manifest["settings"]["label"]["default"] = json!("managed-dependency");
    let primary = json!({"executable":"server/tool.exe", "artifacts":[{"id":"server","version":"1","platform":format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH),"sha256":format!("{:x}",Sha256::digest(b"fixture")),"source":{"kind":"package","path":"tool.exe"},"format":{"kind":"file","path":"tool.exe"}}]});
    let mut alternate = primary.clone();
    alternate["artifacts"][0]["version"] = json!("2");
    let mut unused = primary.clone();
    unused["artifacts"][0]["version"] = json!("unavailable");
    unused["artifacts"][0]["source"] = json!({"kind":"url","url":"http://127.0.0.1:1/unused"});
    manifest["services"] =
        json!({"analysis":{"program":std::env::current_exe().unwrap(),"installation":unused}});
    manifest["language_servers"] =
        json!([{"id":"analysis","language":"novel","service":"analysis","hook":true}]);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    files.insert(
        "dependency-plan.json".into(),
        serde_json::to_vec(&primary).unwrap(),
    );
    files.insert(
        "dependency-plan-alternate.json".into(),
        serde_json::to_vec(&alternate).unwrap(),
    );
    files.insert("tool.exe".into(), b"fixture".to_vec());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let package = Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap();
    let mut manager = Manager::open(
        temp.path().join("plugins"),
        Environment {
            workspace: temp.path().display().to_string(),
            ..Environment::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let scope = plugin_runtime::plugin_protocol::settings::Scope::Project;
    manager
        .update_setting(
            "capability-example",
            scope,
            "label",
            Some(json!("managed-dependency-alternate")),
        )
        .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager.disable("capability-example").unwrap();
    assert_eq!(manager.collect_dependency_cache().unwrap(), 0);
    manager.enable("capability-example").unwrap();
    manager
        .update_setting(
            "capability-example",
            scope,
            "label",
            Some(json!("managed-dependency")),
        )
        .unwrap();
    assert!(manager.language_services()["capability-example/analysis"].is_ok());
    manager
        .update_setting(
            "capability-example",
            scope,
            "label",
            Some(json!("dynamic-start")),
        )
        .unwrap();
    // The default installation points to an unavailable URL; native program discovery must bypass it.
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(manager.language_services()["capability-example/analysis"].is_ok());
}
