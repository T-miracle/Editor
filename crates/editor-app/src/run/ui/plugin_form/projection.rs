//! Existing generic launch grammar is a checked cache, never an execution authorization.
use super::*;
/// Provider launch data is converted through the existing generic target grammar, never Shell parsing.
pub(super) fn projection(
    data: &PluginConfiguration,
    id: &str,
    launch: Option<contract::Launch>,
) -> Result<RunConfig, String> {
    let mut launch = launch.unwrap_or(contract::Launch {
        target: serde_json::json!({"mode":"program","program":data.program,"args":[]}),
        directory: None,
        env: Default::default(),
        tool_paths: vec![],
        build: vec![],
        prelaunch: vec![],
        provider: None,
    });
    // The public self-reference binds a provider-owned target to its authenticated contributor.
    // It never selects an execution provider or grants that contributor additional permissions.
    bind_self_target(&mut launch.target, &data.provider);
    for step in launch.build.iter_mut().chain(launch.prelaunch.iter_mut()) {
        if step["target"]["kind"] == "action" {
            bind_self_target(&mut step["target"]["target"], &data.provider);
        }
    }
    let config: RunConfig = serde_json::from_value(serde_json::json!({
        "id":id,"name":if data.name.trim().is_empty(){id}else{&data.name},"target":launch.target,
        "directory":launch.directory,"env":launch.env,"tool_paths":launch.tool_paths,
        "build":launch.build,"prelaunch":launch.prelaunch,"provider":launch.provider,"local":true
    }))
    .map_err(|error| error.to_string())?;
    config.validate().map_err(|error| error.to_string())?;
    Ok(config)
}

/// Replace only the public provided-target provider token, never opaque bindings or other fields.
fn bind_self_target(target: &mut serde_json::Value, provider: &str) {
    if target["mode"] == "provided" && target["provider"] == "$self" {
        target["provider"] = provider.into();
    }
}

/// A malformed success is unavailable, not valid; old prepared data remains non-authoritative.
pub(super) fn validated(
    snapshot: PluginConfiguration,
    id: &str,
    reply: Result<serde_json::Value, String>,
) -> (PluginConfiguration, Option<RunConfig>) {
    let mut data = snapshot;
    match reply.and_then(contract::decode::<contract::Validation>) {
        Ok(validation) if validation.valid => match validation
            .launch
            .ok_or("Missing launch data".to_owned())
            .and_then(|launch| projection(&data, id, Some(launch)))
        {
            Ok(configuration) => {
                data.validation = ConfigurationValidation::Valid;
                return (data, Some(configuration));
            }
            Err(error) => data.validation = ConfigurationValidation::Unavailable(error),
        },
        Ok(validation) => data.validation = ConfigurationValidation::Invalid(validation.message),
        Err(error) => data.validation = ConfigurationValidation::Unavailable(error),
    }
    (data, None)
}
