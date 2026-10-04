//! Discovery: the host applies a provider's declaration without knowing the provider's language.
use super::*;

/// A provider declaration as a plugin would ship it.
fn declaration(id: &str, target_type: &str, rules: &str) -> RunTargetDiscovery {
    let document = format!(
        r#"{{"version": 1, "id": "{id}", "name": "{id}", "target_type": "{target_type}", "program_field": "name",
            "rules": [{rules}]}}"#
    );
    RunTargetDiscovery::from_json(document.as_bytes())
        .unwrap_or_else(|error| panic!("the fixture declaration is valid: {error}"))
}

/// A Rust-shaped provider: a package name plus binary entries in one manifest.
fn rust_provider() -> RunTargetDiscovery {
    declaration(
        "rust",
        "rust-binary",
        r#"{
            "file": "Cargo.toml",
            "fields": [
                {"name": "package", "section": "package", "key": "name"},
                {"name": "bin", "section": "bin", "key": "name", "optional": true}
            ],
            "targets": [{"name_from": "package", "fields": ["package"]}]
        }"#,
    )
}

/// A second provider with a different type and different fields, so the check is about the host's
/// vocabulary rather than about one language's manifest.
fn script_provider() -> RunTargetDiscovery {
    declaration(
        "tools",
        "tool-command",
        r#"{
            "file": "tools/*.toml",
            "fields": [
                {"name": "command", "key": "command"},
                {"name": "usage", "key": "usage", "optional": true}
            ],
            "targets": [{"name_from": "command", "fields": ["command", "usage"]}]
        }"#,
    )
}

/// Read from an in-memory workspace.
fn workspace(files: &[(&str, &str)]) -> (Vec<String>, std::collections::BTreeMap<String, String>) {
    let contents = files
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect::<std::collections::BTreeMap<_, _>>();
    (contents.keys().cloned().collect(), contents)
}

/// A plugin's contribution file names its discovery declarations, and each is read the same way.
#[test]
fn a_plugin_declares_its_discovery_files_in_its_contributions() {
    let contributions = r#"
[plugin]
id = "rust"
name = "Rust"
version = "0.3.0"
host_version = ">=0.1.0"

[[run_targets]]
id = "rust-binary"
file = "run-targets/rust.json"
"#;
    let manifest = crate::PluginManifest::parse(contributions).expect("the file is valid");
    assert_eq!(manifest.run_targets.len(), 1);
    assert_eq!(manifest.run_targets[0].id, "rust-binary");
    assert_eq!(
        manifest.run_targets[0].file,
        std::path::PathBuf::from("run-targets/rust.json")
    );
    // A field the host does not define is refused rather than silently dropped.
    assert!(
        crate::PluginManifest::parse(
            "[plugin]\nid = \"x\"\nname = \"x\"\nversion = \"1.0.0\"\nhost_version = \">=0.1.0\"\n\
             [[run_targets]]\nid = \"x\"\nfile = \"a.json\"\nmode = \"magic\"\n"
        )
        .is_err()
    );
}

/// A Rust project's own manifest is recognized by the provider that declared how to read it.
#[test]
fn a_rust_manifest_yields_the_package_target() {
    let (files, contents) = workspace(&[(
        "Cargo.toml",
        "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\n",
    )]);
    let targets = rust_provider().discover(&files, |path| contents.get(path).cloned());
    assert_eq!(targets.len(), 1);
    let target = &targets[0];
    assert_eq!(target.id, "rust:my-app");
    assert_eq!(target.provider, "rust");
    assert_eq!(target.target_type, "rust-binary");
    assert_eq!(target.label, "my-app");
    assert_eq!(
        target.fields.get("package").map(String::as_str),
        Some("my-app")
    );
    assert_eq!(target.found_in, "Cargo.toml");
}

