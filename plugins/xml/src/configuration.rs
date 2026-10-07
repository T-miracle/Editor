//! Build LemMinX settings from explicit plugin values and host-owned native resource locations.
use plugin_protocol::{
    api::{ErrorCode, Failure},
    language,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Association {
    pattern: String,
    #[serde(rename = "systemId")]
    system_id: String,
}

/// Native services receive native paths only; the guest never gains a filesystem preopen.
pub(super) fn prepare(context: language::Context) -> Result<language::Proposal, Failure> {
    let setting = |key: &str| context.settings.get(key).map(|value| &value.value);
    let download = setting("download_schemas")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut associations: Vec<Association> =
        parse_list(setting("schema_associations"), "schema_associations")?;
    if associations.len() > 64 {
        return Err(invalid(
            "schema_associations",
            "at most 64 associations are allowed",
        ));
    }
    let mut resolved = Vec::new();
    for association in associations.drain(..) {
        if association.pattern.is_empty()
            || association.pattern.len() > 256
            || association.pattern.chars().any(char::is_control)
        {
            return Err(invalid(
                "schema_associations",
                "pattern must be a nonempty file glob",
            ));
        }
        let system = resolve_uri(&context.workspace, &association.system_id)?;
        if let Some(pattern) = snapshot_pattern(&association.pattern) {
            // Preserve absolute glob boundaries while supporting the host's immutable wire URI identity.
            resolved.push(json!({"pattern":server_pattern(&pattern),"systemId":system}));
        }
        resolved.push(json!({"pattern":server_pattern(&association.pattern),"systemId":system}));
    }
    // Explicit associations precede the bundled SVG assistance so users can replace its rules.
    if setting("svg_suggestions")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        resolved.push(json!({"pattern":"**/*.svg","systemId":resolve_uri(&context.package_root, "schemas/svg.xsd")?}));
    }
    let catalogs: Vec<String> = parse_list(setting("catalogs"), "catalogs")?;
    if catalogs.len() > 32 {
        return Err(invalid("catalogs", "at most 32 catalog paths are allowed"));
    }
    let catalogs = catalogs
        .iter()
        .map(|path| resolve_uri(&context.workspace, path))
        .collect::<Result<Vec<_>, _>>()?;
    let xml = json!({
        "validation":{"enabled":true,"noGrammar":"ignore"},
        "downloadExternalResources":{"enabled":download},
        "fileAssociations":resolved,"catalogs":catalogs,
        // Caches belong to this instance's private directory, never the project or global home.
        "server":{"workDir":format!("{}/schema-cache", native_path(&context.data_root).trim_end_matches('/'))},
        "completion":{"autoCloseTags":true},
        "telemetry":{"enabled":false}
    });
    let options = json!({"settings":{"xml":xml}});
    let mut sections = BTreeMap::from([(String::new(), json!({"xml":xml}))]);
    publish_sections("xml", &xml, &mut sections);
    Ok(language::Proposal {
        initialization_options: Some(options),
        configuration: Some(sections),
        ..Default::default()
    })
}

/// Snapshot URI paths retain exactly 64 redundant dot segments after the volume root.
/// Relative/basename globs are unaffected; absolute patterns get an equivalent companion, not a broader glob.
fn snapshot_pattern(pattern: &str) -> Option<String> {
    let pattern = pattern.replace('\\', "/");
    let prefix =
        if pattern.as_bytes().get(1) == Some(&b':') && pattern.as_bytes().get(2) == Some(&b'/') {
            3
        } else if pattern.starts_with("//") {
            let server = pattern[2..].find('/')? + 2;
            pattern[server + 1..].find('/')? + server + 2
        } else if pattern.starts_with('/') {
            1
        } else {
            return None;
        };
    Some(format!(
        "{}{}{}",
        &pattern[..prefix],
        "./".repeat(language::SNAPSHOT_URI_SEGMENTS),
        &pattern[prefix..]
    ))
}

/// LemMinX prepends `**/` to patterns beginning with a literal drive letter, which cannot match its root.
/// A leading single-segment `*` bypasses that rewrite while retaining the exact native drive and path.
/// Windows local paths have one drive letter before `:`; the added star cannot cross a directory separator.
fn server_pattern(pattern: &str) -> String {
    let pattern = pattern.replace('\\', "/");
    if pattern
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        && pattern.as_bytes().get(1) == Some(&b':')
        && pattern.as_bytes().get(2) == Some(&b'/')
    {
        format!("*{pattern}")
    } else {
        pattern
    }
}

