//! A first-use consent drives the production actor, then draws its genuine preparation failure in native UI.
use super::{Fixture, Manager, Package, Work, Worker};
use crate::extensions::ExtensionPanel;
use crate::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use std::cell::RefCell;
use std::io::{Cursor, Write};

/// A valid component header passes package inspection, but intentionally contains no public SDK exports.
fn empty_component_archive(resource_bytes: &[u8]) -> Vec<u8> {
    let mut files = Package::from_bytes(resource_bytes).unwrap().files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["component"] = serde_json::json!("empty.wasm");
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    files.insert("empty.wasm".into(), b"\0asm\x0d\0\x01\0".to_vec());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    assert!(
        Package::from_bytes(&bytes)
            .unwrap()
            .manifest
            .component
            .is_some(),
        "the fixture must reach normal installation rather than fail ZIP inspection"
    );
    bytes
}

/// Actor teardown must finish before the temporary profile is removed, including the expected RED panic.
struct RunningActor(Arc<Worker>);

impl Drop for RunningActor {
    fn drop(&mut self) {
        let (send, mut acknowledgement) = futures::channel::oneshot::channel();
        if self.0.tx.send(Work::Shutdown(Some(send))).is_err() {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while matches!(acknowledgement.try_recv(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Poll the genuine actor publication through the ordinary panel, then draw normal Root overlays and status.
fn publish_frame(app: &Entity<EditorApp>, ui: &mut VisualTestContext) {
    ui.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |panel, cx| panel.poll(cx));
        app.update(cx, |_, cx| cx.notify());
        window.draw(cx).clear(cx);
    });
    ui.run_until_parked();
    ui.update(|window, cx| window.draw(cx).clear(cx));
}

/// Native waits remain bounded while real WASM preparation runs independently of GPUI's test executor.
fn wait_for_native(
    app: &Entity<EditorApp>,
    ui: &mut VisualTestContext,
    predicate: impl Fn(&ExtensionPanel) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        publish_frame(app, ui);
        if ui.update(|_, cx| predicate(app.read(cx).extensions.read(cx))) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "native first-use actor publication timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A real failed first install has no installed row; its owned error still needs a main-window indicator/detail.
#[gpui::test]
fn first_use_actor_preparation_failure_is_visible_in_native_status(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let mut fixture = Fixture::new();
    fixture.root = fixture.workspace.join(".runtime-plugin-test");
    fixture.bytes = empty_component_archive(&fixture.bytes);
    fixture.catalog("prose", "generic.zip", None);
    let workspace = Workspace::open(&fixture.workspace).unwrap();
    let mut session = crate::app::session::SessionState::load(workspace.root());
    session.workspace_trusted = true;
    session.save();
    let worker = Arc::new(Worker::start_background(
        fixture.root.clone(),
        fixture.environment(),
        true,
    ));
    let _actor = RunningActor(worker.clone());
    super::wait_for(&worker, |state| state.ready);
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let transport = worker.clone();
    let (_, ui) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        let owner = app.read(cx).extensions.clone();
        // Replace only the test recording transport with the actual actor on this same isolated profile.
        // All package inspection, consent, preparation, errors and polling retain their production routes.
        owner.update(cx, |panel, cx| {
            panel.worker = transport;
            panel.poll(cx);
        });
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    ui.update(|window, _| window.activate_window());
    ui.run_until_parked();
    ui.simulate_resize(size(px(1400.), px(900.)));
    ui.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_file(fixture.file.clone(), window, cx)
        })
    });
    wait_for_native(&app, ui, |panel| panel.bundled.dialog_open);
    assert!(
        ui.debug_bounds("plugin-install-consent").is_some(),
        "first use must draw real native permissions before any preparation"
    );
    assert!(worker.state.lock().unwrap().entries.is_empty());
    assert!(ui.debug_bounds("plugin-error-indicator").is_none());
    let confirm = ui
        .debug_bounds("plugin-install-confirm")
        .expect("native installation confirmation");
    ui.simulate_click(confirm.center(), Default::default());
    wait_for_native(&app, ui, |panel| {
        panel.progress.is_none() && panel.status.is_some()
    });
    let failure = ui.update(|_, cx| app.read(cx).extensions.read(cx).status.clone().unwrap());
    assert_eq!(failure.plugin.as_deref(), Some("bundled-prose"));
    assert!(
        failure.message.to_ascii_lowercase().contains("export"),
        "the real SDK-export preparation failure must reach native publication: {}",
        failure.message
    );
    {
        let state = worker.state.lock().unwrap();
        assert!(
            state.entries.is_empty() && state.views.is_empty() && state.processes.is_empty(),
            "failed preparation cannot publish an installed or live plugin"
        );
        assert!(state.pending.is_none());
    }
    assert!(Manager::read_registry(&fixture.root).unwrap().is_empty());
    let indicator = ui
        .debug_bounds("plugin-error-indicator")
        .expect("a genuine first-install failure needs the main-window error indicator");
    ui.simulate_click(indicator.center(), Default::default());
    publish_frame(&app, ui);
    assert!(ui.debug_bounds("plugin-status-popup").is_some());
    assert!(
        ui.debug_bounds("plugin-manager-status-detail").is_some(),
        "the status popup must draw the actual preparation failure without opening plugin management"
    );
}
