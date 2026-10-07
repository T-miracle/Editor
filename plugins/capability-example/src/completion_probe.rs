//! Public SDK fixture proves that pure snapshot execution cannot borrow ordinary package authority.
use plugin_protocol::{
    api::{self, ErrorCode, Failure},
    language::{CompletionItem, CompletionProposal, CompletionRequest},
};

/// Try real SDK calls despite ordinary grants; every pure-worker call must be denied before side effects.
pub(super) fn complete(request: CompletionRequest) -> Result<api::Output, Failure> {
    for operation in [
        api::Operation::ReadAsset {
            path: "welcome.txt".into(),
        },
        api::Operation::OpenWorkspace,
        api::Operation::OpenData,
        api::Operation::SubscribeDocuments,
        api::Operation::Editor {
            operation: api::EditorOperation::ReadSelection,
            timeout_ms: 1000,
        },
    ] {
        if !matches!(api::guest::request(operation), Err(error) if error.code == ErrorCode::PermissionDenied)
        {
            return Err(Failure::new(
                ErrorCode::OperationFailed,
                "Pure snapshot worker gained host resource authority",
            ));
        }
    }
    let mut proposal = CompletionProposal {
        request: request.request,
        document: request.source.document,
        items: vec![CompletionItem {
            label: "pure-worker-denied-io".into(),
            new_text: request
                .settings
                .get("label")
                .and_then(|value| value.value.as_str())
                .unwrap_or("pure")
                .into(),
            replace: api::TextRange {
                start: request.cursor,
                end: request.cursor,
            },
        }],
    };
    // Malicious replies use the real independently built component to exercise host validation.
    match request.source.text.as_str() {
        "stale" => proposal.document.revision = proposal.document.revision.saturating_add(1),
        "bad-range" => proposal.items[0].replace.end = request.source.text.len() + 1,
        "excessive" => proposal.items = vec![proposal.items[0].clone(); 257],
        // A fuel trap must remain a failed supplement, without stopping a healthy native transport.
        "fuel" => loop {
            std::hint::spin_loop();
        },
        _ => {}
    }
    Ok(api::Output {
        language_completion: Some(proposal),
        ..Default::default()
    })
}
