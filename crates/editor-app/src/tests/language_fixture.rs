//! UI lifetime tests obtain service plans through the same declarative package boundary as plugins.
use plugin_runtime::{LanguageService, Manager, Package, plugin_protocol::Environment};
use std::{
    io::{Cursor, Write},
    path::Path,
    sync::Arc,
};

/// Return an active owner and lazy service plan; these callback tests never launch the executable.
pub(crate) fn declared_language_service(root: &Path) -> (Manager, Arc<LanguageService>) {
    let manifest = serde_json::json!({
        "id":"novel-analysis", "name":"Novel analysis", "version":"1.0.0", "protocol":7,
        "api":{"base":"^1", "required":{"language.lsp":"^1", "process":"^1"}},
        "contributions":"plugin.toml", "storage_limit":1024,
        "services":{"analysis":{"program":std::env::current_exe().unwrap()}},
        "permissions":["process.service.analysis"],
        "language_servers":[{"id":"analysis", "language":"fixture", "service":"analysis"}]
    });
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("plugin.toml", b"[plugin]\nid = \"novel-analysis\"\nname = \"Novel analysis\"\nversion = \"1.0.0\"\nhost_version = \"^0.1\"\n".to_vec()),
    ] {
        archive.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        archive.write_all(&bytes).unwrap();
    }
    let package = Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap();
    let mut manager = Manager::open(
        root.join("fixture-plugins"),
        Environment {
            workspace: root.display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let plan = manager.language_services()["novel-analysis/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    (manager, plan)
}
