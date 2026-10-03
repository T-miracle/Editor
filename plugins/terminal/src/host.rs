//! Typed SDK calls keep resource ownership and asynchronous completion separate from terminal interaction.
use plugin_protocol::{api, process};

/// Native unit fixtures use the same typed request shape as the component import.
pub(super) fn call(operation: api::Operation) -> Result<api::Value, api::Failure> {
    #[cfg(test)]
    {
        crate::tests::host(operation)
    }
    #[cfg(not(test))]
    {
        api::guest::request(operation)
    }
}

/// User-facing errors retain the typed boundary until decisions about missing files are finished.
pub(super) fn request(operation: api::Operation) -> Result<api::Value, String> {
    call(operation).map_err(|error| error.to_string())
}

/// Short-lived filesystem handles cannot consume the instance quota as settings are reloaded.
pub(super) fn read(path: &str, private: bool) -> Result<String, String> {
    read_optional(path, private)?.ok_or_else(|| format!("File not found: {path}"))
}

/// Only NotFound means a fresh configuration; denied or corrupt data must never be overwritten.
pub(super) fn read_optional(path: &str, private: bool) -> Result<Option<String>, String> {
    let root = call(if private {
        api::Operation::OpenData
    } else {
        api::Operation::OpenWorkspace
    });
    let api::Value::Resource(handle) = (match root {
        Ok(value) => value,
        Err(error) if error.code == api::ErrorCode::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }) else {
        return Err("Expected filesystem resource".into());
    };
    let result = call(api::Operation::ReadFile {
        handle: handle.clone(),
        path: path.into(),
    });
    let _ = request(api::Operation::CloseResource { handle });
    let value = match result {
        Ok(value) => value,
        Err(error) if error.code == api::ErrorCode::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    match value {
        api::Value::Bytes(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|error| error.to_string()),
        _ => Err("Expected file bytes".into()),
    }
}

/// Private settings writes use the manager's staged data root during update/rollback transactions.
pub(super) fn write_data(path: &str, text: String) -> Result<(), String> {
    let api::Value::Resource(handle) = request(api::Operation::OpenData)? else {
        return Err("Expected private data resource".into());
    };
    let result = request(api::Operation::WriteFile {
        handle: handle.clone(),
        path: path.into(),
        bytes: text.into_bytes(),
    });
    let _ = request(api::Operation::CloseResource { handle });
    result.map(|_| ())
}

pub(super) fn process(operation: process::Operation) -> Result<api::Value, String> {
    request(api::Operation::Process { operation })
}

/// Callers retain the returned identity until an actual terminal completion, not an acknowledgement.
pub(super) fn editor(operation: api::EditorOperation) -> Result<api::ResourceHandle, String> {
    match request(api::Operation::Editor {
        operation,
        timeout_ms: 30_000,
    })? {
        api::Value::Accepted(handle) => Ok(handle),
        _ => Err("Expected asynchronous editor request".into()),
    }
}
