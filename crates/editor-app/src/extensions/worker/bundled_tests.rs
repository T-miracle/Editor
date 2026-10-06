//! Ordinary resource ZIPs exercise first-use helpers and the production actor without a language whitelist.
use super::*;
use crate::extensions::{
    bundled::{Request, prepare_offer, validate_candidate},
    protocol,
};
use std::io::{Cursor, Write};
use std::sync::atomic::{AtomicBool, Ordering};

mod native_failure;

/// A real independent resource package contributes one arbitrary recognition selector and no executable guest.
fn archive(id: &str, version: &str, extension: &str) -> Vec<u8> {
    let manifest = serde_json::json!({
        "id":id,"name":"Generic prose provider","version":version,"protocol":7,
        "api":{"base":"^1"},"contributions":"plugin.toml","storage_limit":1024
    });
    let contribution = format!(
        "[plugin]\nid='{id}'\nname='Generic prose provider'\nversion='{version}'\nhost_version='^0.1'\n\n[[language_definitions]]\nid='prose'\nname='Prose'\nextensions=['{extension}']\n"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("plugin.toml", contribution.into_bytes()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Fixture roots are canonical host paths; only cfg(test) admits the isolated catalog beneath this workspace.
struct Fixture {
    _directory: tempfile::TempDir,
    workspace: PathBuf,
    root: PathBuf,
    file: PathBuf,
    bytes: Vec<u8>,
}

impl Fixture {
    /// Keep the installed profile separate from project files, matching the production manager's ownership.
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let root = directory.path().join("private-runtime");
        let file = workspace.join("notes.prose");
        std::fs::write(&file, "unmodified source").unwrap();
        let bytes = archive("bundled-prose", "1.0.0", "prose");
        let fixture = Self {
            _directory: directory,
            workspace,
            root,
            file,
            bytes,
        };
        fixture.catalog("prose", "generic.zip", None);
        fixture
    }

    /// Explicit overrides create corrupt catalogs through the same filesystem boundary production reads.
    fn catalog(&self, extension: &str, filename: &str, digest: Option<&str>) {
        let directory = self.workspace.join(".bundled-plugin-test");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("generic.zip"), &self.bytes).unwrap();
        let package = Package::from_bytes(&self.bytes).unwrap();
        let value = serde_json::json!({"version":1,"packages":[{
            "file":filename,"sha256":digest.unwrap_or(&package.digest),"file_extensions":[extension]
        }]});
        std::fs::write(
            directory.join("bundle-defaults.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }

    /// Public manager restoration is the only readiness authority in these fixtures.
    fn manager(&self) -> Manager {
        Manager::open(self.root.clone(), self.environment()).unwrap()
    }

    /// Requests carry host paths and a revocable token, with no guest-supplied package identity.
    fn request(&self, token: u64) -> Request {
        Request::new(
            token,
            self.workspace.clone(),
            self.root.clone(),
            self.file.clone(),
        )
    }

    /// The actor and helper use the same workspace environment as ordinary public installations.
    fn environment(&self) -> protocol::Environment {
        protocol::Environment {
            workspace: self.workspace.display().to_string(),
            ..Default::default()
        }
    }
}

/// Inspection interrupted before display remains retryable, while an explicit refusal still blocks future hashes.
#[test]
fn late_inspection_withdrawal_can_offer_again_but_explicit_cancel_is_permanent() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let request = fixture.request(1);
    let late = prepare_offer(&mut manager, &request).unwrap().unwrap();
    request.active.store(false, Ordering::Release);
    assert!(validate_candidate(&manager, &late).is_err());
    assert!(!manager.has_bundle_choice("bundled-prose").unwrap());
    assert!(manager.installed.is_empty() && manager.live.is_empty());
    drop(late);
    drop(manager);
    let mut reopened = fixture.manager();
    let current = prepare_offer(&mut reopened, &fixture.request(2))
        .unwrap()
        .unwrap();
    assert!(validate_candidate(&reopened, &current).is_ok());
    // A real cancellation is an explicit choice even if automatic UI invalidation already sealed its token.
    current.request.active.store(false, Ordering::Release);
    reopened
        .record_bundle_decline(&current.package.manifest.id)
        .unwrap();
    drop(reopened);
    let mut reopened = fixture.manager();
    assert!(reopened.has_bundle_choice("bundled-prose").unwrap());
    assert!(
        prepare_offer(&mut reopened, &fixture.request(3))
            .unwrap()
            .is_none()
    );
}

/// An unrelated active extension never reads or decodes even a corrupt shipped ZIP.
#[test]
fn unmatched_file_does_not_read_the_zip_or_record_a_choice() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let file = fixture.workspace.join("unrelated.txt");
    std::fs::write(&file, "text").unwrap();
    std::fs::write(
        fixture.workspace.join(".bundled-plugin-test/generic.zip"),
        "not a ZIP",
    )
    .unwrap();
    let request = Request::new(1, fixture.workspace.clone(), fixture.root.clone(), file);
    assert!(prepare_offer(&mut manager, &request).unwrap().is_none());
    assert!(!fixture.root.join("bundle-choices.json").exists());
    assert!(manager.installed.is_empty());
}

/// Hash integrity, selector truth and portable paths are validated before consent or a host choice write.
#[test]
fn invalid_catalog_package_hash_selector_and_path_are_fail_closed() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    for (extension, filename, digest) in [
        (
            "prose",
            "generic.zip",
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("different", "generic.zip", None),
        ("prose", "../generic.zip", None),
        ("prose", "CON.zip", None),
    ] {
        fixture.catalog(extension, filename, digest);
        let file = fixture.workspace.join(format!("notes.{extension}"));
        std::fs::write(&file, "source").unwrap();
        let request = Request::new(1, fixture.workspace.clone(), fixture.root.clone(), file);
        assert!(prepare_offer(&mut manager, &request).is_err());
        assert!(!fixture.root.join("bundle-choices.json").exists());
        assert!(manager.installed.is_empty() && manager.live.is_empty());
    }
}

/// An alternative declaration arriving during consent wins; explicit disablement remains an installed choice.
#[test]
fn alternative_provider_during_consent_or_disabled_restart_is_never_overridden() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let candidate = prepare_offer(&mut manager, &fixture.request(1))
        .unwrap()
        .unwrap();
    let alternative = Package::from_bytes(&archive("alternative-prose", "1.0.0", "prose")).unwrap();
    manager
        .install(&alternative, alternative.manifest.permissions.clone())
        .unwrap();
    assert!(validate_candidate(&manager, &candidate).is_err());
    manager.disable("alternative-prose").unwrap();
    drop(manager);
    let mut reopened = fixture.manager();
    assert!(
        prepare_offer(&mut reopened, &fixture.request(2))
            .unwrap()
            .is_none()
    );
    assert!(!reopened.installed["alternative-prose"].enabled);
    assert!(!reopened.installed.contains_key("bundled-prose"));
}

/// Real actor publication is bounded by a deadline and observes the same state that native editor polling reads.
fn wait_for(worker: &Worker, predicate: impl Fn(&Published) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if predicate(&worker.state.lock().unwrap()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "bundled actor publication timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Discovery publishes only an immutable candidate; the ordinary actor installs it after native authorization.
#[test]
fn production_actor_restores_before_offering_and_installs_through_normal_lifecycle() {
    let fixture = Fixture::new();
    let worker = Worker::start_background(fixture.root.clone(), fixture.environment(), true);
    wait_for(&worker, |state| state.ready);
    worker
        .tx
        .send(Work::InspectBundle(fixture.request(1)))
        .unwrap();
    wait_for(&worker, |state| state.bundle_reply.is_some());
    let candidate = worker
        .state
        .lock()
        .unwrap()
        .bundle_reply
        .take()
        .unwrap()
        .result
        .unwrap()
        .unwrap();
    assert!(
        worker.state.lock().unwrap().entries.is_empty(),
        "inspection does not install or grant permissions"
    );
    assert!(worker.queue_lifecycle(Work::InstallBundle(candidate)));
    wait_for(&worker, |state| {
        state.progress.is_none() && !state.entries.is_empty()
    });
    let state = worker.state.lock().unwrap();
    assert!(state.ready);
    assert_eq!(state.entries[0].manifest.id, "bundled-prose");
    assert!(state.entries[0].enabled);
    assert!(
        state.pending.is_none(),
        "first use never opens the manual ZIP manager"
    );
}

/// Corrupt private registry recovery must never publish a healthy empty actor or inspect shipped defaults.
#[test]
fn actor_recovery_error_keeps_first_use_unavailable() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(&fixture.root).unwrap();
    std::fs::write(fixture.root.join("registry.json"), "{invalid").unwrap();
    let worker = Worker::start_background(fixture.root.clone(), fixture.environment(), true);
    wait_for(&worker, |state| state.status.is_some());
    let state = worker.state.lock().unwrap();
    assert!(!state.ready);
    assert!(state.entries.is_empty() && state.bundle_reply.is_none());
    assert!(!fixture.root.join("bundle-choices.json").exists());
}

/// Revocation during the actual resource preparation callback must reach the ordinary control cutover barrier.
#[test]
fn resource_only_candidate_revoked_during_preparation_cannot_commit() {
    let fixture = Fixture::new();
    let worker = Worker::start_background(fixture.root.clone(), fixture.environment(), true);
    wait_for(&worker, |state| state.ready);
    worker
        .tx
        .send(Work::InspectBundle(fixture.request(1)))
        .unwrap();
    wait_for(&worker, |state| state.bundle_reply.is_some());
    let candidate = worker
        .state
        .lock()
        .unwrap()
        .bundle_reply
        .take()
        .unwrap()
        .result
        .unwrap()
        .unwrap();
    let active = candidate.request.active.clone();
    let revoked = Arc::new(AtomicBool::new(false));
    let reached = revoked.clone();
    let control = plugin_runtime::InstallControl::new(move |stage| {
        if matches!(stage, plugin_runtime::InstallStage::Preparing) {
            active.store(false, Ordering::Release);
            reached.store(true, Ordering::Release);
        }
    });
    // Set the real published control before dispatch, rather than race a replacement after queueing.
    worker.state.lock().unwrap().install_control = Some(control);
    worker.tx.send(Work::InstallBundle(candidate)).unwrap();
    wait_for(&worker, |state| {
        state.status.is_some() || !state.entries.is_empty()
    });
    assert!(
        revoked.load(Ordering::Acquire),
        "the real resource preparation stage ran"
    );
    assert!(
        worker.state.lock().unwrap().entries.is_empty(),
        "a revoked resource-only candidate cannot commit its registry or providers"
    );
    let registry = Manager::read_registry(&fixture.root).unwrap();
    assert!(!registry.contains_key("bundled-prose"));
}
