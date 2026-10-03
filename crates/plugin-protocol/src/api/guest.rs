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
    match request(Operation::ReadAsset { path })? {
        Value::Asset { bytes } => Ok(bytes),
        _ => Err(wire_error("Unexpected asset result")),
    }
}

/// Open only the caller's workspace; application-scoped guests have no implicit workspace.
pub fn open_workspace() -> Result<ResourceHandle, Failure> {
    match request(Operation::OpenWorkspace)? {
        Value::Resource(handle) => Ok(handle),
        _ => Err(wire_error("Unexpected workspace result")),
    }
}

/// Find ignored-rule-aware relative files beneath this caller's workspace handle.
pub fn find_files(handle: &ResourceHandle, query: FileQuery) -> Result<FileMatches, Failure> {
    match request(Operation::FindFiles {
        handle: handle.clone(),
        query,
    })? {
        Value::Files(files) => Ok(files),
        _ => Err(wire_error("Unexpected file discovery result")),
    }
}

/// Describe immutable host SDK inputs for a separately authorized native toolchain.
pub fn describe_sdk() -> Result<SdkDescriptor, Failure> {
    match request(Operation::DescribeSdk)? {
        Value::Sdk(sdk) => Ok(sdk),
        _ => Err(wire_error("Unexpected SDK description")),
    }
}

/// Open this instance scope's private files, distinct from user configuration and snapshots.
pub fn open_data() -> Result<ResourceHandle, Failure> {
    match request(Operation::OpenData)? {
        Value::Resource(handle) => Ok(handle),
        _ => Err(wire_error("Unexpected data result")),
    }
}

/// Read a relative path under an owned workspace or private-data handle.
pub fn read_file(handle: &ResourceHandle, path: impl Into<String>) -> Result<Vec<u8>, Failure> {
    match request(Operation::ReadFile {
        handle: handle.clone(),
        path: path.into(),
    })? {
        Value::Bytes(bytes) => Ok(bytes),
        _ => Err(wire_error("Unexpected file result")),
    }
}

/// Writes are available only to private-data handles, never workspace handles.
pub fn write_file(
    handle: &ResourceHandle,
    path: impl Into<String>,
    bytes: Vec<u8>,
) -> Result<(), Failure> {
    match request(Operation::WriteFile {
        handle: handle.clone(),
        path: path.into(),
        bytes,
    })? {
        Value::Unit => Ok(()),
        _ => Err(wire_error("Unexpected write result")),
    }
}

/// Explicit release invalidates the handle; instance retirement releases all remaining handles.
pub fn close_resource(handle: ResourceHandle) -> Result<(), Failure> {
    match request(Operation::CloseResource { handle })? {
        Value::Unit => Ok(()),
        _ => Err(wire_error("Unexpected release result")),
    }
}

/// Close the returned resource to release its bounded latest-version stream.
pub fn subscribe_documents() -> Result<ResourceHandle, Failure> {
    match request(Operation::SubscribeDocuments)? {
        Value::Resource(handle) => Ok(handle),
        _ => Err(wire_error("Expected document subscription")),
    }
}

/// All typed capabilities share one bounded and correlated transport.
pub fn editor(operation: EditorOperation, timeout_ms: u32) -> Result<ResourceHandle, Failure> {
    match request(Operation::Editor {
        operation,
        timeout_ms,
    })? {
        Value::Accepted(handle) => Ok(handle),
        _ => Err(wire_error("Expected request acceptance")),
    }
}

/// Correlation IDs are transport details; callers receive typed values or failures.
pub fn request(operation: Operation) -> Result<Value, Failure> {
    let id = NEXT_ID.with(|next| {
        let id = next.get();
        let following = id
            .checked_add(1)
            .ok_or_else(|| Failure::new(ErrorCode::LimitExceeded, "Request IDs exhausted"))?;
        next.set(following);
        Ok::<_, Failure>(id)
    })?;
    let request = Request { id, operation };
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
    response.result
}

/// Cancellation reports whether side effects may already have started; it never promises rollback.
pub fn cancel_request(
    handle: &ResourceHandle,
    mode: CancelMode,
) -> Result<CancellationEffect, Failure> {
    match request(Operation::CancelRequest {
        handle: handle.clone(),
        mode,
    })? {
        Value::Cancellation(effect) => Ok(effect),
        _ => Err(wire_error("Expected cancellation outcome")),
    }
}

/// Own the correlation state for one UI intent; replacing the task makes older completions irrelevant.
pub struct EditorTask {
    handle: ResourceHandle,
    terminal: bool,
}
impl EditorTask {
    /// Start a typed operation without exposing transport request IDs to plugin authors.
    pub fn start(operation: EditorOperation, timeout_ms: u32) -> Result<Self, Failure> {
        editor(operation, timeout_ms).map(Self::from_accepted)
    }
    /// Adapt an already accepted request when using the lower-level typed transport.
    pub fn from_accepted(handle: ResourceHandle) -> Self {
        Self {
            handle,
            terminal: false,
        }
    }
    /// Ignore unrelated and post-terminal updates, including results from a replaced UI intent.
    pub fn update(&mut self, notification: &Notification) -> Option<RequestUpdate> {
        let Notification::Request { handle, update } = notification else {
            return None;
        };
        if self.terminal || *handle != self.handle {
            return None;
        }
        self.terminal = update.is_terminal();
        Some(update.clone())
    }
    /// The host reports whether execution was prevented or only result waiting stopped.
    pub fn cancel(&self, mode: CancelMode) -> Result<CancellationEffect, Failure> {
        cancel_request(&self.handle, mode)
    }
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
