//! The production actor consumes a registry-produced fixture served over real loopback HTTP.
use super::*;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Bounded fixture server lets tests observe attempted package downloads and catalog outages.
struct Server {
    url: String,
    downloads: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
    offline: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    task: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        Self::from_bytes(
            include_bytes!("../../../../../plugin-runtime/tests/fixtures/marketplace/catalog.json"),
            include_bytes!("../../../../../plugin-runtime/tests/fixtures/marketplace/notes.zip")
                .to_vec(),
        )
    }
    fn from_bytes(catalog_bytes: &[u8], package: Vec<u8>) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let mut catalog: serde_json::Value = serde_json::from_slice(catalog_bytes).unwrap();
        catalog["plugins"][0]["versions"][0]["url"] = format!("{url}/notes.zip").into();
        let catalog = serde_json::to_vec(&catalog).unwrap();
        let downloads = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let request_count = requests.clone();
        let offline = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let pause = Arc::new(AtomicBool::new(false));
        let paused = pause.clone();
        let (count, failed, stopped) = (downloads.clone(), offline.clone(), stop.clone());
        let task = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        // Windows accepts may inherit the listener's nonblocking flag; requests arrive later.
                        socket.set_nonblocking(false).unwrap();
                        request_count.fetch_add(1, Ordering::AcqRel);
                        socket
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut request = [0; 4096];
                        let count_read = socket.read(&mut request).unwrap();
                        let zip =
                            String::from_utf8_lossy(&request[..count_read]).contains("/notes.zip");
                        let bytes: &[u8] = if zip {
                            count.fetch_add(1, Ordering::AcqRel);
                            &package
                        } else {
                            &catalog
                        };
                        let status = if !zip && failed.load(Ordering::Acquire) {
                            "503 Unavailable"
                        } else {
                            "200 OK"
                        };
                        let _ = write!(
                            socket,
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            bytes.len()
                        );
                        while zip
                            && paused.load(Ordering::Acquire)
                            && !stopped.load(Ordering::Acquire)
                        {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        let _ = socket.write_all(bytes);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("fixture server: {error}"),
                }
            }
        });
        Self {
            url,
            downloads,
            requests,
            offline,
            pause,
            stop,
            task: Some(task),
        }
    }
}

/// The catalog may arrive before manager recovery; a genuine unstarted actor cannot authorize installs.
#[test]
fn marketplace_waits_for_initial_installed_records() {
    let server = Server::new();
    let home = tempfile::tempdir().unwrap();
    let worker = Worker::start(home.path().join("runtime"), Environment::default(), true);
    worker.refresh_market(
        home.path().join("cache.json"),
        format!("{}/catalog.json", server.url),
    );
    wait(&worker, |s| s.market.fresh());
    let (release, generation) = {
        let state = worker.state.lock().unwrap();
        assert!(!state.ready);
        (
            state.market.catalog.as_ref().unwrap().plugins[0].versions[0].clone(),
            state.market.generation(),
        )
    };
    assert!(!worker.install_market(release, generation));
    assert_eq!(server.downloads.load(Ordering::Acquire), 0);
    assert!(worker.recorded.lock().unwrap().try_recv().is_err());
}

/// The controlled guest is built by the host SDK entry, then exported by the registry builder.
#[test]
#[ignore = "build capability-example with --plugin-package and export target/marketplace-wasm via marketplace tests.export_wasm_fixture"]
fn marketplace_installs_actual_wasm_through_background_preparation() {
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/marketplace-wasm");
    let server = Server::from_bytes(
        &std::fs::read(fixture.join("catalog.json")).expect("export current WASM catalog"),
        std::fs::read(fixture.join("notes.zip")).expect("export current WASM ZIP"),
    );
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("runtime");
    let worker = Worker::start_background(
        root.clone(),
        Environment {
            workspace: home.path().to_string_lossy().into_owned(),
            ..Environment::default()
        },
        true,
    );
    wait(&worker, |s| s.ready);
    worker.refresh_market(
        root.join("marketplace/catalog.json"),
        format!("{}/catalog.json", server.url),
    );
    wait(&worker, |s| s.market.fresh());
    let (candidate, generation) = {
        let s = worker.state.lock().unwrap();
        (
            s.market.catalog.as_ref().unwrap().plugins[0].versions[0].clone(),
            s.market.generation(),
        )
    };
    let id = candidate.manifest.id.clone();
    assert!(candidate.manifest.component.is_some());
    assert!(worker.install_market(candidate.clone(), generation));
    wait(&worker, |s| s.progress.is_none());
    let state = worker.state.lock().unwrap();
    assert!(
        state.status.is_none(),
        "{:?}",
        state.status.as_ref().map(|s| &s.message)
    );
    assert!(
        state.instance_epochs.contains_key(&id),
        "actual guest must become live"
    );
    assert!(Manager::read_registry(&root).unwrap().contains_key(&id));
    drop(state);
    worker.tx.send(Work::Shutdown(None)).unwrap();
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.task.take().unwrap().join().unwrap();
    }
}

