//! An unfamiliar language exercises structure authority and hostile replies through the public guest SDK.
use plugin_protocol::{
    api::{self, TextRange},
    structure::{Icon, Node, Proposal, Request},
};

/// Probe actual denied operations and return an opaque custom definition kind; host code knows none of its policy.
pub(super) fn describe(request: Request) -> Result<api::Output, api::Failure> {
    if api::guest::open_workspace().is_ok()
        || api::guest::open_data().is_ok()
        || api::guest::read_asset("manifest.json").is_ok()
    {
        return Err(api::Failure::new(
            api::ErrorCode::OperationFailed,
            "Structure worker escaped pure authority",
        ));
    }
    let text = &request.source.text;
    // An actual WASM trap must retire the pure lease and cannot leave its resource count or old tree alive.
    if text == "trap" {
        panic!("intentional pure structure trap");
    }
    let range = TextRange {
        start: 0,
        end: text.len(),
    };
    let mut proposal = Proposal {
        request: request.request,
        document: request.source.document.clone(),
        folds: vec![],
        nodes: vec![Node {
            name: "纯结构定义🙂".into(),
            kind: "unfamiliar-definition".into(),
            icon: Some(Icon {
                light: "icons/structure-missing.svg".into(),
                dark: None,
            }),
            range,
            definition: range,
            children: vec![],
        }],
    };
    match text.as_str() {
        "stale" => proposal.document.revision += 1,
        "bad-range" => proposal.nodes[0].definition.end += 1,
        "中文" => proposal.nodes[0].definition.end = 1,
        "escape-icon" => proposal.nodes[0].icon.as_mut().unwrap().light = "../foreign.svg".into(),
        "unsafe-svg" => {
            proposal.nodes[0].icon.as_mut().unwrap().light = "icons/structure-unsafe.svg".into()
        }
        "excessive" => proposal.nodes = vec![proposal.nodes[0].clone(); 4097],
        "overdepth" => {
            // Typed proposals can still be hostile: exceed the public depth before any host tree is rendered.
            let leaf = proposal.nodes[0].clone();
            let mut nested = leaf.clone();
            for _ in 1..140 {
                let mut parent = leaf.clone();
                parent.children.push(nested);
                nested = parent;
            }
            proposal.nodes = vec![nested];
        }
        _ => {}
    }
    Ok(api::Output {
        language_structure: Some(proposal),
        ..Default::default()
    })
}
