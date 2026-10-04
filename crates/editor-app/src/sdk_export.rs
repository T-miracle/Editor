//! Supply the editor's compiled-in contract to independent Cargo plugin projects.
//!
//! The contract documents are authored for readers under `website/`, which is the single
//! hand-written source: the same files are published on the documentation site and shipped
//! to plugin projects through this export. Moving that directory therefore breaks this
//! build on purpose.
//!
//! The exported names stay as they were before the move, so plugin projects and the
//! distribution checks see an unchanged file set; only the sources changed location and
//! became lower-case for readable URLs.
use anyhow::Context;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const SDK_FILES: &[(&str, &[u8])] = &[
    (
        "MIGRATION.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/migration.md"),
    ),
    (
        "FAULTS.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/faults.md"),
    ),
    (
        "SERVICES.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/services.md"),
    ),
    (
        "src/service.rs",
        include_bytes!("../../plugin-protocol/src/service.rs"),
    ),
    (
        "src/ui/events.rs",
        include_bytes!("../../plugin-protocol/src/ui/events.rs"),
    ),
    (
        "src/ui/canvas.rs",
        include_bytes!("../../plugin-protocol/src/ui/canvas.rs"),
    ),
    (
        "DEPENDENCIES.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/dependencies.md"),
    ),
    (
        "src/dependencies.rs",
        include_bytes!("../../plugin-protocol/src/dependencies.rs"),
    ),
    (
        "LSP.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/lsp.md"),
    ),
    (
        "src/language.rs",
        include_bytes!("../../plugin-protocol/src/language.rs"),
    ),
    (
        "src/process.rs",
        include_bytes!("../../plugin-protocol/src/process.rs"),
    ),
    (
        "src/settings.rs",
        include_bytes!("../../plugin-protocol/src/settings.rs"),
    ),
    (
        "src/api.rs",
        include_bytes!("../../plugin-protocol/src/api.rs"),
    ),
    (
        "src/api/guest.rs",
        include_bytes!("../../plugin-protocol/src/api/guest.rs"),
    ),
    (
        "src/ui/controls.rs",
        include_bytes!("../../plugin-protocol/src/ui/controls.rs"),
    ),
    (
        "Cargo.toml",
        include_bytes!("../../plugin-protocol/Cargo.toml"),
    ),
    (
        "README.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/index.md"),
    ),
    (
        "UI.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/ui.md"),
    ),
    (
        "PROCESSES.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/processes.md"),
    ),
    (
        "LANGUAGES.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/languages.md"),
    ),
    (
        "src/lib.rs",
        include_bytes!("../../plugin-protocol/src/lib.rs"),
    ),
    (
        "src/ui.rs",
        include_bytes!("../../plugin-protocol/src/ui.rs"),
    ),
    (
        "src/ui/validate.rs",
        include_bytes!("../../plugin-protocol/src/ui/validate.rs"),
    ),
    (
        "src/ui/tests.rs",
        include_bytes!("../../plugin-protocol/src/ui/tests.rs"),
    ),
    (
        "wit/plugin.wit",
        include_bytes!("../../plugin-protocol/wit/plugin.wit"),
    ),
];

