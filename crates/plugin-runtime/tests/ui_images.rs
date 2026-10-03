//! Image resources enter through an independent real SDK package and the public manager.
use plugin_runtime::{
    ImageResource, ImageState, Manager, Package,
    plugin_protocol::{Environment, api},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
#[path = "ui_images/http.rs"]
mod http;
#[path = "ui_images/lifecycle.rs"]
mod lifecycle;

use http::{GatedServer, HttpServer, PausedServer};

/// Repackage the actual guest without granting it access to runtime implementation details.
fn package(
    nodes: Vec<Value>,
    permissions: &[&str],
    edit: impl FnOnce(&mut Value, &mut BTreeMap<String, Vec<u8>>),
) -> anyhow::Result<Package> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&path)?.files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"])?;
    manifest["id"] = json!("images-fixture");
    manifest["name"] = json!("Controlled image fixture");
    manifest["api"]["required"] = json!({"package.assets":"^1","ui.native":"^1",
        "ui.images":"^1","editor.documents":"^1","configuration":"^1"});
    manifest["api"]["optional"] = json!({});
    let mut grants = vec!["assets.read", "editor.read"];
    grants.extend_from_slice(permissions);
    manifest["permissions"] = json!(grants);
    manifest["settings_hook"] = json!(false);
    manifest["settings"] = json!({"label":{"title":"Fixture UI",
        "value_type":{"kind":"string","max_length":120},"default":"composable-ui",
        "scope":"user","apply":"restart_instance"}});
    manifest["commands"] = json!([]);
    manifest["panels"] = json!([{"id":"welcome","title":"Images","position":"editor",
        "file_extensions":["sample"]}]);
    files.insert(
        "composed-ui.json".into(),
        serde_json::to_vec(&json!({"version":1,"revision":1,
        "root":{"id":"root","kind":{"type":"column","children":nodes}}}))?,
    );
    edit(&mut manifest, &mut files);
    files.insert("manifest.json".into(), serde_json::to_vec(&manifest)?);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    Package::from_bytes(&archive.finish()?.into_inner())
}

/// The capability is additive and does not require a Markdown identity or UI transport upgrade.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_capability_negotiates_independently() {
    let package = package(Vec::new(), &[], |_, _| {}).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(temp.path().join("plugins"), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert_eq!(manager.live["images-fixture"].views["welcome"].version, 1);
}

/// A minimal image node exercises the ordinary configured native tree, including alternative text.
fn image(id: &str, source: &str) -> Value {
    json!({"id":id,"kind":{"type":"image","source":source,"alt":format!("Alternative {id}")}})
}

