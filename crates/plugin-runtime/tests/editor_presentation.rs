//! Editor presentation declarations enter through an independent SDK ZIP and the public manager.
use plugin_runtime::{Manager, Package, plugin_protocol::Environment};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
};

const SOURCE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 6h16M4 12h16M4 18h16" fill="none" stroke="#000"/></svg>"##;
const SPLIT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="16" fill="none" stroke="#000"/><path d="M12 4v16" stroke="#000"/></svg>"##;
const PREVIEW: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8" fill="none" stroke="#000"/></svg>"##;

/// Mutate only the installed archive boundary; no test-only host API or Markdown identity is used.
fn package(
    edit: impl FnOnce(&mut Value, &mut BTreeMap<String, Vec<u8>>),
) -> anyhow::Result<Package> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&path)?.files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"])?;
    manifest["id"] = json!("presentation-fixture");
    manifest["name"] = json!("Editor presentation fixture");
    manifest["api"]["required"] = json!({
        "package.assets":"^1", "ui.native":"^1", "editor.documents":"^1", "editor.presentation":"^1"
    });
    manifest["api"]["optional"] = json!({});
    manifest["permissions"] = json!(["assets.read", "editor.read"]);
    manifest["settings"] = json!({});
    manifest["settings_hook"] = json!(false);
    manifest["commands"] = json!([]);
    manifest["panels"] = json!([{
        "id":"welcome", "title":"Presentation", "position":"editor", "file_extensions":["sample"],
        "view_modes":{"source":"icons/source.svg", "split":"icons/split.svg", "preview":"icons/preview.svg"}
    }]);
    for (path, svg) in [
        ("icons/source.svg", SOURCE),
        ("icons/split.svg", SPLIT),
        ("icons/preview.svg", PREVIEW),
    ] {
        files.insert(path.into(), svg.as_bytes().to_vec());
    }
    edit(&mut manifest, &mut files);
    files.insert("manifest.json".into(), serde_json::to_vec(&manifest)?);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    Package::from_bytes(&archive.finish()?.into_inner())
}

/// An independently named real guest can request the presentation capability and retain all three icons.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn presentation_declaration_negotiates_on_independent_real_guest() {
    let package = package(|_, _| {}).unwrap();
    let panel = serde_json::to_value(&package.manifest.panels[0]).unwrap();
    assert_eq!(
        panel["view_modes"],
        json!({
            "source":"icons/source.svg", "split":"icons/split.svg", "preview":"icons/preview.svg"
        })
    );
    let temp = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(temp.path().join("plugins"), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(
        manager.live["presentation-fixture"]
            .views
            .contains_key("welcome")
    );
}

/// Presentation is an explicit required contract available only on workspace-owned editor previews.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn presentation_modes_require_required_capability_and_workspace_preview() {
    for invalid in [
        "missing",
        "optional",
        "dock",
        "application",
        "read",
        "documents",
    ] {
        let result = package(|manifest, _| match invalid {
            "missing" | "optional" => {
                manifest["api"]["required"]
                    .as_object_mut()
                    .unwrap()
                    .remove("editor.presentation");
                if invalid == "optional" {
                    manifest["api"]["optional"]["editor.presentation"] = json!("^1");
                }
            }
            "dock" => {
                manifest["panels"][0]["position"] = json!("right");
                manifest["panels"][0]["file_extensions"] = json!([]);
            }
            "application" => manifest["scope"] = json!("application"),
            "read" => manifest["permissions"] = json!(["assets.read"]),
            "documents" => {
                manifest["api"]["required"]
                    .as_object_mut()
                    .unwrap()
                    .remove("editor.documents");
            }
            _ => unreachable!(),
        });
        assert!(
            result.is_err(),
            "accepted invalid presentation declaration: {invalid}"
        );
    }
}