fn wait(worker: &Worker, predicate: impl Fn(&Published) -> bool) {
    let end = Instant::now() + Duration::from_secs(60);
    while !predicate(&worker.state.lock().unwrap()) {
        assert!(
            Instant::now() < end,
            "marketplace publication timed out: {:?}",
            {
                let state = worker.state.lock().unwrap();
                (
                    state.market.error.clone(),
                    state.status.as_ref().map(|s| s.message.clone()),
                    state.installation.clone(),
                )
            }
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Each failure traverses catalog admission, consent/download and the actor; old installations survive.
#[test]
fn marketplace_failed_downloads_and_restricted_workspaces_preserve_existing_installation() {
    for failure in ["digest", "manifest", "cancel", "restricted"] {
        let mut catalog: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../../../plugin-runtime/tests/fixtures/marketplace/catalog.json"
        ))
        .unwrap();
        match failure {
            "digest" => catalog["plugins"][0]["versions"][0]["sha256"] = "0".repeat(64).into(),
            "manifest" => {
                catalog["plugins"][0]["versions"][0]["manifest"]["name"] = "Forged display".into()
            }
            _ => {}
        }
        let bytes =
            include_bytes!("../../../../../plugin-runtime/tests/fixtures/marketplace/notes.zip");
        let server = Server::from_bytes(&serde_json::to_vec(&catalog).unwrap(), bytes.to_vec());
        server.pause.store(failure == "cancel", Ordering::Release);
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("runtime");
        let environment = Environment {
            workspace: home.path().to_string_lossy().into_owned(),
            ..Environment::default()
        };
        // Seed an unrelated installed plugin through the same public manager, never a publication stub.
        let mut files = Package::from_bytes(bytes).unwrap().files;
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&files["manifest.json"]).unwrap();
        manifest["id"] = "existing.notes".into();
        files.insert(
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        files.insert(
            "plugin.toml".into(),
            String::from_utf8(files["plugin.toml"].clone())
                .unwrap()
                .replace("test.notes", "existing.notes")
                .into_bytes(),
        );
        let existing = Package::from_files(files).unwrap();
        let mut manager = Manager::open(root.clone(), environment.clone()).unwrap();
        manager
            .install(&existing, existing.manifest.permissions.clone())
            .unwrap();
        manager.shutdown();
        drop(manager);
        let worker = Worker::start_background(root.clone(), environment, failure != "restricted");
        wait(&worker, |s| s.ready);
        worker.refresh_market(
            root.join("marketplace/catalog.json"),
            format!("{}/catalog.json", server.url),
        );
        wait(&worker, |s| s.market.fresh());
        let (candidate, generation) = {
            let s = worker.state.lock().unwrap();
            (
                s.market.catalog.as_ref().unwrap().plugins[0].versions[0].clone(),
                s.market.generation(),
            )
        };
        assert_eq!(
            worker.install_market(candidate, generation),
            failure != "restricted"
        );
        if failure == "cancel" {
            wait(&worker, |_| server.downloads.load(Ordering::Acquire) > 0);
            worker.cancel_installation();
        }
        if failure != "restricted" {
            wait(&worker, |s| s.progress.is_none() && s.status.is_some());
        }
        let installed = Manager::read_registry(&root).unwrap();
        assert_eq!(installed.len(), 1, "{failure}");
        assert_eq!(installed["existing.notes"].digest, existing.digest);
        if failure == "restricted" {
            assert_eq!(server.downloads.load(Ordering::Acquire), 0);
        }
        worker.tx.send(Work::Shutdown(None)).unwrap();
    }
}

/// No package bytes before consent; failure blocks new installs even when the package URL still works.
#[test]
fn marketplace_refresh_consent_install_and_offline_gate_use_the_real_actor() {
    let server = Server::new();
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("runtime");
    let worker = Worker::start_background(
        root.clone(),
        Environment {
            workspace: home.path().to_string_lossy().into_owned(),
            ..Environment::default()
        },
        true,
    );
    wait(&worker, |s| s.ready);
    let cache = root.join("marketplace/catalog.json");
    worker.refresh_market(cache.clone(), format!("{}/catalog.json", server.url));
    wait(&worker, |s| s.market.fresh());
    assert_eq!(server.downloads.load(Ordering::Acquire), 0);
    let (candidate, generation) = {
        let s = worker.state.lock().unwrap();
        (
            s.market.catalog.as_ref().unwrap().plugins[0].versions[0].clone(),
            s.market.generation(),
        )
    };
    server.offline.store(true, Ordering::Release);
    worker.refresh_market(cache.clone(), format!("{}/catalog.json", server.url));
    wait(&worker, |s| s.market.error.is_some());
    assert!(!worker.install_market(candidate.clone(), generation));
    assert_eq!(server.downloads.load(Ordering::Acquire), 0);
    server.offline.store(false, Ordering::Release);
    worker.refresh_market(cache, format!("{}/catalog.json", server.url));
    wait(&worker, |s| s.market.fresh());
    let generation = worker.state.lock().unwrap().market.generation();
    assert!(worker.install_market(candidate, generation));
    wait(&worker, |s| s.progress.is_none());
    let state = worker.state.lock().unwrap();
    assert!(
        state.status.is_none(),
        "{:?}",
        state.status.as_ref().map(|s| &s.message)
    );
    assert!(state.entries.iter().any(|e| e.manifest.id == "test.notes"));
    drop(state);
    assert!(
        Manager::read_registry(&root)
            .unwrap()
            .contains_key("test.notes")
    );
    assert_eq!(server.downloads.load(Ordering::Acquire), 1);
    let candidate = worker
        .state
        .lock()
        .unwrap()
        .market
        .catalog
        .as_ref()
        .unwrap()
        .plugins[0]
        .versions[0]
        .clone();
    assert!(
        !worker.install_market(candidate, generation),
        "market cannot reinstall an existing ID"
    );
    worker.tx.send(Work::Shutdown(None)).unwrap();
}

/// Both palettes and locales use actual pointer consent, then observe the actor's real installation.
#[gpui_kit::gpui::test]
fn marketplace_native_consent_and_install_in_both_locales(cx: &mut gpui_kit::TestAppContext) {
    use crate::*;
    use gpui_kit::VisualTestContext;
    let previous = rust_i18n::locale().to_string();
    for (locale, dark) in [("zh-CN", false), ("en", true)] {
        rust_i18n::set_locale(locale);
        cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(dark), cx);
            cx.set_reduce_motion(true);
        });
        // Snapshot markup cannot trigger file/network access before consent, even if its author asks.
        let external = Server::new();
        let mut catalog: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../../../plugin-runtime/tests/fixtures/marketplace/catalog.json"
        ))
        .unwrap();
        let version = &mut catalog["plugins"][0]["versions"][0];
        version["readme"] = format!(
            "![remote]({}/image.png)\n![local](file:///C:/Windows/win.ini)\n\n{}",
            external.url,
            version["readme"].as_str().unwrap()
        )
        .into();
        version["icon"] = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><image href="{}/image.png" width="24" height="24"/><image href="file:///C:/Windows/win.ini" width="24" height="24"/></svg>"#, external.url).into();
        let server = Server::from_bytes(
            &serde_json::to_vec(&catalog).unwrap(),
            include_bytes!("../../../../../plugin-runtime/tests/fixtures/marketplace/notes.zip")
                .to_vec(),
        );
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".runtime-plugin-test");
        let worker = Arc::new(Worker::start_background(
            root.clone(),
            Environment {
                workspace: home.path().to_string_lossy().into_owned(),
                ..Environment::default()
            },
            true,
        ));
        wait(&worker, |s| s.ready);
        worker.refresh_market(
            root.join("marketplace/catalog.json"),
            format!("{}/catalog.json", server.url),
        );
        wait(&worker, |s| s.market.fresh());
        let workspace = Workspace::open(home.path()).unwrap();
        let slot = Rc::new(std::cell::RefCell::new(None));
        let captured = slot.clone();
        let (_, editor_cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = slot.borrow_mut().take().unwrap();
        let before = editor_cx.update(|_, cx| cx.windows());
        let panel = editor_cx.update(|window, cx| {
            let panel = app.read(cx).extensions.clone();
            panel.update(cx, |panel, cx| {
                panel.worker = worker.clone();
                panel.manager_market = true;
                panel.poll(cx);
            });
            app.update(cx, |app, cx| app.toggle_extensions(window, cx));
            // Opening resets selection; switch through the same mode state used by the native tabs.
            panel.update(cx, |panel, cx| {
                panel.manager_market = true;
                cx.notify();
            });
            panel
        });
        let window = editor_cx
            .update(|_, cx| cx.windows())
            .into_iter()
            .find(|w| !before.contains(w))
            .unwrap();
        let form = VisualTestContext::from_window(window, editor_cx).into_mut();
        form.run_until_parked();
        form.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(server.downloads.load(Ordering::Acquire), 0);
        assert_eq!(
            external.requests.load(Ordering::Acquire),
            0,
            "browsing cannot resolve external images"
        );
        let source = form.debug_bounds("market-source").unwrap();
        form.simulate_click(source.center(), Default::default());
        assert_eq!(
            form.opened_url().as_deref(),
            Some("https://github.com/author/notes")
        );
        let feedback = form.debug_bounds("market-feedback").unwrap();
        form.simulate_click(feedback.center(), Default::default());
        assert_eq!(
            form.opened_url().as_deref(),
            Some("https://github.com/author/notes/issues")
        );
        let search = form.debug_bounds("market-search").unwrap();
        form.simulate_click(search.center(), Default::default());
        form.simulate_input("未匹配 插件");
        form.run_until_parked();
        form.update(|window, cx| window.draw(cx).clear(cx));
        assert!(form.debug_bounds("market-install").is_none());
        form.simulate_keystrokes("ctrl-a");
        form.simulate_input("Notes");
        form.run_until_parked();
        form.update(|window, cx| window.draw(cx).clear(cx));
        let viewport = form.debug_bounds("online-market-details").unwrap();
        form.simulate_event(gpui_kit::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-600.))),
            touch_phase: gpui_kit::TouchPhase::Moved,
            modifiers: Default::default(),
        });
        form.run_until_parked();
        form.update(|window, cx| window.draw(cx).clear(cx));
        assert!(form.update(|_, cx| panel.read(cx).manager_detail_scroll.offset().y < px(0.)));
        form.simulate_event(gpui_kit::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(600.))),
            touch_phase: gpui_kit::TouchPhase::Moved,
            modifiers: Default::default(),
        });
        form.simulate_scale_factor_change(1.5);
        form.run_until_parked();
        form.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = form
            .debug_bounds("market-install")
            .expect("native install button");
        form.simulate_click(bounds.center(), Default::default());
        form.run_until_parked();
        form.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            server.downloads.load(Ordering::Acquire),
            0,
            "opening consent cannot download"
        );
        let bounds = form
            .debug_bounds("confirm-market-install")
            .expect("permission confirmation");
        form.simulate_click(bounds.center(), Default::default());
        wait(&worker, |s| s.progress.is_none());
        form.update(|_, cx| panel.update(cx, |panel, cx| panel.poll(cx)));
        assert!(form.update(|_, cx| {
            panel
                .read(cx)
                .entries
                .iter()
                .any(|e| e.manifest.id == "test.notes")
        }));
        worker.tx.send(Work::Shutdown(None)).unwrap();
        assert_eq!(external.requests.load(Ordering::Acquire), 0);
    }
    rust_i18n::set_locale(&previous);
}
