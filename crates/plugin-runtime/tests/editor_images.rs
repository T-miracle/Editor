//! Independent actual WASM guests receive scoped image metadata through the public manager.
use plugin_runtime::{
    HostImageInput, HostImageOrigin, Manager, Package,
    plugin_protocol::{Environment, api, ui::Kind},
};
use serde_json::{Value, json};
#[path = "editor_images/lifecycle.rs"]
mod lifecycle;
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    sync::Arc,
};

/// Keep real SDK component dispatch while changing only the ordinary package's identity and declarations.
fn package(
    edit: impl FnOnce(&mut Value, &mut BTreeMap<String, Vec<u8>>),
) -> anyhow::Result<Package> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&path)?.files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"])?;
    manifest["id"] = json!("image-input-fixture");
    manifest["name"] = json!("Document image input fixture");
    manifest["api"]["required"] =
        json!({"package.assets":"^1","ui.native":"^1","editor.images":"^1"});
    manifest["api"]["optional"] = json!({});
    manifest["permissions"] = json!(["assets.read", "editor.read", "editor.write"]);
    manifest["settings"] = json!({});
    manifest["settings_hook"] = json!(false);
    manifest["commands"] = json!([{"id":"scope-probe","title":"Probe scoped resource"}]);
    edit(&mut manifest, &mut files);
    files.insert("manifest.json".into(), serde_json::to_vec(&manifest)?);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    Package::from_bytes(&archive.finish()?.into_inner())
}

/// Negotiation is independent from Markdown and precedes any image capture or filesystem effect.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn editor_images_capability_negotiates_on_independent_real_guest() {
    let package = package(|_, _| {}).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(manager.live.contains_key("image-input-fixture"));
}

/// The ordinary composable asset opts into source input; no Markdown source or identity is involved.
fn image_package(edit: impl FnOnce(&mut Value, &mut BTreeMap<String, Vec<u8>>)) -> Package {
    package(|manifest, files| {
        manifest["api"]["required"]["configuration"] = json!("^1");
        manifest["api"]["required"]["editor.documents"] = json!("^1");
        manifest["permissions"] = json!(["assets.read","editor.read","editor.write","workspace.write","clipboard"]);
        manifest["panels"] = json!([{"id":"welcome","title":"Native image input","position":"editor","file_extensions":["sample"]}]);
        manifest["settings"] = json!({"label":{"title":"Fixture UI","value_type":{"kind":"string","max_length":120},
            "default":"composable-ui","scope":"user","apply":"restart_instance"}});
        files.insert("composed-ui.json".into(), serde_json::to_vec(&json!({
            "version":1,"revision":1,"editor_image_input":true,
            "root":{"id":"body","kind":{"type":"text","text":"Preview"}}
        })).unwrap());
        edit(manifest, files);
    }).unwrap()
}

struct Harness {
    manager: Manager,
    _root: tempfile::TempDir,
}
impl Harness {
    /// Actual saved workspace files let public source checks exercise normal canonical path authority.
    fn new(package: &Package) -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(workspace.join("notes")).unwrap();
        std::fs::write(workspace.join("notes/source.sample"), "你好").unwrap();
        let mut manager = Manager::open(
            root.path().join("plugins"),
            Environment {
                workspace: workspace.to_string_lossy().into(),
                ..Environment::default()
            },
        )
        .unwrap();
        manager
            .install(package, package.manifest.permissions.clone())
            .unwrap();
        Self {
            manager,
            _root: root,
        }
    }
    fn preview(&mut self, id: &str, document: Option<api::DocumentVersion>) -> anyhow::Result<()> {
        self.manager.event(
            id,
            Some("welcome".into()),
            api::Notification::Preview {
                text: if document.is_some() {
                    "你好".into()
                } else {
                    String::new()
                },
                document,
            },
        )
    }
    /// Pixels enter through the trusted native method and are exposed to the guest only as metadata.
    fn offer(
        &mut self,
        id: &str,
        origin: HostImageOrigin,
        images: Vec<HostImageInput>,
    ) -> anyhow::Result<Vec<api::ImageInput>> {
        self.manager.offer_image_input(
            id,
            "welcome",
            document(7),
            api::TextRange { start: 0, end: 6 },
            origin,
            images,
        )?;
        let api::Notification::ImageInput {
            document: source,
            selection,
            images,
        } = serde_json::from_str(&text(&self.manager, id)).unwrap()
        else {
            panic!("Expected native image metadata")
        };
        assert_eq!(source, document(7));
        assert_eq!(selection, api::TextRange { start: 0, end: 6 });
        Ok(images)
    }
    /// The fixture's ordinary command transports a typed SDK request; refusal does not retire its guest.
    fn probe(&mut self, id: &str, operation: Value) -> Result<api::Value, api::Failure> {
        self.manager
            .invoke_command(id, "scope-probe", operation)
            .unwrap();
        serde_json::from_str(&text(&self.manager, id)).unwrap()
    }
    fn save(
        &mut self,
        id: &str,
        input: &api::ResourceHandle,
        name: &str,
    ) -> Result<api::Value, api::Failure> {
        self.probe(
            id,
            json!({"method":"editor","timeout_ms":30000,
            "operation":{"kind":"save_image_input","input":input,"name":name}}),
        )
    }
    fn take(&mut self, id: &str) -> plugin_runtime::EditorRequest {
        let mut requests = self
            .manager
            .live
            .get_mut(id)
            .unwrap()
            .take_editor_requests();
        assert_eq!(requests.len(), 1);
        requests.pop().unwrap()
    }
}

