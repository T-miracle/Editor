//! Observable archive/directory invariants are exercised through the shared public project module.
use super::*;
use std::{collections::BTreeMap, path::Path, sync::atomic::Ordering};

fn fixture(root: &Path) {
    std::fs::write(root.join("manifest.json"),r#"{"id":"test","name":"Test","version":"0.1.0","protocol":7,"api":{"base":"^1","required":{}},"contributions":"plugin.toml","permissions":[],"storage_limit":1024}"#).unwrap();
    std::fs::write(
        root.join("plugin.toml"),
        "[plugin]\nid='test'\nname='Test'\nversion='0.1.0'\nhost_version='>=0.1.0'\n",
    )
    .unwrap();
    std::fs::write(root.join("README.md"), "Resource-only package").unwrap();
    std::fs::write(root.join(DESCRIPTION),r#"{"version":1,"assets":[{"source":"plugin.toml","destination":"plugin.toml"},{"source":"README.md","destination":"README.md"}]}"#).unwrap();
}
/// Missing descriptors retain the exact path and typed I/O cause; malformed data is a separate error.
#[test]
fn description_read_failure_identifies_the_file_and_preserves_io_kind() {
    let root = tempfile::tempdir().unwrap();
    let error = Project::read(root.path()).err().unwrap();
    let description = error.downcast_ref::<DescriptionReadError>().unwrap();
    assert_eq!(description.source.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(
        description.path,
        root.path().canonicalize().unwrap().join(DESCRIPTION)
    );
    assert!(format!("{error:#}").contains(DESCRIPTION));
    std::fs::write(root.path().join(DESCRIPTION), b"invalid JSON").unwrap();
    let malformed = Project::read(root.path()).err().unwrap();
    assert!(malformed.downcast_ref::<DescriptionReadError>().is_none());
    assert!(format!("{malformed:#}").contains(DESCRIPTION));
    assert!(format!("{malformed:#}").contains(&root.path().display().to_string()));
}
/// Default output is the project root, ZIP entries have no outer folder, and source is excluded.
#[test]
fn package_default_root_and_directory_share_validation() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    std::fs::write(dir.path().join("private.txt"), "must not ship").unwrap();
    let project = Project::read(dir.path()).unwrap();
    let package = project
        .prepare(&BuildOptions::default(), &mut |_| {})
        .unwrap();
    let stage = stage(&package, &dir.path().join("dev")).unwrap();
    assert_eq!(read_directory(&stage).unwrap().digest, package.digest);
    assert!(!dir.path().join("test-0.1.0.zip").exists());
    let output = project
        .package(&BuildOptions::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        output,
        dir.path().canonicalize().unwrap().join("test-0.1.0.zip")
    );
    let zipped = crate::Package::read(&output).unwrap();
    assert_eq!(zipped.files, package.files);
    assert!(!zipped.files.contains_key("private.txt"));
    assert!(!zipped.files.contains_key(DESCRIPTION));
}
/// A failed rebuild keeps the previous distributable byte-for-byte.
#[test]
fn missing_asset_never_replaces_existing_zip() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let project = Project::read(dir.path()).unwrap();
    let output = project
        .package(&BuildOptions::default(), &mut |_| {})
        .unwrap();
    let previous = std::fs::read(&output).unwrap();
    std::fs::remove_file(dir.path().join("README.md")).unwrap();
    assert!(
        project
            .package(&BuildOptions::default(), &mut |_| {})
            .is_err()
    );
    assert_eq!(std::fs::read(output).unwrap(), previous);
}
/// Local/CLI output overrides the shared project default without rewriting the description.
#[test]
fn explicit_output_and_atomic_successful_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let outside = tempfile::tempdir().unwrap();
    let options = BuildOptions {
        output: Some(outside.path().into()),
        ..Default::default()
    };
    let project = Project::read(dir.path()).unwrap();
    let output = project.package(&options, &mut |_| {}).unwrap();
    std::fs::write(dir.path().join("README.md"), "new data").unwrap();
    assert_eq!(project.package(&options, &mut |_| {}).unwrap(), output);
    assert_eq!(
        crate::Package::read(&output).unwrap().files["README.md"],
        b"new data"
    );
    assert!(!dir.path().join("test-0.1.0.zip").exists());
}
/// Cancellation is checked before spawning or publishing.
#[test]
fn cancellation_and_duplicate_destinations_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let mut project = Project::read(dir.path()).unwrap();
    let options = BuildOptions::default();
    options.cancelled.store(true, Ordering::Release);
    assert!(project.package(&options, &mut |_| {}).is_err());
    options.cancelled.store(false, Ordering::Release);
    project
        .description
        .assets
        .push(project.description.assets[0].clone());
    assert!(project.prepare(&options, &mut |_| {}).is_err());
}
/// Directory inputs cannot escape the declared project through relative traversal or symlinks.
#[test]
fn unsafe_sources_and_foreign_native_steps_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let mut project = Project::read(dir.path()).unwrap();
    project.description.assets[0].source = "../outside".into();
    assert!(
        project
            .prepare(&BuildOptions::default(), &mut |_| {})
            .is_err()
    );
    project.description.assets.clear();
    project.description.native.push(BuildStep {
        program: "rustc".into(),
        args: vec![],
        platform: "other-architecture".into(),
    });
    assert!(
        project
            .prepare(&BuildOptions::default(), &mut |_| {})
            .is_err()
    );
}
/// In-memory development admission rejects the same retired protocol as ZIP installation.
#[test]
fn development_cannot_bypass_manifest_validation() {
    let files = BTreeMap::from([(
        "manifest.json".into(),
        br#"{"id":"test","protocol":6}"#.to_vec(),
    )]);
    assert!(crate::Package::from_files(files).is_err());
    assert!(
        crate::Package::from_files(BTreeMap::from([("../manifest.json".into(), vec![])])).is_err()
    );
}

/// Choosing a resource subdirectory as output cannot recursively include the previous ZIP.
#[test]
fn output_inside_asset_directory_remains_stable() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/example.txt"), "example").unwrap();
    let mut project = Project::read(dir.path()).unwrap();
    project.description.assets.push(Asset {
        source: "assets".into(),
        destination: "assets".into(),
    });
    let options = BuildOptions {
        output: Some(dir.path().join("assets")),
        ..Default::default()
    };
    let path = project.package(&options, &mut |_| {}).unwrap();
    let original = std::fs::read(&path).unwrap();
    project.package(&options, &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(
        !crate::Package::read(&path)
            .unwrap()
            .files
            .contains_key("assets/test-0.1.0.zip")
    );
}