struct Harness {
    manager: Manager,
    workspace: PathBuf,
    _root: tempfile::TempDir,
}
impl Harness {
    /// Installation grants exactly the declaration; unauthorized image reads still produce a view.
    fn new(nodes: Vec<Value>, permissions: &[&str]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(workspace.join("notes")).unwrap();
        let package = package(nodes, permissions, |_, _| {}).unwrap();
        let mut manager = Manager::open(
            root.path().join("plugins"),
            Environment {
                workspace: workspace.display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        assert!(
            manager.image_resources().is_empty(),
            "startup published an unbound image"
        );
        Self {
            manager,
            workspace,
            _root: root,
        }
    }
    fn source(revision: u64) -> api::DocumentVersion {
        api::DocumentVersion {
            id: "unsaved-source".into(),
            path: "notes/source.sample".into(),
            revision,
        }
    }
    /// Unsaved source authority comes through Preview, independently of the image files on disk.
    fn preview(&mut self, revision: u64) {
        self.manager
            .event(
                "images-fixture",
                Some("welcome".into()),
                api::Notification::Preview {
                    document: Some(Self::source(revision)),
                    text: "Unsaved image markup".into(),
                },
            )
            .unwrap();
    }
    fn settle(&mut self, seconds: u64) -> BTreeMap<String, Arc<ImageResource>> {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            self.manager.poll();
            let resources = self.manager.image_resources();
            if !resources
                .values()
                .any(|image| matches!(image.state, ImageState::Loading))
            {
                return resources;
            }
            assert!(Instant::now() < deadline, "image resources did not settle");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Failures are typed and belong to one image rather than retiring the containing WASM view.
fn failed(resources: &BTreeMap<String, Arc<ImageResource>>, node: &str, code: api::ErrorCode) {
    let ImageState::Failed(failure) =
        &resources[&format!("images-fixture/welcome/image/{node}")].state
    else {
        panic!("expected {node} to fail with {code:?}");
    };
    assert_eq!(failure.code, code, "{node}: {}", failure.message);
}

fn ready<'a>(resources: &'a BTreeMap<String, Arc<ImageResource>>, node: &str) -> &'a [u8] {
    let ImageState::Ready(bytes) =
        &resources[&format!("images-fixture/welcome/image/{node}")].state
    else {
        panic!("expected ready image {node}");
    };
    bytes
}

/// The actual guest may declare an image only after negotiating ui.images and echoing its Preview.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_node_requires_its_own_capability() {
    let package = package(vec![image("picture", "missing.png")], &[], |manifest, _| {
        manifest["api"]["required"]
            .as_object_mut()
            .unwrap()
            .remove("ui.images");
    })
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(temp.path().join("plugins"), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let error = manager
        .event(
            "images-fixture",
            Some("welcome".into()),
            api::Notification::Preview {
                document: Some(Harness::source(1)),
                text: "Image markup".into(),
            },
        )
        .unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<api::Failure>()
            .map(|failure| failure.code),
        Some(api::ErrorCode::CapabilityUnavailable)
    );
    assert!(manager.image_resources().is_empty());
}

/// Local paths allow parent segments and percent-encoded Unicode only inside the canonical workspace.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_local_reads_are_document_relative_and_bounded() {
    let mut harness = Harness::new(
        vec![
            image("good", "../assets/%E4%BD%A0%E5%A5%BD.png"),
            image("missing", "missing.png"),
            image("escape", "../../outside.png"),
            image("absolute", "/outside.png"),
            image("ads", "../assets/file.png%3Asecret"),
            image("backslash", "..%5Coutside.png"),
            image("device", "CON.png"),
            image("oversized", "../assets/large.png"),
        ],
        &["workspace.read"],
    );
    std::fs::create_dir(harness.workspace.join("assets")).unwrap();
    std::fs::write(
        harness.workspace.join("assets/你好.png"),
        b"package-independent-image",
    )
    .unwrap();
    std::fs::write(
        harness._root.path().join("outside.png"),
        b"private-outside-image",
    )
    .unwrap();
    write_large(&harness.workspace.join("assets/large.png"));
    harness.preview(1);
    let resources = harness.settle(5);
    assert_eq!(ready(&resources, "good"), b"package-independent-image");
    assert_eq!(
        resources["images-fixture/welcome/image/good"].source,
        Harness::source(1)
    );
    failed(&resources, "missing", api::ErrorCode::NotFound);
    failed(&resources, "escape", api::ErrorCode::PermissionDenied);
    for node in ["absolute", "ads", "backslash", "device"] {
        failed(&resources, node, api::ErrorCode::InvalidPath);
    }
    failed(&resources, "oversized", api::ErrorCode::LimitExceeded);
    assert!(
        harness.manager.live["images-fixture"]
            .views
            .contains_key("welcome")
    );
}

/// Sparse file metadata tests the public 8 MiB bound without unnecessary test memory allocation.
fn write_large(path: &Path) {
    std::fs::File::create(path)
        .unwrap()
        .set_len(8 * 1024 * 1024 + 1)
        .unwrap();
}

/// Lacking local or network grants is a per-image failure and leaves ordinary content usable.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_missing_permissions_fail_individually() {
    let mut harness = Harness::new(
        vec![
            image("local", "secret.png"),
            image("remote", "http://127.0.0.1:1/image.png"),
            json!({"id":"caption","kind":{"type":"text","text":"The rest of the preview"}}),
        ],
        &[],
    );
    harness.preview(1);
    let resources = harness.settle(2);
    failed(&resources, "local", api::ErrorCode::PermissionDenied);
    failed(&resources, "remote", api::ErrorCode::PermissionDenied);
    assert_eq!(
        harness.manager.live["images-fixture"].views["welcome"].source,
        Some(Harness::source(1))
    );
}

/// Native link resolution must enforce the final junction target, not its lexical workspace prefix.
#[test]
#[cfg(windows)]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_local_junction_cannot_escape_workspace() {
    use std::os::windows::process::CommandExt;
    let mut harness = Harness::new(
        vec![image("junction", "../redirect/secret.png")],
        &["workspace.read"],
    );
    let outside = harness._root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret.png"), b"outside").unwrap();
    let junction = harness.workspace.join("redirect");
    let result = std::process::Command::new("powershell.exe")
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command",
            "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:ME_EDITOR_TEST_IMAGE_JUNCTION -Value $env:ME_EDITOR_TEST_IMAGE_TARGET | Out-Null"])
        .env("ME_EDITOR_TEST_IMAGE_JUNCTION", &junction).env("ME_EDITOR_TEST_IMAGE_TARGET", &outside).output().unwrap();
    assert!(
        result.status.success(),
        "junction creation failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    harness.preview(1);
    failed(
        &harness.settle(5),
        "junction",
        api::ErrorCode::PermissionDenied,
    );
}

