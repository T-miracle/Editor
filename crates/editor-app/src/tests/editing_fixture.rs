//! Unknown language packages exercise public declarations and controlled native dependency preparation.
use plugin_runtime::Package;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

/// Reidentify the distributed HTML parser without changing its services, recognition or dependency bytes.
/// Passing this archive through Manager proves editing mechanisms are independent of installed package IDs.
pub(super) fn relabel_html(id: &str) -> Package {
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/html.zip"),
    )
    .unwrap();
    let mut files = package.files;
    let mut manifest = serde_json::to_value(package.manifest).unwrap();
    manifest["id"] = json!(id);
    manifest["name"] = json!(id);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let contribution = String::from_utf8(files.remove("plugin.toml").unwrap())
        .unwrap()
        .replacen("id = \"html\"", &format!("id = \"{id}\""), 1);
    files.insert("plugin.toml".into(), contribution.into_bytes());
    repack(files)
}

/// Reuse the approved bundled Node artifact, while installing this fixture's own script and identity.
pub(super) fn package(
    id: &str,
    language: &str,
    primary: bool,
    editing: bool,
    log: &Path,
    delay: u64,
    mode: &str,
) -> Package {
    let javascript = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/javascript.zip"),
    )
    .unwrap();
    let mut manifest = serde_json::to_value(javascript.manifest).unwrap();
    let service = if primary { "analysis" } else { "format" };
    let source = include_bytes!("fixtures/editing-service.cjs");
    let mut plan = manifest["services"]["format"].clone();
    plan["args"] = json!([
        "${dependency:server}/server.cjs",
        log,
        delay.to_string(),
        mode
    ]);
    plan["installation"]["artifacts"][1]["version"] = json!("1.0.0");
    plan["installation"]["artifacts"][1]["sha256"] = json!(format!("{:x}", Sha256::digest(source)));
    manifest["id"] = json!(id);
    manifest["name"] = json!(id);
    manifest["version"] = json!("1.0.0");
    manifest["services"] = json!({service:plan});
    manifest["permissions"] = json!([format!("process.service.{service}"), "dependencies.prepare"]);
    manifest["language_servers"] = json!([{"id":service,"language":language,"service":service,"primary":primary,"formatting":!primary || editing,"editing":editing,"executable_setting":"tool"}]);
    manifest["settings"] = json!({"tool":{"title":"Executable","value_type":{"kind":"string"},"default":"","scope":"project"}});
    if editing {
        manifest["api"]["required"]["language.editing"] = json!("^1");
    }
    let recognition = if editing {
        format!(
            "\n[[language_definitions]]\nid = \"{language}\"\nname = \"{language}\"\nextensions = [\"linked\"]\n"
        )
    } else {
        String::new()
    };
    let contribution = format!(
        "[plugin]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nhost_version = \"^0.1\"\n{recognition}"
    );
    repack(BTreeMap::from([
        (
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        ),
        ("plugin.toml".into(), contribution.into_bytes()),
        ("native/server.cjs".into(), source.to_vec()),
    ]))
}

/// Reuse the exact approved 01 executable through public settings, then install the original XML ZIP.
/// Missing proof artifacts use normal private preparation; a present artifact must match its pinned hash.
pub(super) fn install_xml(manager: &mut plugin_runtime::Manager, package: &Package) {
    let began = std::time::Instant::now();
    let executable = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/xml-verification/native/lemminx-win32.exe");
    if executable.is_file() {
        let bytes = std::fs::read(&executable).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            "25f571c0f07d0ad76be60a86c1b864d46cf7bf94773dc21e5d5da646478aac79",
            "the local reuse fixture must be the already-approved formal native distribution"
        );
        let key = package.manifest.language_servers[0]
            .executable_setting
            .as_ref()
            .unwrap();
        // Settings require an installed owner. Publish no UI from this resource-only bootstrap,
        // and replace it with the unmodified formal ZIP after recording the explicit user choice.
        let mut manifest = serde_json::to_value(&package.manifest).unwrap();
        manifest["component"] = json!(null);
        manifest["services"] = json!({});
        manifest["language_servers"] = json!([]);
        manifest["structure_providers"] = json!([]);
        manifest["permissions"] = json!([]);
        let mut files = package.files.clone();
        files.insert(
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        let bootstrap = repack(files);
        manager
            .install(&bootstrap, bootstrap.manifest.permissions.clone())
            .unwrap();
        manager
            .update_setting(
                &package.manifest.id,
                plugin_runtime::plugin_protocol::settings::Scope::Project,
                key,
                Some(json!(executable.canonicalize().unwrap())),
            )
            .unwrap();
    }
    manager
        .install(package, package.manifest.permissions.clone())
        .unwrap();
    if executable.is_file() {
        let key = package.manifest.language_servers[0]
            .executable_setting
            .as_ref()
            .unwrap();
        // Read back the formal instance after replacement; a dropped override would re-download its tool.
        let effective = manager.effective_settings(&package.manifest.id).unwrap();
        assert_eq!(
            effective[key].value,
            json!(executable.canonicalize().unwrap()),
            "formal XML installation must retain the public local executable selection"
        );
        assert_eq!(
            effective[key].source,
            plugin_runtime::plugin_protocol::settings::Source::Project
        );
        eprintln!(
            "Formal XML public install retained the approved local executable in {:.2}s",
            began.elapsed().as_secs_f64()
        );
    }
}

/// Reinspect every assembled fixture at the same archive boundary as an externally installed package.
fn repack(files: BTreeMap<String, Vec<u8>>) -> Package {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// Count transport requests from the fixture process, rather than asserting host call internals.
pub(super) fn count(log: &Path, method: &str) -> usize {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["method"] == method)
        .count()
}
