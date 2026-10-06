//! Shared actual-package transport and native input for file layouts and toolbar acceptance.
use super::*;
use gpui_kit::VisualTestContext;
use plugin_runtime::Manager;
use std::io::{Cursor, Write};

/// Repack one independent component under two identities through the ordinary package validator.
pub(super) fn package(id: &str, name: &str) -> Package {
    let package = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-layout-test/layout-example.zip"),
    )
    .expect("build-layout-example.ps1 first");
    identify(package, id, name)
}

/// Alternate identities retain the actual shipped component and pass the public archive validator.
pub(super) fn identify(package: Package, id: &str, name: &str) -> Package {
    let mut files = package.files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = id.into();
    manifest["name"] = name.into();
    if let Some(path) = manifest["contributions"].as_str() {
        // Hybrid packages declare the same owner in both manifests; preserve artwork and other
        // contributions while changing identity instead of weakening the public admission checks.
        let source = std::str::from_utf8(&files[path]).unwrap();
        let mut contributions: toml::Value = toml::from_str(source).unwrap();
        contributions["plugin"]["id"] = toml::Value::String(id.into());
        contributions["plugin"]["name"] = toml::Value::String(name.into());
        files.insert(
            path.into(),
            toml::to_string(&contributions).unwrap().into_bytes(),
        );
    }
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

/// Execute the existing native transport, then publish Manager-owned immutable views at its real boundary.
pub(super) fn pump(app: &Entity<EditorApp>, manager: &mut Manager, cx: &mut App) {
    let owner = app.read(cx).extensions.clone();
    let work = owner
        .read(cx)
        .worker
        .recorded
        .lock()
        .unwrap()
        .try_iter()
        .collect::<Vec<_>>();
    for work in work {
        let work = match work {
            Work::ImportPreference {
                plugin,
                owner,
                workspace,
                key,
                data,
                ..
            } => {
                let succeeded = manager
                    .import_preference(&plugin, key, data.clone())
                    .is_ok();
                owner_receipt(
                    &owner,
                    &workspace,
                    data,
                    succeeded,
                    &app.read(cx).extensions.read(cx).worker,
                );
                continue;
            }
            work => work,
        };
        if let Work::Event(id, _, panel, event) = work {
            // A direct lifecycle call can retire the guest between native frames; its queued callbacks
            // lose authority just as production callbacks do when publication advances the instance epoch.
            if manager.instance_id(&id).is_none() {
                continue;
            }
            let result = manager.event(&id, panel, event);
            assert!(
                result.is_ok()
                    || result
                        .as_ref()
                        .err()
                        .and_then(|e| e.downcast_ref::<protocol::api::Failure>())
                        .is_some_and(|e| e.code == protocol::api::ErrorCode::StaleRevision),
                "{result:?}"
            );
        }
    }
    let views = manager
        .live
        .iter()
        .flat_map(|(id, live)| {
            live.views
                .iter()
                .map(move |(panel, view)| (format!("{id}/{panel}"), view.clone()))
        })
        .collect();
    let until = Instant::now() + Duration::from_secs(5);
    let resources = loop {
        let resources = manager.image_resources();
        if resources
            .values()
            .all(|image| !matches!(image.state, plugin_runtime::ImageState::Loading))
        {
            break resources;
        }
        assert!(Instant::now() < until, "file image did not finish");
        std::thread::sleep(Duration::from_millis(5));
    };
    let pixels = images::VectorRenderer::default().prepare_resources(&views, &resources);
    owner.update(cx, |owner, cx| {
        let mut state = owner.worker.state.lock().unwrap();
        state.entries = manager.installed.values().cloned().collect();
        state.views = views;
        state.images = pixels;
        drop(state);
        owner.poll(cx);
    });
    let panels = app
        .read(cx)
        .plugin_panels
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for panel in panels {
        panel.update(cx, |panel, cx| panel.poll(cx));
    }
}
/// Native harnesses acknowledge actual Manager writes; they never synthesize stored intent.
pub(super) fn owner_receipt(
    owner: &str,
    workspace: &str,
    data: serde_json::Value,
    succeeded: bool,
    worker: &Worker,
) {
    worker
        .state
        .lock()
        .unwrap()
        .preference_imports
        .push(worker::PreferenceImport {
            owner: owner.into(),
            workspace: workspace.into(),
            data,
            succeeded,
        });
}

/// Flush two ordinary render/notification turns; resize publications settle without private test APIs.
pub(super) fn draw(app: &Entity<EditorApp>, manager: &mut Manager, cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| {
            pump(app, manager, cx);
            window.draw(cx).clear(cx);
        });
    }
}

/// Select from the actual file Tab popup, capturing the same native context used in production.
pub(super) fn choose(
    index: usize,
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    cx: &mut VisualTestContext,
) {
    let active = cx.update(|_, cx| app.read(cx).active_tab_index().unwrap());
    let tab = cx
        .debug_bounds(["editor-tab-0", "editor-tab-1", "editor-tab-2"][active])
        .unwrap()
        .center();
    cx.simulate_mouse_down(tab, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(tab, MouseButton::Right, Modifiers::default());
    // Drain tab activation's deferred focus before exercising the popup's keyboard owner.
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.update(|_, cx| app.read(cx).file_view_menu.is_none()));
    cx.simulate_mouse_down(tab, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(tab, MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let steps = cx.update(|_, cx| {
        let items = &app.read(cx).file_view_menu.as_ref().unwrap().read(cx).items;
        assert!(!items[index].disabled);
        items
            .iter()
            .take(index)
            .filter(|item| !item.disabled)
            .count()
    });
    // Navigation skips disabled candidates just as the ordinary native popup does.
    cx.simulate_keystrokes(&format!("home {}enter", "down ".repeat(steps)));
    draw(app, manager, cx);
}
