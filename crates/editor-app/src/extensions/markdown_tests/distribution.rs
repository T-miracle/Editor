//! Delivered-package native consent and lifecycle regressions use an isolated fresh fixture.
use super::*;
mod fixture;
use fixture::{Fresh, archive, catalog, delivered, independent_preview};

/// The first trusted file opens consent before WASM and hot-enables its source grammar and preview.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn fresh_markdown_first_use_confirms_real_package_and_retains_disabled_choice(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) = Fresh::mount(cx, true, &delivered(), "md");
    fixture.open("notes.md", ui);
    assert!(fixture.consent(ui));
    assert!(fixture.native.manager.installed.is_empty() && fixture.native.manager.live.is_empty());
    assert!(ui.debug_bounds("editor-plugin-layout").is_none());
    assert!(ui.debug_bounds("editor-source-toolbar").is_none());
    assert!(
        !fixture
            .native
            .manager
            .has_bundle_choice("markdown")
            .unwrap(),
        "an offer is not an explicit user refusal"
    );
    fixture.click_consent("plugin-install-confirm", ui);
    fixture.settle(ui);
    assert_eq!(
        fixture.native.manager.installed["markdown"]
            .manifest
            .version,
        "0.16.0"
    );
    assert!(ui.debug_bounds("editor-plugin-layout").is_some());
    assert!(ui.debug_bounds("editor-source-toolbar").is_some());
    assert!(
        ui.debug_bounds("plugin-tool-markdown/preview/display-sync")
            .is_some()
    );
    assert_eq!(
        ui.update(|_, cx| fixture
            .native
            .app
            .read(cx)
            .editor
            .read(cx)
            .language_name()
            .to_string()),
        "markdown"
    );
    fixture.native.manager.disable("markdown").unwrap();
    fixture.settle(ui);
    assert!(ui.debug_bounds("editor-plugin-layout").is_none());
    let (mut fixture, ui) = fixture.reopen(cx);
    fixture.open("notes.md", ui);
    assert!(!fixture.native.manager.installed["markdown"].enabled);
    assert!(!fixture.native.manager.live.contains_key("markdown"));
    assert!(!fixture.consent(ui));
}

/// Declining the real prompt remains effective after restarting and replacing the catalog's ZIP hash.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn fresh_markdown_cancel_survives_restart_and_changed_shipped_hash(cx: &mut TestAppContext) {
    let (mut fixture, ui) = Fresh::mount(cx, true, &delivered(), "md");
    fixture.open("notes.md", ui);
    fixture.click_consent("plugin-install-cancel", ui);
    fixture.settle(ui);
    assert!(fixture.native.manager.installed.is_empty());
    let mut files = Package::from_bytes(&delivered()).unwrap().files;
    files.insert(
        "README.md".into(),
        b"A changed shipped package must retain the user choice.".to_vec(),
    );
    let changed = language_tests::packages::repack(files).unwrap();
    assert_ne!(
        changed.digest,
        Package::from_bytes(&delivered()).unwrap().digest
    );
    let workspace = Workspace::open(fixture.native.directory.path()).unwrap();
    catalog(workspace.root(), &archive(&changed), "md");
    let (mut fixture, ui) = fixture.reopen(cx);
    fixture.open("notes.md", ui);
    assert!(!fixture.consent(ui));
    assert!(fixture.native.manager.installed.is_empty() && fixture.native.manager.live.is_empty());
}

/// Trust gates both inspection and a previously confirmed but not yet executed installation.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn fresh_markdown_restricted_workspace_and_revoked_consent_never_execute_guest(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) = Fresh::mount(cx, false, &delivered(), "md");
    fixture.open("notes.md", ui);
    assert!(!fixture.consent(ui));
    assert!(
        !fixture
            .native
            .manager
            .has_bundle_choice("markdown")
            .unwrap()
    );
    ui.update(|_, cx| {
        fixture
            .native
            .app
            .update(cx, |app, cx| app.set_workspace_trusted(true, cx))
    });
    fixture.settle(ui);
    assert!(fixture.consent(ui));
    fixture.click_consent("plugin-install-confirm", ui);
    ui.update(|_, cx| {
        fixture
            .native
            .app
            .update(cx, |app, cx| app.set_workspace_trusted(false, cx))
    });
    fixture.settle(ui);
    assert!(fixture.native.manager.installed.is_empty() && fixture.native.manager.live.is_empty());
    assert!(ui.debug_bounds("editor-plugin-layout").is_none());
}

/// A queued candidate cannot install into a document that has been switched away or closed.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn fresh_markdown_late_confirm_is_withdrawn_on_switch_and_close(cx: &mut TestAppContext) {
    for close in [false, true] {
        let (mut fixture, ui) = Fresh::mount(cx, true, &delivered(), "md");
        fixture.open("notes.md", ui);
        fixture.click_consent("plugin-install-confirm", ui);
        ui.update(|window, cx| {
            fixture.native.app.update(cx, |app, cx| {
                if close {
                    app.close_tab(app.active_path.clone().unwrap(), window, cx);
                } else {
                    app.open_file(
                        fixture.native.directory.path().join("other.txt"),
                        window,
                        cx,
                    );
                }
            })
        });
        fixture.settle(ui);
        assert!(
            fixture.native.manager.installed.is_empty() && fixture.native.manager.live.is_empty()
        );
        assert!(ui.debug_bounds("editor-plugin-layout").is_none());
    }
}

