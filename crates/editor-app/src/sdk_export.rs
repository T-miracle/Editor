//! Supply the editor's compiled-in contract to independent Cargo plugin projects.
//!
//! The contract documents are authored for readers under `website/`, which is the single
//! hand-written source: the same files are published on the documentation site and shipped
//! to plugin projects through this export. Moving that directory therefore breaks this
//! build on purpose.
//!
//! Established exported filenames stay stable. Shared project packaging documentation ships
//! alongside the public contract; website names remain lower-case for readable URLs.
use anyhow::Context;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{borrow::Cow, io::Write};

const SDK_FILES: &[(&str, &[u8])] = &[
    // The reader-facing document contract is exported verbatim with these Rust types.
    (
        "DOCUMENTS.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/documents.md"),
    ),
    // Document contracts ship in the exact same SDK used by the host consumer.
    (
        "src/api/documents.rs",
        include_bytes!("../../plugin-protocol/src/api/documents.rs"),
    ),
    // Author-facing contract pages ship with the identical independent guest interface.
    (
        "COMMANDS.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/commands.md"),
    ),
    (
        "INTERACTION.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/interaction.md"),
    ),
    // Typed commands and native menu declarations share the actual independent guest SDK.
    (
        "src/commands.rs",
        include_bytes!("../../plugin-protocol/src/commands.rs"),
    ),
    // Independent guests receive the identical native interaction types as the runtime.
    (
        "src/interaction.rs",
        include_bytes!("../../plugin-protocol/src/interaction.rs"),
    ),
    // The shared project format is shipped beside the public contract for independent authors.
    (
        "PACKAGING.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/packaging.md"),
    ),
    (
        "src/api/preference_binding.rs",
        include_bytes!("../../plugin-protocol/src/api/preference_binding.rs"),
    ),
    ("TOOLS.md", include_bytes!("../../plugin-protocol/TOOLS.md")),
    (
        "src/ui/layout_tests.rs",
        include_bytes!("../../plugin-protocol/src/ui/layout_tests.rs"),
    ),
    (
        "CONFIGURATIONS.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/configurations.md"),
    ),
    // Public configuration providers receive the identical template, native form and validation types.
    (
        "src/configurations.rs",
        include_bytes!("../../plugin-protocol/src/configurations.rs"),
    ),
    (
        "src/configurations/command_form.rs",
        include_bytes!("../../plugin-protocol/src/configurations/command_form.rs"),
    ),
    // Reader-facing English is the canonical documentation exported with the identical public SDK.
    (
        "DEBUG.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/debug.md"),
    ),
    (
        "TARGETS.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/targets.md"),
    ),
    // Independent target providers use the same exact schemas as the host consumer.
    (
        "src/targets.rs",
        include_bytes!("../../plugin-protocol/src/targets.rs"),
    ),
    (
        "src/run-targets.json",
        include_bytes!("../../plugin-protocol/src/run-targets.json"),
    ),
    // The canonical optional debug signatures are packaged with their Rust SDK accessor.
    (
        "src/debug.rs",
        include_bytes!("../../plugin-protocol/src/debug.rs"),
    ),
    (
        "src/debug-session.json",
        include_bytes!("../../plugin-protocol/src/debug-session.json"),
    ),
    // Independent providers receive the same bounded observation contract as the host.
    (
        "src/execution.rs",
        include_bytes!("../../plugin-protocol/src/execution.rs"),
    ),
    // Independent consumers receive the same session contract and migration rules as the host.
    (
        "SESSIONS.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/sessions.md"),
    ),
    (
        "VIEWPORT.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/viewport.md"),
    ),
    (
        "src/api/viewport.rs",
        include_bytes!("../../plugin-protocol/src/api/viewport.rs"),
    ),
    (
        "CODE_HIGHLIGHTING.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/code-highlighting.md"),
    ),
    (
        "NAVIGATION.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/navigation.md"),
    ),
    (
        "src/api/navigation.rs",
        include_bytes!("../../plugin-protocol/src/api/navigation.rs"),
    ),
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
        "src/ui/tools.rs",
        include_bytes!("../../plugin-protocol/src/ui/tools.rs"),
    ),
    (
        "src/ui/tools_tests.rs",
        include_bytes!("../../plugin-protocol/src/ui/tools_tests.rs"),
    ),
    (
        "src/ui/canvas.rs",
        include_bytes!("../../plugin-protocol/src/ui/canvas.rs"),
    ),
    (
        "src/ui/visual_viewport.rs",
        include_bytes!("../../plugin-protocol/src/ui/visual_viewport.rs"),
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
    // Pure structure providers build independently against the same versioned data types as the host.
    (
        "src/structure.rs",
        include_bytes!("../../plugin-protocol/src/structure.rs"),
    ),
    (
        "STRUCTURE.md",
        include_bytes!("../../../website/src/content/docs/en/sdk/structure.md"),
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
        "src/api/preferences.rs",
        include_bytes!("../../plugin-protocol/src/api/preferences.rs"),
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
    // Incremental native publication is part of the same independently exported public SDK.
    (
        "src/ui/incremental.rs",
        include_bytes!("../../plugin-protocol/src/ui/incremental.rs"),
    ),
    (
        "src/ui/tests.rs",
        include_bytes!("../../plugin-protocol/src/ui/tests.rs"),
    ),
    (
        "src/ui/images_tests.rs",
        include_bytes!("../../plugin-protocol/src/ui/images_tests.rs"),
    ),
    (
        "src/api/images_tests.rs",
        include_bytes!("../../plugin-protocol/src/api/images_tests.rs"),
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
    publish(&config, contents.as_bytes())?;
    Ok(config)
}

/// Derive offline Markdown from the sole reader source; SDK code and schemas stay byte-identical.
/// The known documentation routes become package-relative links, including any trailing anchor.
fn exported_bytes<'a>(relative: &str, bytes: &'a [u8]) -> Cow<'a, [u8]> {
    if !relative.ends_with(".md") {
        return Cow::Borrowed(bytes);
    }
    let mut document = std::str::from_utf8(bytes)
        .expect("embedded reader documentation is UTF-8")
        .to_owned();
    // Astro metadata is presentation-only and is omitted from the independent SDK's Markdown.
    if let Some(rest) = document
        .strip_prefix("---\r\n")
        .or_else(|| document.strip_prefix("---\n"))
        && let Some(end) = rest.find("\n---")
    {
        // Site metadata is removed on either checkout style; body bytes retain their line endings.
        document = rest[(end + 4)..]
            .trim_start_matches(['\r', '\n'])
            .to_owned();
    }
    for (route, file) in [
        // Capability pages must precede the SDK root fallback to retain local exported links.
        ("commands", "COMMANDS.md"),
        ("interaction", "INTERACTION.md"),
        ("packaging", "PACKAGING.md"),
        ("configurations", "CONFIGURATIONS.md"),
        ("debug", "DEBUG.md"),
        ("targets", "TARGETS.md"),
        ("sessions", "SESSIONS.md"),
        ("viewport", "VIEWPORT.md"),
        ("code-highlighting", "CODE_HIGHLIGHTING.md"),
        ("navigation", "NAVIGATION.md"),
        ("migration", "MIGRATION.md"),
        ("faults", "FAULTS.md"),
        ("services", "SERVICES.md"),
        ("dependencies", "DEPENDENCIES.md"),
        ("lsp", "LSP.md"),
        ("structure", "STRUCTURE.md"),
        ("ui", "UI.md"),
        ("processes", "PROCESSES.md"),
        ("languages", "LANGUAGES.md"),
        ("documents", "DOCUMENTS.md"),
        ("", "README.md"),
    ] {
        let route = if route.is_empty() {
            "/en/sdk/".to_owned()
        } else {
            format!("/en/sdk/{route}/")
        };
        document = document.replace(&format!("]({route}"), &format!("]({file}"));
    }
    // User guides are not part of the guest SDK source bundle; exported links still open the
    // published guide while capability references above resolve to local SDK Markdown.
    document = document.replace(
        "](/en/guide/",
        "](https://t-miracle.github.io/Editor/en/guide/",
    );
    Cow::Owned(document.into_bytes())
}

