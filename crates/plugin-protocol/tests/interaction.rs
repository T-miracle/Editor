//! Public interaction declarations reject ambiguous choices before a native dialog is opened.
use plugin_protocol::interaction::{Operation, PickItem};

/// Stable item identities, rather than labels, determine the value returned by a quick pick.
#[test]
fn quick_pick_rejects_duplicate_item_ids() {
    let request = Operation::QuickPick {
        title: "Choose a target".into(),
        items: vec![
            PickItem {
                id: "one".into(),
                label: "First".into(),
                description: None,
            },
            PickItem {
                id: "one".into(),
                label: "Second".into(),
                description: None,
            },
        ],
    };
    assert!(request.validate().is_err());
}

/// A password prompt must still respect the same byte budget as ordinary Unicode input.
#[test]
fn input_cannot_supply_a_default_larger_than_its_result_budget() {
    let request = Operation::Input {
        title: "Enter a key".into(),
        value: "中文".into(),
        placeholder: None,
        password: true,
        max_bytes: 4,
    };
    assert!(request.validate().is_err());
}
