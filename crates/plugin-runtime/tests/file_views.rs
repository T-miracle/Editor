//! Real Image packages exercise file-only authority through the public manager.
use plugin_runtime::{
    ImageState, Manager, Package,
    plugin_protocol::{Environment, api, ui},
};
use std::{
    io::{Cursor, Write},
    path::Path,
    time::{Duration, Instant},
};

/// An alternative package identity proves that file routing does not depend on a bundled ID.
fn image_package(id: &str) -> Package {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/svg.zip");
    let mut files = Package::read(&path)
        .expect("build Image with build-plugins.ps1 -Packages svg")
        .files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = id.into();
    // Resource declarations follow the same independent identity as its inspected executable manifest.
    let declaration = String::from_utf8(files["plugin.toml"].clone()).unwrap();
    files.insert(
        "plugin.toml".into(),
        declaration
            .replacen("id = \"svg\"", &format!("id = \"{id}\""), 1)
            .into_bytes(),
    );
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&writer.finish().unwrap().into_inner()).unwrap()
}

/// Wait only for the resource loader; this does not pretend to validate native image decoding.
fn ready(manager: &mut Manager, key: &str) -> std::sync::Arc<Vec<u8>> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let images = manager.image_resources();
        match &images[key].state {
            ImageState::Ready(bytes) => return bytes.clone(),
            ImageState::Failed(error) => panic!("unexpected image failure: {error:?}"),
            ImageState::Loading => assert!(Instant::now() < until, "file resource timed out"),
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Closing, retrying, replacing and revoking a viewer all retain exact file-resource ownership.
#[test]
#[ignore = "build current Image package with scripts/build-plugins.ps1 -Packages svg first"]
fn image_file_resources_follow_public_file_context_and_permissions() {
    let workspace = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    // Resource loading is encoding-independent. Pixel decoding is verified by native UI tests.
    let bytes = b"opaque image bytes, including invalid UTF-8: \xff";
    std::fs::write(workspace.path().join("photo#1.png"), bytes).unwrap();
    for id in ["svg", "independent-image-viewer"] {
        let package = image_package(id);
        let environment = Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        };
        let mut manager = Manager::open(data.path().join(id), environment).unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let file = api::FileContext {
            version: api::FileVersion {
                id: "opened-file".into(),
                path: "photo#1.png".into(),
                revision: 2,
            },
            file_type: "png".into(),
            text: None,
        };
        let send = |manager: &mut Manager, file| {
            manager.event(
                id,
                Some("preview".into()),
                api::Notification::FilePreview { file },
            )
        };
        send(&mut manager, Some(file.clone())).unwrap();
        let view = &manager.live[id].views["preview"];
        assert!(view.source.is_none());
        assert_eq!(view.file.as_ref(), Some(&file.version));
        assert!(matches!(
            view.root.kind,
            ui::Kind::FileImage {
                sizing: ui::ImageSizing::OriginalContain,
                ..
            }
        ));
        let key = format!("{id}/preview/image/image");
        assert_eq!(&*ready(&mut manager, &key), bytes);
        let mut stale = file.clone();
        stale.version.revision = 1;
        let error = send(&mut manager, Some(stale)).unwrap_err();
        assert_eq!(
            error.downcast_ref::<api::Failure>().unwrap().code,
            api::ErrorCode::StaleRevision
        );
        send(&mut manager, None).unwrap();
        assert!(manager.image_resources().is_empty());
        let mut reopened = file.clone();
        reopened.version.id = "reopened-file".into();
        reopened.version.revision = 0;
        send(&mut manager, Some(reopened.clone())).unwrap();
        assert_eq!(&*ready(&mut manager, &key), bytes);
        manager
            .installed
            .get_mut(id)
            .unwrap()
            .grants
            .remove("workspace.read");
        let images = manager.image_resources();
        assert!(
            matches!(&images[&key].state, ImageState::Failed(error) if error.code == api::ErrorCode::PermissionDenied)
        );
        let mut invalid = reopened;
        invalid.version.path = "../outside.png".into();
        assert!(send(&mut manager, Some(invalid)).is_err());
        manager
            .installed
            .get_mut(id)
            .unwrap()
            .grants
            .remove("editor.read");
        assert!(send(&mut manager, Some(file)).is_err());
        assert!(manager.image_resources().is_empty());
        assert!(
            manager.live.contains_key(id),
            "admission rejection must not kill a healthy guest"
        );
    }
}
