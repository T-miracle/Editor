//! Menu conditions use immutable user context and cannot confer any execution authority.
use plugin_protocol::commands::{Condition, Context};

/// Selection-specific actions remain hidden when the editor has no selection, despite a language match.
#[test]
fn menu_condition_requires_all_declared_context_fields() {
    let condition = Condition {
        has_selection: Some(true),
        language: Some("sample".into()),
        ..Default::default()
    };
    let mut context = Context {
        language: Some("sample".into()),
        ..Default::default()
    };
    assert!(!condition.matches(&context));
    context.has_selection = true;
    assert!(condition.matches(&context));
}

/// Public command results use the same bounded closed schemas as plugin collaboration.
#[test]
fn command_contract_refuses_unbounded_result_strings() {
    let command: plugin_protocol::Command = serde_json::from_value(serde_json::json!({
        "id":"echo", "title":"Echo", "signature":{
            "parameters":{"type":"null"}, "result":{"type":"string","max_bytes":1000000}, "permissions":[]
        }
    })).unwrap();
    assert!(command.validate().is_err());
}
