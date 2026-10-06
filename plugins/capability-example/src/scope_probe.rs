//! Bound diagnostic text construction before issuing an ordinary public capability request.
use plugin_protocol::api::{self, EditorOperation, ErrorCode, Failure};

/// Parse normal Operation JSON; optional repeat_text constructs a bounded text quota probe inside the guest.
pub(super) fn operation(mut arguments: serde_json::Value) -> Result<api::Operation, Failure> {
    let repetitions = arguments
        .as_object_mut()
        .and_then(|arguments| arguments.remove("repeat_text"));
    let mut operation = serde_json::from_value(arguments)
        .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
    if let Some(repetitions) = repetitions {
        let count = repetitions
            .as_u64()
            .filter(|count| *count <= 1024 * 1024 + 1)
            .ok_or_else(|| {
                Failure::new(ErrorCode::InvalidRequest, "Invalid text repetition count")
            })? as usize;
        let text = match &mut operation {
            api::Operation::Editor {
                operation:
                    EditorOperation::ReplaceDocumentRange { text, .. }
                    | EditorOperation::WriteClipboard { text },
                ..
            } => text,
            _ => {
                return Err(Failure::new(
                    ErrorCode::InvalidRequest,
                    "Text repetition requires a text-writing editor operation",
                ));
            }
        };
        // One byte beyond the public text quota is enough; never permit an unbounded allocation.
        if !text
            .len()
            .checked_mul(count)
            .is_some_and(|bytes| bytes <= 1024 * 1024 + 1)
        {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Repeated text exceeds the diagnostic bound",
            ));
        }
        *text = text.repeat(count);
    }
    Ok(operation)
}
