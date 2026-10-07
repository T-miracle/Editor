//! Legacy and reloaded preference archives exercise the public provider configuration boundary.
use super::*;
use serde_json::json;

/// An old file obtains finite proof from live candidates, while a hand edit cannot inherit automatic proof.
#[test]
fn legacy_formatter_archive_validates_explicit_values_and_preserves_normal_withdrawal() {
    let store = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let file = store.path().join("language-providers.json");
    // This represents an existing store with no new verification field at all.
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({"user":{"formatter:novel":"alpha/format"}})).unwrap(),
    )
    .unwrap();
    let provider: plugin_runtime::plugin_protocol::language::Provider =
        serde_json::from_value(json!({
            "id":"format","language":"novel","service":"format","primary":false,"formatting":true
        }))
        .unwrap();
    configure(store.path(), workspace.path());
    refresh(
        store.path(),
        Vec::new(),
        vec![
            ("alpha".into(), vec![provider.clone()]),
            ("beta".into(), vec![provider.clone()]),
        ],
    );
    assert_eq!(formatters()["novel"].as_deref(), Some("alpha/format"));
    assert!(formatter_error("novel").is_none());
    let migrated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        migrated["formatter_choices"]["user"]["formatter:novel"],
        json!("alpha/format")
    );

    refresh(
        store.path(),
        Vec::new(),
        vec![("beta".into(), vec![provider.clone()])],
    );
    configure(store.path(), other.path());
    configure(store.path(), workspace.path());
    refresh(
        store.path(),
        Vec::new(),
        vec![("beta".into(), vec![provider.clone()])],
    );
    assert_eq!(formatters()["novel"].as_deref(), Some("beta/format"));
    assert!(
        formatter_error("novel").is_none(),
        "sole fallback cannot overwrite the removed explicit choice's proof"
    );

    let mut edited: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    edited["user"]["formatter:novel"] = json!("typo/format");
    std::fs::write(&file, serde_json::to_vec(&edited).unwrap()).unwrap();
    configure(store.path(), other.path());
    configure(store.path(), workspace.path());
    refresh(
        store.path(),
        Vec::new(),
        vec![("beta".into(), vec![provider])],
    );
    assert!(formatters()["novel"].is_none());
    let error = formatter_error("novel").unwrap();
    assert_eq!(error.provider, "typo/format");
    assert_eq!(error.source, "user");
    // A per-key error never poisons the global load/write gate used by the normal Reset control.
    choose(Scope::User, "formatter:novel", None).unwrap();
    assert_eq!(formatters()["novel"].as_deref(), Some("beta/format"));
    assert!(formatter_error("novel").is_none());
    let cleared: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert!(
        cleared["formatter_choices"]["user"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}