/// Parse user-facing JSON arrays with their setting key in errors; never silently discard bad configuration.
fn parse_list<T: serde::de::DeserializeOwned>(
    value: Option<&Value>,
    key: &str,
) -> Result<Vec<T>, Failure> {
    let text = value.and_then(Value::as_str).unwrap_or("[]");
    serde_json::from_str(text).map_err(|error| invalid(key, &error.to_string()))
}

/// Resolve workspace-relative schema paths as file URIs; remote references remain server-controlled.
fn resolve_uri(root: &str, value: &str) -> Result<String, Failure> {
    if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(invalid(
            "schema path",
            "path must be nonempty and contain no control characters",
        ));
    }
    let value = native_path(value);
    if value.starts_with("https://") || value.starts_with("http://") || value.starts_with("file://")
    {
        return url::Url::parse(&value)
            .map(|url| url.to_string())
            .map_err(|error| invalid("schema path", &error.to_string()));
    }
    let absolute = value.starts_with('/') || value.as_bytes().get(1) == Some(&b':');
    let path = if absolute {
        value
    } else {
        format!("{}/{}", native_path(root).trim_end_matches('/'), value)
    };
    // WASI does not recognize Windows absolute Path values. URL path segments encode literal # and %,
    // including characters in a workspace/package root, rather than interpreting them as URL syntax.
    let mut uri = url::Url::parse("file:///").expect("static file URL is valid");
    let local = if let Some(unc) = path.strip_prefix("//") {
        let (server, local) = unc
            .split_once('/')
            .ok_or_else(|| invalid("schema path", "UNC path has no share"))?;
        uri.set_host(Some(server))
            .map_err(|_| invalid("schema path", "invalid UNC server"))?;
        local
    } else {
        path.trim_start_matches('/')
    };
    if local.as_bytes().get(1) == Some(&b':')
        && (!local.as_bytes()[0].is_ascii_alphabetic() || local.as_bytes().get(2) != Some(&b'/'))
    {
        return Err(invalid("schema path", "drive paths must be absolute"));
    }
    // URL segment insertion deliberately skips . and .. instead of resolving them. Fold native
    // parent segments first, without allowing traversal above a drive, UNC share or Unix root.
    let minimum = usize::from(path.starts_with("//") || local.as_bytes().get(1) == Some(&b':'));
    let mut segments = Vec::new();
    for segment in local.split('/').filter(|segment| !segment.is_empty()) {
        match segment {
            "." => {}
            ".." if segments.len() > minimum => {
                segments.pop();
            }
            ".." => {
                return Err(invalid(
                    "schema path",
                    "parent path escapes the native root",
                ));
            }
            segment => segments.push(segment),
        }
    }
    if segments.len() < minimum {
        return Err(invalid(
            "schema path",
            "native path has no drive or UNC share",
        ));
    }
    uri.path_segments_mut()
        .map_err(|_| invalid("schema path", "file URL has no path"))?
        .clear()
        .extend(segments);
    Ok(uri.to_string())
}

/// Convert Windows canonical extended paths into spelling understood by the native language server.
fn native_path(value: &str) -> String {
    let value = value.replace('\\', "/");
    if let Some(path) = value.strip_prefix("//?/UNC/") {
        format!("//{path}")
    } else {
        value.strip_prefix("//?/").unwrap_or(&value).to_owned()
    }
}

/// Both whole and dotted settings sections share the exact effective plugin configuration.
fn publish_sections(prefix: &str, value: &Value, sections: &mut BTreeMap<String, Value>) {
    sections.insert(prefix.into(), value.clone());
    if let Some(fields) = value.as_object() {
        for (name, child) in fields {
            publish_sections(&format!("{prefix}.{name}"), child, sections);
        }
    }
}

/// Configuration failures stay explicit so the host preserves highlighters and reports service state.
fn invalid(key: &str, reason: &str) -> Failure {
    Failure::new(ErrorCode::InvalidRequest, format!("XML {key}: {reason}"))
}