/// Content-addressed caches let different editor versions build plugins independently.
fn prepare_cache(root: &Path) -> anyhow::Result<PathBuf> {
    let mut digest = Sha256::new();
    for (name, raw) in SDK_FILES {
        let bytes = exported_bytes(name, raw);
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes.as_ref());
    }
    let target = root.join(format!("{:x}", digest.finalize()));
    export(&target)?;
    Ok(target)
}

/// Preserve unchanged files for Cargo freshness and atomically repair incomplete caches.
pub fn export(target: &Path) -> anyhow::Result<()> {
    for (relative, raw) in SDK_FILES {
        let bytes = exported_bytes(relative, raw);
        let path = target.join(relative);
        publish(&path, bytes.as_ref())?;
    }
    Ok(())
}

/// Concurrent first-use builds can prepare the identical content-addressed SDK. On Windows,
/// replacement may fail while another reader opens the winner; identical committed bytes are
/// already success. Other write failures still report the exact path and preserve existing data.
fn publish(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if std::fs::read(path).is_ok_and(|existing| existing == bytes) {
        return Ok(());
    }
    let parent = path.parent().context("SDK file needs a parent")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    if let Err(error) = file.persist(path) {
        if !std::fs::read(path).is_ok_and(|existing| existing == bytes) {
            return Err(error.error)
                .with_context(|| format!("Could not publish SDK file {}", path.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two independent host invocations can publish a fresh SDK concurrently without partial files.
    #[test]
    fn concurrent_sdk_exports_converge() {
        let root = tempfile::tempdir().unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let workers = (0..4)
            .map(|_| {
                let path = root.path().to_owned();
                let ready = barrier.clone();
                std::thread::spawn(move || {
                    ready.wait();
                    export(&path)
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        for (relative, bytes) in SDK_FILES {
            assert_eq!(
                std::fs::read(root.path().join(relative)).unwrap(),
                exported_bytes(relative, bytes).as_ref()
            );
        }
    }

    /// Both checkout styles remove Astro metadata and rewrite links while retaining body newlines.
    #[test]
    fn reader_export_handles_both_checkout_newlines() {
        let document =
            "---\ntitle: Protocol\n---\n\n# Public protocol\n[Services](/en/sdk/services/)\n";
        let crlf = document.replace('\n', "\r\n");
        assert_eq!(
            exported_bytes("README.md", document.as_bytes()).as_ref(),
            b"# Public protocol\n[Services](SERVICES.md)\n"
        );
        assert_eq!(
            exported_bytes("README.md", crlf.as_bytes()).as_ref(),
            b"# Public protocol\r\n[Services](SERVICES.md)\r\n"
        );
    }

    /// Exported Markdown must be navigable from the SDK directory without a running website.
    #[test]
    fn documents_sdk_exports_stable_contract_and_policy_link() {
        let root = tempfile::tempdir().unwrap();
        export(root.path()).unwrap();
        let readme = std::fs::read_to_string(root.path().join("README.md")).unwrap();
        let documents = std::fs::read_to_string(root.path().join("DOCUMENTS.md")).unwrap();
        assert!(
            readme.contains("](DOCUMENTS.md)"),
            "README must link to the embedded document contract"
        );
        assert_eq!(
            documents.lines().next(),
            Some("# Documents and readonly resources")
        );
        assert!(documents.contains("are stable core"));
        assert!(documents.contains("](README.md#stability-and-capability-compatibility)"));
        assert!(readme.contains("## Stability and capability compatibility"));
    }

    /// All independently exported pages resolve to the same immutable SDK bundle.
    #[test]
    fn exported_reader_documents_resolve_all_local_links() {
        let root = tempfile::tempdir().unwrap();
        export(root.path()).unwrap();
        for (relative, _) in SDK_FILES.iter().filter(|(name, _)| name.ends_with(".md")) {
            let document = std::fs::read_to_string(root.path().join(relative)).unwrap();
            assert!(
                !document.contains("](/en/"),
                "{relative} still contains a site-root link"
            );
            for link in document
                .split("](")
                .skip(1)
                .filter_map(|tail| tail.split(')').next())
            {
                if link.starts_with("https://")
                    || link.starts_with("http://")
                    || link.starts_with('#')
                {
                    continue;
                }
                let path = link.split('#').next().unwrap();
                assert!(
                    root.path().join(path).is_file(),
                    "{relative} points to missing {path}"
                );
            }
        }
    }

    #[test]
    fn cached_contract_is_complete_reused_and_repairs_corruption() {
        let root = tempfile::tempdir().unwrap();
        let sdk = prepare_cache(root.path()).unwrap();
        for (relative, bytes) in SDK_FILES {
            assert_eq!(
                std::fs::read(sdk.join(relative)).unwrap(),
                exported_bytes(relative, bytes).as_ref()
            );
        }
        let manifest = sdk.join("Cargo.toml");
        let modified = manifest.metadata().unwrap().modified().unwrap();
        assert_eq!(prepare_cache(root.path()).unwrap(), sdk);
        assert_eq!(manifest.metadata().unwrap().modified().unwrap(), modified);
        std::fs::write(&manifest, b"interrupted or modified cache").unwrap();
        std::fs::remove_file(sdk.join("wit/plugin.wit")).unwrap();
        prepare_cache(root.path()).unwrap();
        for (relative, bytes) in SDK_FILES {
            assert_eq!(
                std::fs::read(sdk.join(relative)).unwrap(),
                exported_bytes(relative, bytes).as_ref()
            );
        }
    }

    #[test]
    fn exported_documents_carry_no_site_frontmatter() {
        for (relative, bytes) in SDK_FILES {
            if !relative.ends_with(".md") {
                continue;
            }
            let exported = String::from_utf8(exported_bytes(relative, bytes).into_owned()).unwrap();
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
        let with_frontmatter =
            b"---\ntitle: Example\nalternate: /en/x/\n---\n\n# Heading\n\nBody.\n";
        assert_eq!(
            String::from_utf8(exported_bytes("DOC.md", with_frontmatter).into_owned()).unwrap(),
            "# Heading\n\nBody.\n"
        );
        // A Windows checkout must export the same heading, preserving the body's own CRLF.
        let windows_frontmatter = b"---\r\ntitle: Example\r\n---\r\n\r\n# Heading\r\n\r\nBody.\r\n";
        assert_eq!(
            exported_bytes("DOC.md", windows_frontmatter).as_ref(),
            b"# Heading\r\n\r\nBody.\r\n"
        );
        let without_frontmatter = b"# Heading\n\nBody.\n";
        assert_eq!(
            exported_bytes("DOC.md", without_frontmatter).as_ref(),
            without_frontmatter
        );
        let horizontal_rule = b"# Heading\n\n---\n\nBody.\n";
        assert_eq!(
            exported_bytes("DOC.md", horizontal_rule).as_ref(),
            horizontal_rule
        );
    }
}