/// An existing independently named language provider must survive without any default installation.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn fresh_markdown_keeps_alternative_source_provider(cx: &mut TestAppContext) {
    let (mut fixture, ui) = Fresh::mount(cx, true, &delivered(), "md");
    let mut files = language_package().files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = serde_json::json!("alternate-prose");
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let declaration = String::from_utf8(files["plugin.toml"].clone())
        .unwrap()
        .replacen("id = \"markdown\"", "id = \"alternate-prose\"", 1);
    files.insert("plugin.toml".into(), declaration.into_bytes());
    let alternate = language_tests::packages::repack(files).unwrap();
    fixture
        .native
        .manager
        .install(&alternate, alternate.manifest.permissions.clone())
        .unwrap();
    language_tests::publish_languages(&fixture.native.app, &fixture.native.manager, ui);
    fixture.open("notes.md", ui);
    language_tests::publish_languages(&fixture.native.app, &fixture.native.manager, ui);
    assert!(!fixture.consent(ui));
    assert!(!fixture.native.manager.installed.contains_key("markdown"));
    assert!(fixture.native.manager.installed["alternate-prose"].enabled);
    assert_eq!(
        ui.update(|_, cx| fixture
            .native
            .app
            .read(cx)
            .editor
            .read(cx)
            .language_name()
            .to_string()),
        "markdown"
    );
}

/// A withdrawn inspection is not a user refusal; its delayed reply cannot consume future first-use support.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn fresh_markdown_withdrawn_late_offer_is_available_when_returning_to_file(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) = Fresh::mount(cx, true, &delivered(), "md");
    fixture.settle(ui);
    let path = fixture.native.directory.path().join("notes.md");
    ui.update(|window, cx| {
        fixture
            .native
            .app
            .update(cx, |app, cx| app.open_file(path, window, cx));
        window.draw(cx).clear(cx);
    });
    let request = ui
        .update(|_, cx| {
            fixture
                .native
                .app
                .read(cx)
                .extensions
                .read(cx)
                .worker
                .recorded
                .lock()
                .unwrap()
                .try_iter()
                .find_map(|work| match work {
                    Work::InspectBundle(request) => Some(request),
                    _ => None,
                })
        })
        .expect("production inspection of the active file");
    let token = request.token;
    let candidate = bundled::prepare_offer(&mut fixture.native.manager, &request)
        .unwrap()
        .unwrap();
    assert!(
        !fixture
            .native
            .manager
            .has_bundle_choice("markdown")
            .unwrap()
    );
    ui.update(|window, cx| {
        fixture.native.app.update(cx, |app, cx| {
            app.open_file(
                fixture.native.directory.path().join("other.txt"),
                window,
                cx,
            )
        });
        window.draw(cx).clear(cx);
        fixture
            .native
            .app
            .read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .bundle_reply = Some(bundled::Reply {
            token,
            result: Ok(Some(candidate)),
        });
    });
    fixture.settle(ui);
    assert!(!fixture.consent(ui));
    assert!(fixture.native.manager.installed.is_empty());
    fixture.open("notes.md", ui);
    assert!(
        fixture.consent(ui),
        "returning to the file must still offer a default the user never refused"
    );
    fixture.click_consent("plugin-install-confirm", ui);
    fixture.settle(ui);
    assert!(ui.debug_bounds("editor-plugin-layout").is_some());
}

/// Invalid shipped integrity or private choice metadata fails closed before permission or guest startup.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn fresh_markdown_bad_catalog_or_choice_metadata_never_offers_execution(cx: &mut TestAppContext) {
    for bad_choices in [false, true] {
        let (mut fixture, ui) = Fresh::mount(cx, true, &delivered(), "md");
        let workspace = Workspace::open(fixture.native.directory.path()).unwrap();
        if bad_choices {
            std::fs::write(
                workspace
                    .root()
                    .join(".runtime-plugin-test/bundle-choices.json"),
                b"{\"version\":99,\"entries\":{}}",
            )
            .unwrap();
        } else {
            let path = workspace
                .root()
                .join(".bundled-plugin-test/bundle-defaults.json");
            let mut catalog: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            catalog["packages"][0]["sha256"] = serde_json::json!("0".repeat(64));
            std::fs::write(path, serde_json::to_vec(&catalog).unwrap()).unwrap();
        }
        fixture.open("notes.md", ui);
        assert!(!fixture.consent(ui));
        assert!(
            fixture.native.manager.installed.is_empty() && fixture.native.manager.live.is_empty()
        );
        assert!(ui.debug_bounds("editor-source-toolbar").is_none());
        assert!(
            ui.update(|_, cx| fixture
                .native
                .app
                .read(cx)
                .extensions
                .read(cx)
                .status
                .is_some()),
            "the first-use integrity failure must be visible"
        );
        let indicator = ui
            .debug_bounds("plugin-error-indicator")
            .expect("main-window error indicator");
        ui.simulate_click(indicator.center(), Default::default());
        ui.run_until_parked();
        fixture.settle(ui);
        assert!(ui.debug_bounds("plugin-status-popup").is_some());
        assert!(
            ui.debug_bounds("plugin-manager-status-detail").is_some(),
            "the actual main-window popup must draw the integrity failure reason"
        );
    }
}