/// Handle development commands before starting the GUI. Cargo owns build/test options.
pub fn run_cli() -> anyhow::Result<bool> {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref().and_then(|arg| arg.to_str()) {
        Some("--plugin-cargo") => {
            let manifest = args
                .next()
                .context("usage: --plugin-cargo <Cargo.toml> <build|check|test> [Cargo options]")?;
            let operation = args
                .next()
                .context("missing Cargo operation: build, check or test")?;
            anyhow::ensure!(
                matches!(operation.to_str(), Some("build" | "check" | "test")),
                "supported plugin Cargo operations: build, check, test"
            );
            let manifest = PathBuf::from(manifest)
                .canonicalize()
                .context("plugin Cargo.toml does not exist")?;
            anyhow::ensure!(manifest.is_file(), "plugin manifest must be a file");
            let status = Command::new("cargo")
                .args(cargo_args()?)
                .arg(operation)
                .arg("--manifest-path")
                .arg(manifest)
                .args(args)
                .status()
                .context("could not start Cargo; install the Rust toolchain")?;
            anyhow::ensure!(status.success(), "plugin Cargo command failed: {status}");
        }
        Some("--export-plugin-sdk") => {
            // Keep explicit exports available for other language toolchains and inspection.
            let target = args
                .next()
                .context("--export-plugin-sdk requires an output directory")?;
            anyhow::ensure!(args.next().is_none(), "unexpected SDK export argument");
            export(Path::new(&target))?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// Build and language analysis resolve the exact same host-owned, versioned SDK.
pub(crate) fn cargo_args() -> anyhow::Result<Vec<String>> {
    Ok(vec![
        "--config".into(),
        cargo_config()?.to_string_lossy().into_owned(),
    ])
}

/// Describe the same immutable public contract used by Cargo, without granting guests filesystem handles.
pub(crate) fn descriptor() -> anyhow::Result<plugin_runtime::plugin_protocol::api::SdkDescriptor> {
    let config = cargo_config()?;
    let root = config
        .parent()
        .context("SDK configuration has no directory")?;
    let digest = root
        .file_name()
        .and_then(|name| name.to_str())
        .context("SDK cache identity is not Unicode")?;
    Ok(plugin_runtime::plugin_protocol::api::SdkDescriptor {
        digest: digest.to_owned(),
        root: root.display().to_string(),
        cargo_config: config.display().to_string(),
    })
}

/// Keep Cargo configuration beside the host cache, shared by metadata, checks and builds.
pub(crate) fn cargo_config() -> anyhow::Result<PathBuf> {
    let cache_root = dirs::cache_dir()
        .context("no user cache directory available")?
        .join("MeEditor")
        .join("plugin-sdk");
    let sdk = prepare_cache(&cache_root)?;
    // JSON string quoting also escapes Windows paths in TOML configuration values.
    let contents = format!(
        "# Host-managed plugin protocol dependency.\n[patch.crates-io]\nplugin-protocol = {{ path = {} }}\n",
        serde_json::to_string(&sdk)?
    );
    let config = sdk.join("host-cargo.toml");
    if !std::fs::read(&config).is_ok_and(|existing| existing == contents.as_bytes()) {
        let mut file = tempfile::NamedTempFile::new_in(&sdk)?;
        file.write_all(contents.as_bytes())?;
        file.persist(&config)?;
    }
    Ok(config)
}

/// Content-addressed caches let different editor versions build plugins independently.
fn prepare_cache(root: &Path) -> anyhow::Result<PathBuf> {
    let mut digest = Sha256::new();
    for (name, bytes) in SDK_FILES {
        let payload = payload(name, bytes);
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update((payload.len() as u64).to_le_bytes());
        digest.update(&payload);
    }
    let target = root.join(format!("{:x}", digest.finalize()));
    export(&target)?;
    Ok(target)
}

/// Return the bytes an export writes for one SDK file.
///
/// Contract documents are authored as site pages, so they carry YAML frontmatter
/// that the site needs and a plugin project does not: the exported document must
/// start with its heading. Stripping it here keeps one hand-written source for
/// both readers instead of maintaining a second copy for the SDK.
fn payload(name: &str, bytes: &'static [u8]) -> Vec<u8> {
    if !name.ends_with(".md") {
        return bytes.to_vec();
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };
    let Some(rest) = text.strip_prefix("---\n") else {
        return bytes.to_vec();
    };
    match rest.find("\n---") {
        Some(end) => rest[end + 4..].trim_start_matches('\n').as_bytes().to_vec(),
        // An unterminated block is not frontmatter; export the text unchanged.
        None => bytes.to_vec(),
    }
}

/// Preserve unchanged files for Cargo freshness and atomically repair incomplete caches.
pub fn export(target: &Path) -> anyhow::Result<()> {
    for (relative, bytes) in SDK_FILES {
        let payload = payload(relative, bytes);
        let path = target.join(relative);
        if std::fs::read(&path).is_ok_and(|existing| existing == payload) {
            continue;
        }
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&payload)?;
        file.persist(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_contract_is_complete_reused_and_repairs_corruption() {
        let root = tempfile::tempdir().unwrap();
        let sdk = prepare_cache(root.path()).unwrap();
        for (relative, bytes) in SDK_FILES {
            assert_eq!(std::fs::read(sdk.join(relative)).unwrap(), payload(relative, bytes));
        }
        let manifest = sdk.join("Cargo.toml");
        let modified = manifest.metadata().unwrap().modified().unwrap();
        assert_eq!(prepare_cache(root.path()).unwrap(), sdk);
        assert_eq!(manifest.metadata().unwrap().modified().unwrap(), modified);
        std::fs::write(&manifest, b"interrupted or modified cache").unwrap();
        std::fs::remove_file(sdk.join("wit/plugin.wit")).unwrap();
        prepare_cache(root.path()).unwrap();
        for (relative, bytes) in SDK_FILES {
            assert_eq!(std::fs::read(sdk.join(relative)).unwrap(), payload(relative, bytes));
        }
    }

    #[test]
    fn exported_documents_carry_no_site_frontmatter() {
        for (relative, bytes) in SDK_FILES {
            if !relative.ends_with(".md") {
                continue;
            }
            let exported = String::from_utf8(payload(relative, bytes)).unwrap();
            assert!(
                exported.starts_with("# "),
                "{relative} must start with its heading"
            );
            assert!(
                !exported.starts_with("---"),
                "{relative} must not export the site frontmatter"
            );
        }
    }

    #[test]
    fn frontmatter_stripping_leaves_other_text_alone() {
        let with_frontmatter = b"---\ntitle: Example\nalternate: /en/x/\n---\n\n# Heading\n\nBody.\n";
        assert_eq!(
            String::from_utf8(payload("DOC.md", with_frontmatter)).unwrap(),
            "# Heading\n\nBody.\n"
        );
        let without_frontmatter = b"# Heading\n\nBody.\n";
        assert_eq!(payload("DOC.md", without_frontmatter), without_frontmatter);
        let horizontal_rule = b"# Heading\n\n---\n\nBody.\n";
        assert_eq!(payload("DOC.md", horizontal_rule), horizontal_rule);
    }
}