/// Mode artwork cannot turn a declarative layout contribution into ambient file, URL or script access.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn presentation_icons_reject_unsafe_paths_and_external_svg_content() {
    for path in [
        "../source.svg",
        "/source.svg",
        "C:/source.svg",
        "icons\\source.svg",
        "icons/missing.svg",
    ] {
        assert!(
            package(|manifest, _| {
                manifest["panels"][0]["view_modes"]["source"] = json!(path);
            })
            .is_err(),
            "accepted unsafe or missing mode icon: {path}"
        );
    }
    for svg in [
        r#"<svg><path d="M0 0h1" href = "file:///secret.svg"/></svg>"#,
        "<svg xmlns:xlink=\"http://www.w3.org/1999/xlink\"><g xlink:href\n=\"https://example.invalid/a.svg\"/></svg>",
        r#"<svg xmlns="http://www.w3.org/2000/svg"><image href = "file:///secret.png"/></svg>"#,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><image xlink:href\n=\"https://example.invalid/a.png\"/></svg>",
        r#"<svg><path d="M0 0h1" fill="url&#40;file:///secret.svg&#41;"/></svg>"#,
        r#"<svg><path d="M0 0h1" style="fill: url (https://example.invalid/a.svg)"/></svg>"#,
        r#"<svg><style>@import url(https://example.invalid/style.css)</style></svg>"#,
        r#"<svg><script>alert(1)</script></svg>"#,
        r#"<svg><foreignObject><iframe src="file:///secret.html"/></foreignObject></svg>"#,
        r#"<?xml-stylesheet href="https://example.invalid/style.css"?><svg/>"#,
        r#"<!DOCTYPE svg [<!ENTITY external SYSTEM "file:///secret">]><svg>&external;</svg>"#,
        "<svg><path></svg>",
    ] {
        assert!(
            package(|_, files| {
                files.insert("icons/source.svg".into(), svg.as_bytes().to_vec());
            })
            .is_err(),
            "accepted unsafe mode SVG: {svg}"
        );
    }
    let oversized = format!("{SOURCE}{}", " ".repeat(64 * 1024 + 1));
    assert!(
        package(|_, files| {
            files.insert("icons/source.svg".into(), oversized.into_bytes());
        })
        .is_err()
    );
}

/// The public installed-package reader selects every mode and refuses artwork changed after inspection.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn presentation_installed_icons_are_owned_bounded_and_revalidated() {
    use plugin_runtime::plugin_protocol::PreviewMode;
    let package = package(|_, _| {}).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("plugins");
    let mut manager = Manager::open(root.clone(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let mut installed = manager.installed["presentation-fixture"].clone();
    assert_eq!(PreviewMode::default(), PreviewMode::Split);
    for (mode, expected, encoding) in [
        (PreviewMode::Source, SOURCE, "\"source\""),
        (PreviewMode::Split, SPLIT, "\"split\""),
        (PreviewMode::Preview, PREVIEW, "\"preview\""),
    ] {
        assert_eq!(serde_json::to_string(&mode).unwrap(), encoding);
        assert_eq!(
            installed.preview_mode_icon(&root, "welcome", mode).unwrap(),
            expected.as_bytes()
        );
    }
    assert!(
        installed
            .preview_mode_icon(&root, "missing", PreviewMode::Source)
            .is_none()
    );
    let icon = root
        .join("packages/presentation-fixture")
        .join(&installed.digest)
        .join("icons/source.svg");
    std::fs::write(&icon, r#"<svg><image href = "file:///secret.png"/></svg>"#).unwrap();
    assert!(
        installed
            .preview_mode_icon(&root, "welcome", PreviewMode::Source)
            .is_none()
    );
    std::fs::write(&icon, vec![b' '; 64 * 1024 + 1]).unwrap();
    assert!(
        installed
            .preview_mode_icon(&root, "welcome", PreviewMode::Source)
            .is_none()
    );
    installed.manifest.panels[0]
        .view_modes
        .as_mut()
        .unwrap()
        .source = "../outside.svg".into();
    assert!(
        installed
            .preview_mode_icon(&root, "welcome", PreviewMode::Source)
            .is_none()
    );
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Junctions test the resolved ownership boundary without developer-mode symlink privileges.
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("source.svg"), SOURCE).unwrap();
        let redirect = icon.parent().unwrap().join("redirect");
        let result = std::process::Command::new("powershell.exe")
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:ME_EDITOR_TEST_ICON_JUNCTION -Value $env:ME_EDITOR_TEST_ICON_TARGET | Out-Null"])
            .env("ME_EDITOR_TEST_ICON_JUNCTION", &redirect)
            .env("ME_EDITOR_TEST_ICON_TARGET", &outside)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "junction creation failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        installed.manifest.panels[0]
            .view_modes
            .as_mut()
            .unwrap()
            .source = "icons/redirect/source.svg".into();
        assert!(
            installed
                .preview_mode_icon(&root, "welcome", PreviewMode::Source)
                .is_none()
        );
    }
}
