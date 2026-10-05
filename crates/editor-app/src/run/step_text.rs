//! Lossless editable step rows: literal values use JSON string quoting only when separators need it.
//! Ordinary unquoted historical rows remain readable; Shell rows are explicit, never inferred.
use super::RunTarget;
use editor_core::{RunStep, StepTarget};

/// Split separators only outside quoted values; a quoted script may contain newlines or pipes.
fn parts(text: &str, delimiter: char, limit: usize) -> Result<Vec<&str>, String> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
            continue;
        }
        if character == delimiter && !quoted && result.len() < limit {
            result.push(&text[start..index]);
            start = index + character.len_utf8();
        }
    }
    if quoted {
        return Err(rust_i18n::t!("run.step_unclosed_quote").into());
    }
    result.push(&text[start..]);
    Ok(result)
}
/// A quoted scalar keeps empty strings and whitespace; old unquoted scalars keep their old trim rule.
fn scalar(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.starts_with('"') {
        serde_json::from_str(text).map_err(|_| rust_i18n::t!("run.step_invalid_quote").to_string())
    } else {
        Ok(text.into())
    }
}
/// Names and argv are never composed into a command line. Explicit !shell carries one final script.
pub fn parse_steps(text: &str) -> Result<Vec<RunStep>, String> {
    let mut result = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let pair = parts(line, '=', 1)?;
        if pair.len() != 2 {
            return Err(rust_i18n::t!("run.step_syntax", line = line).into());
        }
        let name = scalar(pair[0])?;
        if name.trim().is_empty() {
            return Err(rust_i18n::t!("run.step_name_missing", line = line).into());
        }
        let rest = pair[1].trim();
        let shell = rest.starts_with("!shell ");
        let rest = if shell {
            rest.trim_start_matches("!shell ")
        } else {
            rest
        };
        let fields = parts(rest, '|', usize::MAX)?;
        let first = fields[0].trim();
        let executable = scalar(first)?;
        if executable.is_empty() {
            return Err(rust_i18n::t!("run.step_program_missing", line = line).into());
        }
        let mut arguments = fields[1..]
            .iter()
            .map(|part| scalar(part))
            .collect::<Result<Vec<_>, _>>()?;
        let target = if let Some(reference) = first.strip_prefix('@') {
            if shell || !arguments.is_empty() {
                return Err(rust_i18n::t!("run.step_reference_arguments").into());
            }
            let config = scalar(reference)?;
            if config.is_empty() {
                return Err(rust_i18n::t!("run.step_reference_missing", line = line).into());
            }
            StepTarget::Build { config }
        } else if shell {
            let script = arguments
                .pop()
                .ok_or_else(|| rust_i18n::t!("run.step_script_missing").to_string())?;
            StepTarget::Action {
                target: RunTarget::Script {
                    interpreter: executable,
                    args: arguments,
                    script,
                },
            }
        } else {
            StepTarget::Action {
                target: RunTarget::Program {
                    program: executable,
                    args: arguments,
                },
            }
        };
        result.push(RunStep { name, target });
    }
    Ok(result)
}
/// Quote only values that cannot safely remain one ordinary row field.
fn quoted(value: &str) -> String {
    if value.is_empty()
        || value.trim() != value
        || value.contains(['|', '=', '"', '\\', '\n', '\r'])
        || value.starts_with(['@', '!', '#'])
    {
        serde_json::to_string(value).expect("a Rust string is serializable")
    } else {
        value.into()
    }
}
/// Rendering retains argv boundaries, empty arguments, script mode and script text on a single row.
pub fn render_steps(steps: &[RunStep]) -> String {
    steps
        .iter()
        .map(|step| {
            let mut line = format!("{} = ", quoted(&step.name));
            match &step.target {
                StepTarget::Build { config } => {
                    line.push('@');
                    line.push_str(&quoted(config));
                }
                StepTarget::Action { target } => {
                    if matches!(target, RunTarget::Script { .. }) {
                        line.push_str("!shell ");
                    }
                    line.push_str(&quoted(target.executable()));
                    for argument in target.arguments() {
                        line.push_str(" | ");
                        line.push_str(&quoted(&argument));
                    }
                }
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}