fn document(revision: u64) -> api::DocumentVersion {
    api::DocumentVersion {
        id: "saved-file".into(),
        path: "notes/source.sample".into(),
        revision,
    }
}
fn text(manager: &Manager, id: &str) -> String {
    let Kind::Text { text } = &manager.live[id].views["welcome"].root.kind else {
        panic!("Expected native diagnostic text")
    };
    text.clone()
}
fn image(bytes: Arc<Vec<u8>>) -> HostImageInput {
    HostImageInput {
        format: api::ImageFormat::Png,
        bytes,
    }
}
fn code(error: anyhow::Error) -> api::ErrorCode {
    error.downcast_ref::<api::Failure>().unwrap().code
}

/// A batch never serializes its pixels; conflicts retain input, success consumes it and receipts remain exact.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn image_input_metadata_and_collision_retry_keep_owned_native_payload() {
    let mut host = Harness::new(&image_package(|_, _| {}));
    host.preview("image-input-fixture", Some(document(7)))
        .unwrap();
    let bytes = Arc::new(vec![1; 8 * 1024 * 1024]);
    let input = host
        .offer(
            "image-input-fixture",
            HostImageOrigin::Clipboard,
            vec![image(bytes.clone())],
        )
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(input.byte_len, 8 * 1024 * 1024);
    assert!(text(&host.manager, "image-input-fixture").len() < 1024);
    assert!(matches!(
        host.save("image-input-fixture", &input.handle, "img.png")
            .unwrap(),
        api::Value::Accepted(_)
    ));
    let first = host.take("image-input-fixture");
    // Even a valid resource from this same instance cannot substitute a different root kind.
    assert_eq!(
        host.save("image-input-fixture", first.handle(), "img.png")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
    let payload = first.image_input().unwrap();
    assert!(Arc::ptr_eq(&bytes, &payload.bytes));
    assert_eq!(payload.document, document(7));
    assert_eq!(payload.selection, api::TextRange { start: 0, end: 6 });
    assert_eq!(
        host.save("image-input-fixture", &input.handle, "img1.png")
            .unwrap_err()
            .code,
        api::ErrorCode::Conflict
    );
    first.finish(Err(api::Failure::new(
        api::ErrorCode::Conflict,
        "already exists",
    )));
    host.manager.poll();
    assert!(matches!(
        host.save("image-input-fixture", &input.handle, "img1.png")
            .unwrap(),
        api::Value::Accepted(_)
    ));
    let retry = host.take("image-input-fixture");
    assert!(retry.begin() && retry.enter_side_effect());
    retry.finish(Ok(api::EditorValue::ImageSaved {
        input: input.handle.clone(),
        document: document(7),
        name: "img1.png".into(),
    }));
    host.manager.poll();
    assert!(text(&host.manager, "image-input-fixture").contains("ImageSaved"));
    assert_eq!(
        host.save("image-input-fixture", &input.handle, "img2.png")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
}

/// Names, origins and instance identity do not create ambient write or clipboard authority.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn image_input_names_and_foreign_handles_are_rejected_without_native_requests() {
    let mut host = Harness::new(&image_package(|_, _| {}));
    host.preview("image-input-fixture", Some(document(7)))
        .unwrap();
    let input = host
        .offer(
            "image-input-fixture",
            HostImageOrigin::Drop,
            vec![image(Arc::new(vec![1]))],
        )
        .unwrap()
        .pop()
        .unwrap();
    let too_long = format!("{}.png", "a".repeat(252));
    for name in [
        "../img.png",
        "/img.png",
        "C:/img.png",
        "img.png:stream",
        "CON.png",
        "COM1.png",
        "LPT³.png",
        "img.png ",
        "img.jpg",
        ".png",
        "a\\img.png",
    ]
    .into_iter()
    .chain(std::iter::once(too_long.as_str()))
    {
        assert_eq!(
            host.save("image-input-fixture", &input.handle, name)
                .unwrap_err()
                .code,
            api::ErrorCode::InvalidPath,
            "{name}"
        );
    }
    let mut forged = input.handle.clone();
    forged.instance = "foreign".into();
    assert_eq!(
        host.save("image-input-fixture", &forged, "img.png")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
    let other = image_package(|manifest, _| manifest["id"] = json!("other-image-input-fixture"));
    host.manager
        .install(&other, other.manifest.permissions.clone())
        .unwrap();
    host.preview("other-image-input-fixture", Some(document(7)))
        .unwrap();
    assert_eq!(
        host.save("other-image-input-fixture", &input.handle, "img.png")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
    assert!(
        host.manager
            .live
            .get_mut("image-input-fixture")
            .unwrap()
            .take_editor_requests()
            .is_empty()
    );
    host.probe(
        "image-input-fixture",
        json!({"method":"close_resource","handle":input.handle}),
    )
    .unwrap();
    assert_eq!(
        host.save("image-input-fixture", &input.handle, "img.png")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
}

/// Batch and resident limits are atomic; releasing tokens returns quota without passing pixels through WASM.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn image_input_batch_and_resident_quotas_release_with_owned_resources() {
    let mut host = Harness::new(&image_package(|_, _| {}));
    host.preview("image-input-fixture", Some(document(7)))
        .unwrap();
    for batch in [
        vec![],
        vec![image(Arc::new(vec![1])); 9],
        vec![image(Arc::new(vec![1; 8 * 1024 * 1024 + 1]))],
        vec![image(Arc::new(vec![1; 8 * 1024 * 1024])); 5],
    ] {
        assert!(
            host.offer("image-input-fixture", HostImageOrigin::Drop, batch)
                .is_err()
        );
    }
    let bytes = Arc::new(vec![1; 8 * 1024 * 1024]);
    let first = host
        .offer(
            "image-input-fixture",
            HostImageOrigin::Drop,
            vec![image(bytes.clone()); 4],
        )
        .unwrap();
    let second = host
        .offer(
            "image-input-fixture",
            HostImageOrigin::Drop,
            vec![image(bytes); 4],
        )
        .unwrap();
    assert_eq!(
        code(
            host.offer(
                "image-input-fixture",
                HostImageOrigin::Drop,
                vec![image(Arc::new(vec![1]))]
            )
            .unwrap_err()
        ),
        api::ErrorCode::LimitExceeded
    );
    for input in first.into_iter().chain(second) {
        host.probe(
            "image-input-fixture",
            json!({"method":"close_resource","handle":input.handle}),
        )
        .unwrap();
    }
    assert!(
        host.offer(
            "image-input-fixture",
            HostImageOrigin::Drop,
            vec![image(Arc::new(vec![1]))]
        )
        .is_ok()
    );
}

/// Opt-in publication is separately negotiated; capture requires workspace.write and clipboard grant by origin.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn image_input_declaration_and_capture_permissions_are_independent() {
    for (missing, expected) in [
        ("editor.images", api::ErrorCode::CapabilityUnavailable),
        ("editor.write", api::ErrorCode::PermissionDenied),
    ] {
        let package = image_package(|manifest, _| {
            if missing == "editor.images" {
                manifest["api"]["required"]
                    .as_object_mut()
                    .unwrap()
                    .remove(missing);
            } else {
                manifest["permissions"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|value| value != missing);
            }
        });
        let mut host = Harness::new(&package);
        assert_eq!(
            code(
                host.preview("image-input-fixture", Some(document(7)))
                    .unwrap_err()
            ),
            expected
        );
    }
    for missing in ["workspace.write", "clipboard"] {
        let package = image_package(|manifest, _| {
            manifest["permissions"]
                .as_array_mut()
                .unwrap()
                .retain(|value| value != missing)
        });
        let mut host = Harness::new(&package);
        host.preview("image-input-fixture", Some(document(7)))
            .unwrap();
        assert_eq!(
            code(
                host.offer(
                    "image-input-fixture",
                    HostImageOrigin::Clipboard,
                    vec![image(Arc::new(vec![1]))]
                )
                .unwrap_err()
            ),
            api::ErrorCode::PermissionDenied
        );
        if missing == "clipboard" {
            assert!(
                host.offer(
                    "image-input-fixture",
                    HostImageOrigin::Drop,
                    vec![image(Arc::new(vec![1]))]
                )
                .is_ok()
            );
        }
    }
    let mut host = Harness::new(&image_package(|_, _| {}));
    host.preview("image-input-fixture", Some(document(7)))
        .unwrap();
    assert_eq!(
        code(
            host.manager
                .event(
                    "image-input-fixture",
                    Some("welcome".into()),
                    api::Notification::ImageInput {
                        document: document(7),
                        selection: api::TextRange { start: 0, end: 0 },
                        images: Vec::new()
                    }
                )
                .unwrap_err()
        ),
        api::ErrorCode::InvalidRequest
    );
}