/// Remote bytes use only network.images and never inherit URL credentials, cookies or redirects.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_http_status_permissions_and_redirects_are_per_node() {
    let server = HttpServer::new();
    let mut harness = Harness::new(
        vec![
            image("good", &server.url("ok")),
            image("missing", &server.url("missing")),
            image("redirect", &server.url("redirect")),
            image("large", &server.url("large")),
            image(
                "credentials",
                &server
                    .url("ok")
                    .replacen("http://", "http://user:secret@", 1),
            ),
            image("scheme", "file:///secret.png"),
        ],
        &["network.images", "workspace.read"],
    );
    harness.preview(1);
    let resources = harness.settle(5);
    assert_eq!(ready(&resources, "good"), b"fixture-image");
    failed(&resources, "missing", api::ErrorCode::OperationFailed);
    failed(&resources, "redirect", api::ErrorCode::OperationFailed);
    failed(&resources, "large", api::ErrorCode::LimitExceeded);
    failed(&resources, "credentials", api::ErrorCode::InvalidPath);
    failed(&resources, "scheme", api::ErrorCode::InvalidPath);
    assert_eq!(
        server.requests(),
        4,
        "redirects or credentials created an extra HTTP request"
    );
    assert!(!server.ambient_headers());
}

/// URI schemes are ASCII-insensitive; a TLS failure on loopback still proves dispatch used network authority.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_http_scheme_case_uses_network_authority() {
    let server = HttpServer::new();
    let upper = server.url("ok").replacen("http://", "HTTP://", 1);
    let mixed_tls = server.url("ok").replacen("http://", "HtTpS://", 1);
    let mut harness = Harness::new(
        vec![image("upper", &upper), image("mixed-tls", &mixed_tls)],
        &["network.images"],
    );
    harness.preview(1);
    let resources = harness.settle(5);
    assert_eq!(ready(&resources, "upper"), b"fixture-image");
    assert_eq!(resources["images-fixture/welcome/image/upper"].uri, upper);
    // The controlled server speaks plaintext deliberately; no external TLS endpoint is contacted.
    failed(&resources, "mixed-tls", api::ErrorCode::OperationFailed);
    assert_eq!(server.requests(), 2);
}

/// Per-manager resident limits count encoded bytes from all images without truncating successful images.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_resident_bytes_are_bounded_across_nodes() {
    let nodes = (0..10)
        .map(|index| image(&format!("image-{index}"), "../assets/full.png"))
        .collect();
    let mut harness = Harness::new(nodes, &["workspace.read"]);
    std::fs::create_dir(harness.workspace.join("assets")).unwrap();
    std::fs::File::create(harness.workspace.join("assets/full.png"))
        .unwrap()
        .set_len(8 * 1024 * 1024)
        .unwrap();
    harness.preview(1);
    let resources = harness.settle(10);
    let resident: usize = resources
        .values()
        .map(|image| match &image.state {
            ImageState::Ready(bytes) => bytes.len(),
            ImageState::Failed(error) => {
                assert_eq!(error.code, api::ErrorCode::LimitExceeded);
                0
            }
            ImageState::Loading => panic!("bounded image loading did not complete"),
        })
        .sum();
    assert!(resident <= 64 * 1024 * 1024);
    assert!(
        resources
            .values()
            .any(|image| matches!(image.state, ImageState::Failed(_)))
    );
}

/// Two different manager roots share one process-wide pool; pending nodes resume through normal polling.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_worker_pool_is_global_and_queued_nodes_resume() {
    let server = GatedServer::new();
    let nodes = || {
        (0..8)
            .map(|index| image(&format!("image-{index}"), &server.url()))
            .collect()
    };
    let mut first = Harness::new(nodes(), &["network.images"]);
    let mut second = Harness::new(nodes(), &["network.images"]);
    first.preview(1);
    second.preview(1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.requests() < 8 {
        first.manager.poll();
        second.manager.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        server.requests(),
        8,
        "separate managers bypassed the global producer bound"
    );
    server.release();
    assert!(
        first
            .settle(5)
            .values()
            .all(|image| matches!(image.state, ImageState::Ready(_)))
    );
    assert!(
        second
            .settle(5)
            .values()
            .all(|image| matches!(image.state, ImageState::Ready(_)))
    );
    assert_eq!(server.requests(), 16, "queued images did not resume");
}
