//! Tree behavior is tested through stored configuration sets, including transaction scope and cycles.
use super::*;

/// Build domain-neutral configuration records without invoking or embedding provider business logic.
fn configuration(set: &mut RunConfigSet, id: &str, name: &str) {
    set.upsert(
        serde_json::from_value(
            serde_json::json!({"id":id,"name":name,"target":{"mode":"program","program":"probe"}}),
        )
        .unwrap(),
    )
    .unwrap();
    set.plugin_configurations.insert(
        id.into(),
        PluginConfiguration {
            provider: "provider".into(),
            template: "command".into(),
            values: serde_json::json!({"label":name}).to_string(),
            pending_events: vec![],
            name: name.into(),
            program: "probe".into(),
            revision: 0,
            validation: ConfigurationValidation::Unchecked,
        },
    );
}
fn folder(set: &mut RunConfigSet, id: &str, parent: Option<&str>) {
    set.tree
        .folders
        .insert(id.into(), ConfigurationFolder { name: id.into() });
    set.place_tree_node(id, parent.map(str::to_owned), None)
        .unwrap();
}

/// Reordering persists, folders remain first, cycle attempts are atomic, and removal clears descendants.
#[test]
fn configuration_tree_order_cycles_storage_and_recursive_delete() {
    let mut set = RunConfigSet::default();
    configuration(&mut set, "one", "first");
    configuration(&mut set, "two", "second");
    folder(&mut set, "group", None);
    folder(&mut set, "nested", Some("group"));
    set.place_tree_node("one", Some("nested".into()), None)
        .unwrap();
    set.place_tree_node("two", None, None).unwrap();
    assert_eq!(set.tree_children(None), ["group", "two"]);
    assert_eq!(set.insertion_parent(Some("one")).as_deref(), Some("nested"));
    assert_eq!(
        set.insertion_parent(Some("group")).as_deref(),
        Some("group")
    );
    let unchanged = set.to_json().unwrap();
    assert!(
        set.place_tree_node("group", Some("nested".into()), None)
            .is_err()
    );
    assert_eq!(set.to_json().unwrap(), unchanged);
    set.place_tree_node("one", None, Some("two")).unwrap();
    let mut restored = RunConfigSet::from_json(&set.to_json().unwrap()).unwrap();
    assert_eq!(restored.tree_children(None), ["group", "one", "two"]);
    restored
        .place_tree_node("one", Some("nested".into()), None)
        .unwrap();
    restored.selected = Some("one".into());
    restored.remove_tree_node("group");
    assert!(restored.selected.is_none());
    assert!(restored.find("one").is_none());
    assert!(restored.find("two").is_some());
    assert!(restored.tree.folders.is_empty());
}

/// Apply commits exactly the chosen record and its necessary path; other drafts and selection stay separate.
#[test]
fn configuration_tree_apply_merges_only_required_ancestors() {
    let mut baseline = RunConfigSet::default();
    configuration(&mut baseline, "old", "saved");
    baseline.select("old");
    let mut draft = baseline.clone();
    draft.plugin_configurations.get_mut("old").unwrap().name = "unapplied".into();
    folder(&mut draft, "ancestor", None);
    folder(&mut draft, "necessary", Some("ancestor"));
    folder(&mut draft, "unrelated", None);
    configuration(&mut draft, "current", "applied");
    configuration(&mut draft, "other", "unapplied");
    draft
        .place_tree_node("current", Some("necessary".into()), None)
        .unwrap();
    baseline
        .apply_tree_configuration(&draft, "current")
        .unwrap();
    let stored = RunConfigSet::from_json(&baseline.to_json().unwrap()).unwrap();
    assert_eq!(stored.tree_ancestors("current"), ["ancestor", "necessary"]);
    assert!(!stored.tree.folders.contains_key("unrelated"));
    assert!(stored.find("other").is_none());
    assert_eq!(stored.plugin_configurations["old"].name, "saved");
    assert_eq!(stored.selected.as_deref(), Some("old"));
}

/// Corrupt or ambiguous persisted hierarchies are rejected at the same storage boundary as real writes.
#[test]
fn configuration_tree_rejects_orphans_cycles_and_duplicate_configuration_ids() {
    let mut set = RunConfigSet::default();
    folder(&mut set, "group", None);
    configuration(&mut set, "one", "first");
    set.place_tree_node("one", Some("group".into()), None)
        .unwrap();
    let mut invalid = set.clone();
    invalid.tree.placements.get_mut("group").unwrap().parent = Some("group".into());
    assert!(RunConfigSet::from_json(&invalid.to_json().unwrap()).is_err());
    let mut invalid = set.clone();
    invalid.tree.folders.clear();
    assert!(RunConfigSet::from_json(&invalid.to_json().unwrap()).is_err());
    set.configurations.push(set.configurations[0].clone());
    assert!(RunConfigSet::from_json(&set.to_json().unwrap()).is_err());
}