/// A separately built SDK guest and unfamiliar extension follow exactly the same first-use dialog path.
#[gpui::test]
#[ignore = "build current capability-example through scripts/verify-plugin-sdk.ps1 first"]
fn first_use_catalog_installs_an_independent_sdk_preview_with_an_opaque_identity(
    cx: &mut TestAppContext,
) {
    let package = independent_preview("opaque");
    let (mut fixture, ui) = Fresh::mount(cx, true, &archive(&package), "opaque");
    fixture.open("notes.opaque", ui);
    assert!(fixture.consent(ui));
    assert!(fixture.native.manager.live.is_empty());
    fixture.click_consent("plugin-install-confirm", ui);
    fixture.settle(ui);
    assert!(
        fixture
            .native
            .manager
            .installed
            .contains_key("bundled-opaque-preview")
    );
    assert!(!fixture.native.manager.installed.contains_key("markdown"));
    assert!(ui.debug_bounds("plugin-ui-bundle-greeting").is_some());
    assert!(ui.debug_bounds("editor-plugin-layout").is_some());
    fixture
        .native
        .manager
        .uninstall("bundled-opaque-preview", true)
        .unwrap();
    fixture.settle(ui);
    assert!(ui.debug_bounds("plugin-ui-bundle-greeting").is_none());
    let (mut fixture, ui) = fixture.reopen(cx);
    fixture.open("notes.opaque", ui);
    assert!(!fixture.consent(ui));
    assert!(fixture.native.manager.installed.is_empty());
}

/// Existing and explicitly disabled previews retain their selection instead of being replaced by a default.
#[gpui::test]
#[ignore = "build markdown and the external SDK fixture through their public host scripts first"]
fn fresh_markdown_keeps_an_independent_preview_provider_across_restart(cx: &mut TestAppContext) {
    for disabled in [false, true] {
        let (mut fixture, ui) = Fresh::mount(cx, true, &delivered(), "md");
        let alternative = independent_preview("md");
        fixture
            .native
            .manager
            .install(&alternative, alternative.manifest.permissions.clone())
            .unwrap();
        fixture.settle(ui);
        fixture.open("notes.md", ui);
        assert!(!fixture.consent(ui));
        assert!(!fixture.native.manager.installed.contains_key("markdown"));
        assert!(ui.debug_bounds("plugin-ui-bundle-greeting").is_some());
        if disabled {
            fixture
                .native
                .manager
                .disable("bundled-opaque-preview")
                .unwrap();
            fixture.settle(ui);
            assert!(ui.debug_bounds("editor-plugin-layout").is_none());
        }
        let (mut fixture, ui) = fixture.reopen(cx);
        fixture.open("notes.md", ui);
        assert!(!fixture.consent(ui));
        assert!(!fixture.native.manager.installed.contains_key("markdown"));
        assert_eq!(
            fixture.native.manager.installed["bundled-opaque-preview"].enabled,
            !disabled
        );
        assert_eq!(
            ui.debug_bounds("plugin-ui-bundle-greeting").is_some(),
            !disabled
        );
        assert!(
            !fixture
                .native
                .manager
                .has_bundle_choice("markdown")
                .unwrap()
        );
    }
}

/// A→B→A between shell frames must create a new live token rather than retain a cancelled request forever.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn fresh_markdown_same_frame_return_replaces_cancelled_consent(cx: &mut TestAppContext) {
    let bytes = delivered();
    let package = Package::from_bytes(&bytes).unwrap();
    let (mut fixture, ui) = Fresh::mount(cx, true, &bytes, "md");
    fixture.open("notes.md", ui);
    let old = ui
        .update(|_, cx| {
            fixture
                .native
                .app
                .read(cx)
                .extensions
                .read(cx)
                .bundled
                .confirmation(&package)
        })
        .unwrap();
    ui.update(|window, cx| {
        fixture.native.app.update(cx, |app, cx| {
            app.open_file(
                fixture.native.directory.path().join("other.txt"),
                window,
                cx,
            );
            app.open_file(fixture.native.directory.path().join("notes.md"), window, cx);
        })
    });
    assert!(
        !old.request.is_active(),
        "switching revokes ownership before any subsequent shell frame"
    );
    fixture.settle(ui);
    let current = ui
        .update(|_, cx| {
            fixture
                .native
                .app
                .read(cx)
                .extensions
                .read(cx)
                .bundled
                .confirmation(&package)
        })
        .expect("returning to the same file exposes a new current consent");
    assert_ne!(old.request.token, current.request.token);
    fixture.click_consent("plugin-install-confirm", ui);
    fixture.settle(ui);
    assert!(ui.debug_bounds("editor-plugin-layout").is_some());
}
