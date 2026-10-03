//! Exercise the real actor and its published request channel while a local dependency source is paused.
use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

/// Native publications are the same seam the editor polls; no second manager services these commands.
fn wait_for(worker: &Worker, ready: impl Fn(&Published) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if ready(&worker.state.lock().unwrap()) {
            return;
        }
        assert!(Instant::now() < deadline, "worker publication timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Production dispatch retains a command target while inspection failures remain manager-wide.
#[test]
fn operation_errors_retain_plugin_ownership_through_worker_publication() {
    let directory = tempfile::tempdir().unwrap();
    let worker = Worker::start_background(
        directory.path().join("plugins"),
        Environment::default(),
        true,
    );
    worker
        .tx
        .send(Work::Invoke {
            plugin: "missing-plugin".into(),
            command: "unknown".into(),
            arguments: json!(null),
        })
        .unwrap();
    wait_for(&worker, |state| state.status.is_some());
    let status = worker.state.lock().unwrap().status.take().unwrap();
    assert_eq!(status.plugin.as_deref(), Some("missing-plugin"));
    assert!(status.message.contains("Plugin is not running"));
    let logs = worker.state.lock().unwrap().logs.clone();
    assert!(logs.records("missing-plugin").iter().any(|record| {
        record.level == plugin_runtime::logs::LogLevel::Error && record.source == "host.operation"
    }));
    worker.queue_lifecycle(Work::Inspect(directory.path().join("missing.zip")));
    wait_for(&worker, |state| state.status.is_some());
    assert!(
        worker
            .state
            .lock()
            .unwrap()
            .status
            .as_ref()
            .unwrap()
            .plugin
            .is_none()
    );
    let (tx, rx) = futures::channel::oneshot::channel();
    worker.tx.send(Work::Shutdown(Some(tx))).unwrap();
    futures::executor::block_on(rx).unwrap();
}

/// Obsolete native revisions and incarnations remain rejected without faulting a healthy real guest.
#[test]
#[ignore = "build capability-example through the public SDK first"]
fn obsolete_ui_callbacks_are_information_but_real_rejections_are_errors() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    let environment = Environment {
        workspace: directory.path().display().to_string(),
        ..Default::default()
    };
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let mut manager = Manager::open(root.clone(), environment.clone()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    drop(manager);
    let worker = Worker::start_background(root, environment, true);
    wait_for(&worker, |state| {
        state.views.contains_key("capability-example/welcome")
    });
    let (logs, epoch, revision) = {
        let state = worker.state.lock().unwrap();
        (
            state.logs.clone(),
            state.instance_epochs["capability-example"],
            state.views["capability-example/welcome"].revision,
        )
    };
    worker
        .tx
        .send(Work::Event(
            "capability-example".into(),
            epoch,
            Some("welcome".into()),
            api::Notification::Ui(ui::UiEvent {
                revision: revision.wrapping_add(1),
                node: "obsolete".into(),
                action: ui::Action::Click,
            }),
        ))
        .unwrap();
    wait_for(&worker, |_| {
        logs.records("capability-example")
            .iter()
            .any(|entry| entry.source == "host.ui.stale")
    });
    assert_eq!(logs.unread_severity("capability-example"), None);
    assert!(worker.state.lock().unwrap().status.is_none());
    worker
        .tx
        .send(Work::Event(
            "capability-example".into(),
            epoch.wrapping_add(1),
            Some("welcome".into()),
            api::Notification::Ui(ui::UiEvent {
                revision,
                node: "obsolete".into(),
                action: ui::Action::Click,
            }),
        ))
        .unwrap();
    wait_for(&worker, |_| {
        logs.records("capability-example")
            .iter()
            .any(|entry| entry.source == "host.ui.retired")
    });
    assert_eq!(logs.unread_severity("capability-example"), None);
    // A current callback to an invalid node is a real error; typed stale handling cannot swallow it.
    worker
        .tx
        .send(Work::Event(
            "capability-example".into(),
            epoch,
            Some("welcome".into()),
            api::Notification::Ui(ui::UiEvent {
                revision,
                node: "missing-node".into(),
                action: ui::Action::Click,
            }),
        ))
        .unwrap();
    wait_for(&worker, |state| state.status.is_some());
    assert_eq!(
        logs.unread_severity("capability-example"),
        Some(plugin_runtime::logs::LogLevel::Error)
    );
    assert!(
        worker
            .state
            .lock()
            .unwrap()
            .views
            .contains_key("capability-example/welcome")
    );
    let (tx, rx) = futures::channel::oneshot::channel();
    worker.tx.send(Work::Shutdown(Some(tx))).unwrap();
    futures::executor::block_on(rx).unwrap();
}

/// Both old-version writes and typed editor completions must progress before the download is released.
#[test]
#[ignore = "build capability-example through the public SDK first"]
fn background_preparation_keeps_old_plugin_commands_and_requests_running() {
    run_preparation(false);
}

/// A grant queued before a later revocation must not revive the old plugin after cancellation finishes.
#[test]
#[ignore = "build capability-example through the public SDK first"]
fn cancelled_preparation_does_not_replay_an_older_trust_grant() {
    run_preparation(true);
}

/// Both paths use a paused HTTP source and the production actor, including real cancellation cleanup.
fn run_preparation(restrict: bool) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    std::fs::write(directory.path().join("source.txt"), "workspace").unwrap();
    let environment = Environment {
        workspace: directory.path().display().to_string(),
        ..Default::default()
    };
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let old = Package::read(&source).unwrap();
    let mut manager = Manager::open(root.clone(), environment.clone()).unwrap();
    manager
        .install(&old, old.manifest.permissions.clone())
        .unwrap();
    drop(manager);
    let worker = Worker::start_background(root, environment, true);
    wait_for(&worker, |state| {
        state.views.contains_key("capability-example/welcome")
    });

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/artifact", listener.local_addr().unwrap());
    let (entered, waiting) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let payload = b"unused preparation fixture";
    let download = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        stream.read(&mut request).unwrap();
        entered.send(()).unwrap();
        // A dropped sender also unblocks this thread when an earlier assertion fails.
        let _ = gate.recv_timeout(Duration::from_secs(30));
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        let _ = stream.write_all(payload);
    });
    let mut files = old.files.clone();
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["version"] = json!("99.0.0");
    manifest["api"]["required"]["dependencies"] = json!("^1");
    manifest["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("dependencies.prepare"));
    manifest["services"] = json!({"prepared-only":{
        "program":"unused-prepared-service", "args":[], "installation":{
            "executable":"artifact/tool.exe", "artifacts":[{
                "id":"artifact", "version":"1.0.0", "platform":format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH),
                "sha256":format!("{:x}",Sha256::digest(payload)),
                "source":{"kind":"url","url":url}, "format":{"kind":"file","path":"tool.exe"}
            }]
        }
    }});
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let update = crate::extensions::language_tests::packages::repack(files).unwrap();
    assert!(worker.queue_lifecycle(Work::Install(update)));
    waiting.recv_timeout(Duration::from_secs(60)).unwrap();
    worker
        .tx
        .send(Work::Invoke {
            plugin: "capability-example".into(),
            command: "scope-write".into(),
            arguments: json!({"text":"during preparation"}),
        })
        .unwrap();
    wait_for(&worker, |state| {
        state.views.get("capability-example/welcome").map(|document| document.as_ref())
            .is_some_and(|ui| matches!(&ui.root.kind, ui::Kind::Text{text} if text == "workspace|during preparation"))
    });
    assert!(worker.state.lock().unwrap().progress.is_some());
    worker
        .tx
        .send(Work::Invoke {
            plugin: "capability-example".into(),
            command: "active-directory".into(),
            arguments: json!(null),
        })
        .unwrap();
    wait_for(&worker, |state| !state.editor_requests.is_empty());
    let request = worker
        .state
        .lock()
        .unwrap()
        .editor_requests
        .pop()
        .unwrap()
        .1;
    assert!(request.begin());
    request.finish(Ok(api::EditorValue::Directory {
        path: "still-responsive".into(),
    }));
    wait_for(&worker, |state| {
        state.views.get("capability-example/welcome").map(|document| document.as_ref())
            .is_some_and(|ui| matches!(&ui.root.kind, ui::Kind::Text{text} if text.contains("still-responsive")))
    });
    let old_epoch = worker.state.lock().unwrap().instance_epochs["capability-example"];
    if restrict {
        worker.tx.send(Work::SetTrust(true)).unwrap();
        worker.tx.send(Work::SetTrust(false)).unwrap();
        // Revocation takes effect while the network is still paused, not after background cleanup.
        wait_for(&worker, |state| state.views.is_empty());
    }
    release.send(()).unwrap();
    download.join().unwrap();
    wait_for(&worker, |state| state.progress.is_none());
    if restrict {
        assert!(
            !worker
                .state
                .lock()
                .unwrap()
                .installation
                .as_ref()
                .unwrap()
                .installed
        );
        worker
            .tx
            .send(Work::Invoke {
                plugin: "capability-example".into(),
                command: "scope-read".into(),
                arguments: json!(null),
            })
            .unwrap();
        // This later command is an ordering barrier after any queued grant; it must remain rejected.
        wait_for(&worker, |state| {
            state
                .status
                .as_ref()
                .is_some_and(|status| status.message.contains("Plugin is not running"))
        });
        let state = worker.state.lock().unwrap();
        assert!(state.views.is_empty());
        assert_eq!(state.entries[0].manifest.version, old.manifest.version);
    } else {
        assert!(
            worker
                .state
                .lock()
                .unwrap()
                .installation
                .as_ref()
                .unwrap()
                .installed
        );
        worker
            .tx
            .send(Work::Event(
                "capability-example".into(),
                old_epoch,
                None,
                api::Notification::Command {
                    id: "scope-write".into(),
                    arguments: Some(json!({"text":"stale callback"})),
                },
            ))
            .unwrap();
        worker
            .tx
            .send(Work::Invoke {
                plugin: "capability-example".into(),
                command: "scope-read".into(),
                arguments: json!(null),
            })
            .unwrap();
        wait_for(&worker, |state| {
            state.views.get("capability-example/welcome").map(|document| document.as_ref())
            .is_some_and(|ui| matches!(&ui.root.kind, ui::Kind::Text{text} if text == "workspace|during preparation"))
        });
    }
    // Shutdown acknowledgement waits for owner cleanup, so this test never races temp directory deletion.
    let (ack, finished) = futures::channel::oneshot::channel();
    worker.tx.send(Work::Shutdown(Some(ack))).unwrap();
    futures::executor::block_on(finished).unwrap();
}
