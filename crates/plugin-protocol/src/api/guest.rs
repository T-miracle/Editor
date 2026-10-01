//! Guest conveniences keep wire encoding and request bookkeeping out of plugin logic.
use super::*;
use crate::bindings::editor::plugin::host;
use std::cell::Cell;

thread_local! {
    /// A component instance dispatches serially, so IDs never need a cross-instance global.
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

/// Read an installed-package resource through the negotiated, permission-checked host API.
pub fn read_asset(path: impl Into<String>) -> Result<Vec<u8>, Failure> {
    let path = path.into();
    // Reject obviously oversized paths before allocating their JSON representation.
    check_request_size(path.len())?;
    let id = NEXT_ID.with(|next| {
        let id = next.get();
        let following = id
            .checked_add(1)
            .ok_or_else(|| Failure::new(ErrorCode::LimitExceeded, "Request IDs exhausted"))?;
        next.set(following);
        Ok::<_, Failure>(id)
    })?;
    let request = Request {
        id,
        operation: Operation::ReadAsset { path },
    };
    let payload = serde_json::to_string(&request).map_err(wire_error)?;
    // Escaping and envelope fields also count toward the shared transport quota.
    check_request_size(payload.len())?;
    let result = host::request(&payload).map_err(wire_error)?;
    let response: Response = serde_json::from_str(&result).map_err(wire_error)?;
    if response.id != id {
        return Err(Failure::new(
            ErrorCode::InvalidRequest,
            "Host response ID mismatch",
        ));
    }
    let Value::Asset { bytes } = response.result?;
    Ok(bytes)
}

/// Preserve the quota error instead of sending an envelope the host cannot correlate.
fn check_request_size(bytes: usize) -> Result<(), Failure> {
    if bytes > MAX_REQUEST_BYTES {
        return Err(Failure::new(
            ErrorCode::LimitExceeded,
            "Host request too large",
        ));
    }
    Ok(())
}

/// Decode one lifecycle call and return a correlated, typed completion to the host.
pub fn dispatch(
    payload: &str,
    handle: impl FnOnce(Input) -> Result<Output, Failure>,
) -> Result<String, String> {
    let invocation: Invocation =
        serde_json::from_str(payload).map_err(|error| error.to_string())?;
    let result = if invocation.id == 0 {
        Err(Failure::new(
            ErrorCode::InvalidRequest,
            "Invocation ID must be nonzero",
        ))
    } else {
        handle(invocation.message)
    };
    serde_json::to_string(&Completion {
        id: invocation.id,
        result,
    })
    .map_err(|error| error.to_string())
}

/// Encoding and transport failures are distinct from a host's returned domain failure.
fn wire_error(error: impl std::fmt::Display) -> Failure {
    Failure::new(ErrorCode::OperationFailed, error.to_string())
}
