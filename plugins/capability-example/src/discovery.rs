//! Public discovery fixture keeps workspace file authority separate from native SDK metadata.
use plugin_protocol::{api, language};

/// Installation preparation and active language discovery execute the same read-only guest code.
pub(super) fn language_service() -> Result<api::Output, api::Failure> {
    let sdk = api::guest::describe_sdk()?;
    let workspace = api::guest::open_workspace()?;
    let files = api::guest::find_files(
        &workspace,
        api::FileQuery {
            include: vec!["**/*.marker".into()],
            exclude: vec!["**/generated/**".into()],
            max_results: 32,
        },
    )?;
    // A hook may release its own temporary roots; the host still releases any root left at return.
    let released = api::guest::open_workspace()?;
    api::guest::close_resource(released)?;
    let write = api::guest::write_file(
        &workspace,
        "hook-must-not-write.marker",
        b"forbidden".to_vec(),
    );
    if !matches!(write, Err(error) if error.code == api::ErrorCode::InvalidState) {
        return Err(api::Failure::new(
            api::ErrorCode::OperationFailed,
            "Language discovery unexpectedly accepted a write",
        ));
    }
    Ok(api::Output {
        language_service: Some(language::Proposal {
            initialization_options: Some(serde_json::json!({
                "sdk": sdk,
                "files": files,
                // Returning an opaque handle as data must not extend its read-only hook lifetime.
                "temporary_workspace": workspace,
            })),
            ..Default::default()
        }),
        ..Default::default()
    })
}