/// A second provider with its own type and fields is discovered by the same host code.
#[test]
fn a_second_provider_type_is_discovered_without_host_knowledge_of_it() {
    let (files, contents) = workspace(&[
        (
            "tools/fmt.toml",
            "command = \"cargo fmt\"\nusage = \"format\"\n",
        ),
        ("tools/lint.toml", "command = \"cargo clippy\"\n"),
        // A file the provider's pattern does not name is not offered.
        ("other/fmt.toml", "command = \"ignored\"\n"),
    ]);
    let targets = script_provider().discover(&files, |path| contents.get(path).cloned());
    let labels = targets
        .iter()
        .map(|target| (target.label.as_str(), target.target_type.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        vec![
            ("cargo fmt", "tool-command"),
            ("cargo clippy", "tool-command")
        ]
    );
    // A field the provider marked optional is reported when present and simply absent otherwise.
    let format = targets
        .iter()
        .find(|target| target.label == "cargo fmt")
        .unwrap();
    assert_eq!(
        format.fields.get("usage").map(String::as_str),
        Some("format")
    );
    let lint = targets
        .iter()
        .find(|target| target.label == "cargo clippy")
        .unwrap();
    assert!(!lint.fields.contains_key("usage"));
    // The identity is the provider's namespace plus the target's own name, so two providers cannot
    // collide and a repeat discovery offers the same identity.
    let again = script_provider().discover(&files, |path| contents.get(path).cloned());
    assert_eq!(
        targets.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
        again.iter().map(|t| t.id.clone()).collect::<Vec<_>>()
    );
}

/// A file that is not readable as the declared shape simply offers nothing.
#[test]
fn files_that_do_not_match_the_declared_shape_offer_nothing() {
    let (files, contents) = workspace(&[
        // No [package] table at all, so the required field is missing.
        ("Cargo.toml", "[workspace]\nmembers = [\"a\"]\n"),
    ]);
    assert!(
        rust_provider()
            .discover(&files, |path| contents.get(path).cloned())
            .is_empty()
    );
    // A file that exists but cannot be read offers nothing either, rather than failing the walk.
    let (files, _) = workspace(&[("Cargo.toml", "")]);
    assert!(rust_provider().discover(&files, |_| None).is_empty());
    // A missing file is not an error: a project without the manifest simply has no targets.
    assert!(rust_provider().discover(&[], |_| None).is_empty());
}

/// The host refuses a declaration it could not honour, instead of guessing at its meaning.
#[test]
fn an_unusable_declaration_is_refused() {
    // A target named from a field the rule never reads could never be identified.
    let error = RunTargetDiscovery::from_json(
        br#"{"id":"x","name":"x","target_type":"t","program_field":"name","rules":[
            {"file":"a.toml","fields":[{"name":"one","key":"one"}],
             "targets":[{"name_from":"two"}]}]}"#,
    )
    .expect_err("a target named from an unknown field is refused");
    assert!(error.to_string().contains("unknown field"), "{error}");

    // An absolute path would reach outside the workspace the provider was given.
    let error = RunTargetDiscovery::from_json(
        br#"{"id":"x","name":"x","target_type":"t","program_field":"name","rules":[
            {"file":"C:/elsewhere/a.toml","fields":[{"name":"one","key":"one"}]}]}"#,
    )
    .expect_err("an absolute rule file is refused");
    assert!(error.to_string().contains("workspace-relative"), "{error}");

    // A newer declaration is refused rather than read with fields this build does not implement.
    let error = RunTargetDiscovery::from_json(
        br#"{"version":99,"id":"x","name":"x","target_type":"t","program_field":"name","rules":[
            {"file":"a.toml","fields":[{"name":"one","key":"one"}]}]}"#,
    )
    .expect_err("a newer declaration is refused");
    assert!(
        matches!(error, DiscoveryError::UnsupportedVersion { .. }),
        "{error}"
    );

    // A field this build does not define is not silently ignored.
    assert!(
        RunTargetDiscovery::from_json(
            br#"{"id":"x","name":"x","target_type":"t","program_field":"name","rules":[
                {"file":"a.toml","fields":[{"name":"one","key":"one","transform":"upper"}]}]}"#,
        )
        .is_err()
    );
}

/// A pattern reaches only the files it names.
#[test]
fn declared_patterns_stay_inside_the_paths_they_name() {
    assert!(pattern_matches("Cargo.toml", "Cargo.toml"));
    assert!(pattern_matches("tools/*.toml", "tools/fmt.toml"));
    // One segment per wildcard, so a nested file is not matched by a single-level pattern.
    assert!(!pattern_matches("tools/*.toml", "tools/nested/fmt.toml"));
    assert!(!pattern_matches("*.toml", "tools/fmt.toml"));
    // A tree pattern is written explicitly.
    assert!(pattern_matches(
        "crates/**/Cargo.toml",
        "crates/a/b/Cargo.toml"
    ));
    assert!(pattern_matches(
        "crates/**/Cargo.toml",
        "crates/a/Cargo.toml"
    ));
    assert!(!pattern_matches(
        "crates/**/Cargo.toml",
        "other/a/Cargo.toml"
    ));
    // Windows separators are normalized so one declaration works on either platform.
    assert!(pattern_matches("tools/*.toml", "tools\\fmt.toml"));
}
