//! Package-owned function artwork keeps its resource boundary after fixed host modes are removed.
use plugin_runtime::{Manager, Package, plugin_protocol::Environment};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Write},
    path::Path,
};

const ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 6h16M4 12h16" fill="none" stroke="#000"/></svg>"##;

/// Repack the independently built guest under a different identity through normal archive admission.
fn package() -> Package {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&path).unwrap().files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!("artwork-fixture");
    manifest["name"] = json!("Artwork Fixture");
    manifest["api"]["required"] = json!({"package.assets":"^1", "ui.native":"^1", "ui.tools":"^1"});
    manifest["api"]["optional"] = json!({});
    manifest["permissions"] = json!(["assets.read"]);
    manifest["settings"] = json!({});
    manifest["settings_hook"] = json!(false);
    manifest["commands"] = json!([]);
    manifest["panels"] = json!([{"id":"welcome", "title":"Artwork", "position":"right"}]);
    files.insert("icons/action.svg".into(), ICON.as_bytes().to_vec());
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

/// Installed artwork rechecks bytes, quotas and resolved ownership rather than trusting an earlier inspection.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn installed_tool_icons_reject_unsafe_bytes_and_resolved_ownership_escape() {
    let package = package();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("plugins");
    let mut manager = Manager::open(root.clone(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let installed = &manager.installed["artwork-fixture"];
    assert_eq!(
        installed.tool_icon(&root, "icons/action.svg").unwrap(),
        ICON.as_bytes()
    );
    for path in [
        "../action.svg",
        "/action.svg",
        "C:/action.svg",
        "icons\\action.svg",
        "icons/missing.svg",
    ] {
        assert!(
            installed.tool_icon(&root, path).is_none(),
            "accepted {path}"
        );
    }
    let icon = root
        .join("packages/artwork-fixture")
        .join(&installed.digest)
        .join("icons/action.svg");
    for source in [
        r#"<svg><path d="M0 0h1" href = "file:///secret.svg"/></svg>"#,
        "<svg xmlns:xlink=\"http://www.w3.org/1999/xlink\"><g xlink:href\n=\"https://example.invalid/a.svg\"/></svg>",
        r#"<svg><path d="M0 0h1" fill="url&#40;file:///secret.svg&#41;"/></svg>"#,
        r#"<svg><path d="M0 0h1" style="fill: url (https://example.invalid/a.svg)"/></svg>"#,
        "<svg><style>@import url(https://example.invalid/style.css)</style></svg>",
        "<svg><script>alert(1)</script></svg>",
        "<svg><foreignObject><iframe/></foreignObject></svg>",
        "<?xml-stylesheet href=\"https://example.invalid/style.css\"?><svg/>",
        "<!DOCTYPE svg [<!ENTITY external SYSTEM \"file:///secret\">]><svg>&external;</svg>",
        "<svg><path></svg>",
    ] {
        std::fs::write(&icon, source).unwrap();
        assert!(
            installed.tool_icon(&root, "icons/action.svg").is_none(),
            "accepted unsafe SVG: {source}"
        );
    }
    std::fs::write(&icon, vec![b' '; 64 * 1024 + 1]).unwrap();
    assert!(installed.tool_icon(&root, "icons/action.svg").is_none());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A real junction tests resolved ownership without requiring developer-mode symlink privileges.
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("action.svg"), ICON).unwrap();
        let redirect = icon.parent().unwrap().join("redirect");
        let result = std::process::Command::new("powershell.exe")
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:ME_EDITOR_TEST_ICON_JUNCTION -Value $env:ME_EDITOR_TEST_ICON_TARGET | Out-Null"])
            .env("ME_EDITOR_TEST_ICON_JUNCTION", &redirect)
            .env("ME_EDITOR_TEST_ICON_TARGET", &outside).output().unwrap();
        assert!(
            result.status.success(),
            "junction failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            installed
                .tool_icon(&root, "icons/redirect/action.svg")
                .is_none()
        );
    }
    manager.uninstall("artwork-fixture", true).unwrap();
    assert!(!manager.installed.contains_key("artwork-fixture"));
    assert!(!root.join("packages/artwork-fixture").exists());
}
