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
        state.scenes.contains_key("capability-example/welcome")
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
        state.scenes.get("capability-example/welcome").and_then(|scene|scene.ui.as_ref())
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
        state.scenes.get("capability-example/welcome").and_then(|scene|scene.ui.as_ref())
            .is_some_and(|ui| matches!(&ui.root.kind, ui::Kind::Text{text} if text.contains("still-responsive")))
    });
    let old_epoch = worker.state.lock().unwrap().instance_epochs["capability-example"];
    if restrict {
        worker.tx.send(Work::SetTrust(true)).unwrap();
        worker.tx.send(Work::SetTrust(false)).unwrap();
        // Revocation takes effect while the network is still paused, not after background cleanup.
        wait_for(&worker, |state| state.scenes.is_empty());
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
                .is_some_and(|message| message.contains("Plugin is not running"))
        });
        let state = worker.state.lock().unwrap();
        assert!(state.scenes.is_empty());
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
                Event::Command {
                    id: "scope-write".into(),
                    cwd: None,
                    text: None,
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
            state.scenes.get("capability-example/welcome").and_then(|scene|scene.ui.as_ref())
            .is_some_and(|ui| matches!(&ui.root.kind, ui::Kind::Text{text} if text == "workspace|during preparation"))
        });
    }
    // Shutdown acknowledgement waits for owner cleanup, so this test never races temp directory deletion.
    let (ack, finished) = futures::channel::oneshot::channel();
    worker.tx.send(Work::Shutdown(Some(ack))).unwrap();
    futures::executor::block_on(finished).unwrap();
}
