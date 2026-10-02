//! Resolve native tools once to absolute files; project-relative search is never implicit authority.
use std::path::{Path, PathBuf};

/// Bare tools search only absolute PATH entries; extensions are executable formats, never scripts.
pub(crate) fn resolve(program: &str) -> anyhow::Result<PathBuf> {
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
    for candidate in candidates {
        if candidate.is_file() {
            let path = candidate.canonicalize()?;
            #[cfg(windows)]
            let path = executable_spelling(path)?;
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

/// .NET Framework programs reject device-prefixed argv[0]; both pipes and PTYs use the same spelling.
#[cfg(windows)]
fn executable_spelling(canonical: PathBuf) -> anyhow::Result<PathBuf> {
    let program = canonical
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Executable path is not Unicode"))?;
    let ordinary = if let Some(path) = program.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{path}")
    } else {
        program.strip_prefix(r"\\?\").unwrap_or(program).to_owned()
    };
    let ordinary = PathBuf::from(ordinary);
    anyhow::ensure!(
        ordinary.canonicalize()? == canonical,
        "Executable path spelling changed its target"
    );
    Ok(ordinary)
}
