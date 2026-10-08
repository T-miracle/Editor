//! Resolve native tools once to absolute files; project-relative search is never implicit authority.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Package declarations control discovery; the host only enforces paths, budgets and process ownership.
pub(crate) fn resolve_service(
    service: &plugin_protocol::process::Service,
) -> anyhow::Result<PathBuf> {
    resolve_service_until(service, Instant::now() + Duration::from_secs(5))
}

/// Synchronous guest calls share their existing deadline instead of receiving a fresh discovery budget.
pub(crate) fn resolve_service_until(
    service: &plugin_protocol::process::Service,
    deadline: Instant,
) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(service.valid(), "Invalid service declaration");
    let deadline = deadline.min(Instant::now() + Duration::from_secs(5));
    let mut visited = 0;
    let mut candidates = Vec::new();
    if !Path::new(&service.program).is_absolute() {
        for pattern in &service.search_paths {
            let expanded = if let Some(tail) = pattern.strip_prefix("${HOME}/") {
                dirs::home_dir()
                    .ok_or_else(|| anyhow::anyhow!("Home directory is unavailable"))?
                    .join(tail)
                    .to_string_lossy()
                    .replace('\\', "/")
            } else {
                // Preserve Windows verbatim prefixes; their '?' is path syntax, not a glob.
                pattern.clone()
            };
            let mut matches = expand_pattern(&expanded, deadline, &mut visited)?;
            anyhow::ensure!(
                matches.len() + candidates.len() <= 256,
                "Tool search quota exceeded"
            );
            // A distribution may install several versions; prefer the most recent path spelling.
            matches.sort_by(|a, b| b.cmp(a));
            candidates.extend(matches);
        }
    }
    candidates.extend(program_candidates(&service.program)?);
    for candidate in candidates {
        anyhow::ensure!(Instant::now() < deadline, "Tool discovery timed out");
        let Ok(path) = resolve(&candidate.to_string_lossy()) else {
            continue;
        };
        if service.check_args.is_empty()
            || crate::process::probe(
                &path,
                &service.check_args,
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(2)),
            )
            .unwrap_or(false)
        {
            return Ok(path);
        }
    }
    anyhow::bail!("No usable native tool found: {}", service.program)
}

/// Expand one path component at a time so zero-result globs still consume traversal and memory quotas.
fn expand_pattern(
    pattern: &str,
    deadline: Instant,
    visited: &mut usize,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut candidates = vec![PathBuf::new()];
    for component in Path::new(pattern).components() {
        anyhow::ensure!(Instant::now() < deadline, "Tool discovery timed out");
        let text = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Tool path is not Unicode"))?;
        // Drive/UNC/verbatim prefixes and root separators are literal path components.
        if !matches!(component, std::path::Component::Normal(_)) || !text.contains(['*', '?', '['])
        {
            for candidate in &mut candidates {
                candidate.push(component);
            }
            continue;
        }
        let matcher = glob::Pattern::new(text)?;
        let mut next = Vec::new();
        for directory in candidates {
            let Ok(entries) = std::fs::read_dir(directory) else {
                continue;
            };
            for entry in entries {
                anyhow::ensure!(Instant::now() < deadline, "Tool discovery timed out");
                *visited += 1;
                anyhow::ensure!(*visited <= 20_000, "Tool search traversal quota exceeded");
                let Ok(entry) = entry else { continue };
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| matcher.matches(name))
                {
                    next.push(entry.path());
                    anyhow::ensure!(next.len() <= 256, "Tool search expansion quota exceeded");
                }
            }
        }
        candidates = next;
    }
    Ok(candidates)
}

/// Bare tools search only absolute PATH entries; extensions are executable formats, never scripts.
pub(crate) fn resolve(program: &str) -> anyhow::Result<PathBuf> {
    for candidate in program_candidates(program)? {
        if candidate.is_file() {
            let path = candidate.canonicalize()?;
            #[cfg(windows)]
            let path = child_path_spelling(path)?;
            #[cfg(windows)]
            anyhow::ensure!(
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe")),
                "Only native executable tools are supported"
            );
            return Ok(path);
        }
    }
    anyhow::bail!("Native tool not found: {program}")
}

/// Explicit absolute paths have one candidate; bare names never consult project-relative PATH entries.
fn program_candidates(program: &str) -> anyhow::Result<Vec<PathBuf>> {
    let requested = Path::new(program);
    let candidates = if requested.is_absolute() {
        vec![requested.to_path_buf()]
    } else {
        anyhow::ensure!(
            !program.contains(['/', '\\', ':']),
            "Tool path must be absolute or a bare name"
        );
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|path| path.is_absolute())
            .flat_map(|path| {
                let candidate = path.join(program);
                #[cfg(windows)]
                if candidate.extension().is_none() {
                    return vec![candidate.with_extension("exe")];
                }
                vec![candidate]
            })
            .collect()
    };
    Ok(candidates)
}

/// Native interpreters and module loaders can reject device-prefixed argv paths. Return an ordinary
/// Windows spelling only after proving that it resolves to the same previously validated target.
#[cfg(windows)]
pub(crate) fn child_path_spelling(canonical: PathBuf) -> anyhow::Result<PathBuf> {
    let program = canonical
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Native path is not Unicode"))?;
    let ordinary = if let Some(path) = program.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{path}")
    } else {
        program.strip_prefix(r"\\?\").unwrap_or(program).to_owned()
    };
    let ordinary = PathBuf::from(ordinary);
    anyhow::ensure!(
        ordinary.canonicalize()? == canonical,
        "Native path spelling changed its target"
    );
    Ok(ordinary)
}
