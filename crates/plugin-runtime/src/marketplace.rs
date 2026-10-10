//! Inert catalog snapshots become executable packages only after consent, download and validation.
//! This module never grants permissions or mutates a Manager's installed registry.
use crate::{InstallControl, Package, plugin_protocol::Manifest};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io::Read, sync::Arc, time::Duration};

/// The official reviewed registry; arbitrary user-provided market endpoints are not supported.
pub const CATALOG_URL: &str = "https://t-miracle.github.io/Nanobug-Plugin-Marketplace/catalog.json";
const MAX_CATALOG: usize = 16 * 1024 * 1024;

/// Generation-gated publication: cached data is browse-only until the latest refresh succeeds.
#[derive(Clone, Debug, Default)]
pub struct CatalogState {
    pub catalog: Option<Arc<Catalog>>,
    pub error: Option<String>,
    pub refreshing: bool,
    /// Unix seconds of the last successful fetch, retained across outages and process restarts.
    pub fetched_at: Option<u64>,
    generation: u64,
    fresh: bool,
}

impl CatalogState {
    /// Restore inert cached content. Persistence does not restore network authority.
    pub fn cached(catalog: Option<Catalog>) -> Self {
        Self {
            catalog: catalog.map(Arc::new),
            ..Self::default()
        }
    }
    /// Invalidate outstanding consent/download authority immediately when requesting a refresh.
    pub fn begin_refresh(&mut self) -> u64 {
        self.generation += 1;
        self.fresh = false;
        self.refreshing = true;
        self.error = None;
        self.generation
    }
    /// Publish only the latest response; errors preserve older content solely for browsing.
    pub fn finish_refresh(&mut self, generation: u64, result: Result<Catalog, String>) -> bool {
        if generation != self.generation {
            return false;
        }
        self.refreshing = false;
        match result {
            Ok(catalog) => {
                self.catalog = Some(Arc::new(catalog));
                self.fetched_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|v| v.as_secs());
                self.fresh = true;
                self.error = None;
            }
            Err(error) => {
                self.fresh = false;
                self.error = Some(error);
            }
        }
        true
    }
    /// Download authority belongs to this in-memory successful refresh only.
    pub fn fresh(&self) -> bool {
        self.fresh
    }
    /// Compare this token again at consent acceptance and installation commit.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Disk cache records age but never carries installation authority.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogCache {
    pub fetched_at: u64,
    pub catalog: Catalog,
}
impl CatalogCache {
    /// Reapply the same bounded catalog invariants after decoding the local envelope.
    pub fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(bytes.len() <= MAX_CATALOG + 128, "Cache exceeds limit");
        let cache: Self = serde_json::from_slice(bytes)?;
        Catalog::parse(&serde_json::to_vec(&cache.catalog)?)?;
        Ok(cache)
    }
}

/// Versioned publication. Unknown fields are rejected to avoid silently ignoring new security policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub plugins: Vec<Listing>,
}

/// Searchable listing; absent download counts mean unavailable, never zero installs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listing {
    pub id: String,
    pub repository: String,
    pub maintainers: Vec<String>,
    pub summary: String,
    pub category: String,
    pub tags: Vec<String>,
    pub downloads: Option<u64>,
    pub versions: Vec<Release>,
}

/// All authorization claims are an inert manifest snapshot, compared again with downloaded bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub version: String,
    pub manifest: Manifest,
    pub host_version: String,
    pub platforms: Vec<String>,
    pub sha256: String,
    pub commit: String,
    pub license: String,
    pub url: String,
    pub asset_id: u64,
    pub withdrawn: bool,
    pub published_at: String,
    pub readme: String,
    pub icon: String,
    pub changelog: String,
}

/// UI sort choices are independent of locale; ties always use stable IDs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Relevance,
    Downloads,
    Updated,
    Name,
}

impl Catalog {
    /// Parse bounded JSON and enforce identity/source invariants before caching or displaying it.
    pub fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(bytes.len() <= MAX_CATALOG, "Catalog exceeds 16 MiB");
        let catalog: Self = serde_json::from_slice(bytes)?;
        anyhow::ensure!(catalog.schema == 1, "Unsupported marketplace schema");
        let mut identities = BTreeSet::new();
        for plugin in &catalog.plugins {
            anyhow::ensure!(
                plugin.repository.split('/').count() == 2
                    && plugin.repository.split('/').all(|part| !part.is_empty()
                        && part != "."
                        && part != ".."
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))),
                "Invalid GitHub repository"
            );
            anyhow::ensure!(
                identities.insert(&plugin.id),
                "Duplicate marketplace identity"
            );
            anyhow::ensure!(!plugin.versions.is_empty(), "No reviewed versions");
            let mut versions = BTreeSet::new();
            for version in &plugin.versions {
                let parsed = semver::Version::parse(&version.version)?;
                anyhow::ensure!(
                    parsed.pre.is_empty() && parsed.build.is_empty(),
                    "Only stable versions"
                );
                anyhow::ensure!(
                    versions.insert(&version.version),
                    "Duplicate marketplace version"
                );
                anyhow::ensure!(
                    version.manifest.id == plugin.id && version.manifest.version == version.version,
                    "Catalog identity mismatch"
                );
                anyhow::ensure!(
                    version.sha256.len() == 64
                        && version.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                    "Missing SHA-256"
                );
                anyhow::ensure!(
                    version.commit.len() == 40
                        && version.commit.bytes().all(|b| b.is_ascii_hexdigit()),
                    "Missing source commit"
                );
                semver::VersionReq::parse(&version.host_version)?;
                validate_url(&version.url)?;
                // Loopback is the network test adapter; production release URLs stay in their named repository.
                anyhow::ensure!(
                    version.url.starts_with(&format!(
                        "https://github.com/{}/releases/download/",
                        plugin.repository
                    )) || is_loopback(&version.url),
                    "Unreviewed package source"
                );
            }
        }
        Ok(catalog)
    }

    /// Fetching failure leaves cache policy to the caller; a saved catalog never proves freshness.
    pub fn fetch(url: &str, control: &InstallControl) -> anyhow::Result<Self> {
        let catalog = Self::parse(&receive(url, MAX_CATALOG, control, |_| {})?)?;
        // Loopback releases belong only to a loopback fixture catalog, never to an Internet catalog.
        if !is_loopback(url) {
            anyhow::ensure!(
                catalog
                    .plugins
                    .iter()
                    .flat_map(|p| &p.versions)
                    .all(|v| !is_loopback(&v.url)),
                "Remote catalog cannot refer to loopback packages"
            );
        }
        Ok(catalog)
    }

    /// Search five declared fields and optionally filter the main category and one tag.
    pub fn search(&self, query: &str, category: &str, tag: &str, sort: Sort) -> Vec<&Listing> {
        let query = query.to_lowercase();
        let mut rows: Vec<_> = self
            .plugins
            .iter()
            .filter(|p| {
                (category.is_empty() || p.category == category)
                    && (tag.is_empty() || p.tags.iter().any(|t| t == tag))
                    && (query.is_empty() || p.score(&query) > 0)
            })
            .collect();
        rows.sort_by(|a, b| {
            let order = match sort {
                Sort::Relevance if !query.is_empty() => b.score(&query).cmp(&a.score(&query)),
                Sort::Downloads | Sort::Relevance => b.downloads.cmp(&a.downloads),
                Sort::Updated => b
                    .versions
                    .iter()
                    .map(|v| &v.published_at)
                    .max()
                    .cmp(&a.versions.iter().map(|v| &v.published_at).max()),
                Sort::Name => a.versions[0]
                    .manifest
                    .name
                    .to_lowercase()
                    .cmp(&b.versions[0].manifest.name.to_lowercase()),
            };
            order.then_with(|| a.id.cmp(&b.id))
        });
        rows
    }
}

impl Listing {
    fn score(&self, query: &str) -> usize {
        [&self.id, &self.versions[0].manifest.name, &self.summary]
            .into_iter()
            .chain(self.maintainers.iter())
            .chain(self.tags.iter())
            .map(|s| {
                if s.to_lowercase() == query {
                    3
                } else {
                    usize::from(s.to_lowercase().contains(query))
                }
            })
            .sum()
    }

    /// Select the newest compatible stable release, falling back to an older reviewed version.
    pub fn compatible(&self, host: &str, platform: &str) -> Option<&Release> {
        self.versions
            .iter()
            .filter(|v| v.compatible(host, platform).is_ok())
            .max_by_key(|v| semver::Version::parse(&v.version).ok())
    }
}

impl Release {
    /// Compatibility does not grant permission; Manager rechecks trust and actual package capabilities.
    pub fn compatible(&self, host: &str, platform: &str) -> anyhow::Result<()> {
        anyhow::ensure!(!self.withdrawn, "Version withdrawn");
        anyhow::ensure!(
            semver::VersionReq::parse(&self.host_version)?.matches(&semver::Version::parse(host)?),
            "Host version incompatible"
        );
        anyhow::ensure!(
            self.platforms.is_empty() || self.platforms.iter().any(|p| p == platform),
            "Platform incompatible"
        );
        crate::capabilities::require_current(&self.manifest)
    }

    /// Download only after host consent; cancellation, quotas, digest and all manifest claims are checked.
    /// The progress callback receives bytes read. No archive is retained as an offline package cache.
    pub fn download(
        &self,
        control: &InstallControl,
        progress: impl FnMut(u64),
    ) -> anyhow::Result<Package> {
        control.check()?;
        let bytes = receive(&self.url, 64 * 1024 * 1024, control, progress)?;
        let mut package = Package::from_bytes(&bytes)?;
        anyhow::ensure!(
            package.digest == self.sha256,
            "Marketplace SHA-256 mismatch"
        );
        anyhow::ensure!(
            serde_json::to_value(&package.manifest)? == serde_json::to_value(&self.manifest)?,
            "Marketplace manifest mismatch"
        );
        let host_version = if let Some(path) = &package.manifest.contributions {
            let bytes = package.files.get(path).context("Missing contributions")?;
            plugin_schema::PluginManifest::parse(std::str::from_utf8(bytes)?)?
                .plugin
                .host_version
        } else {
            "*".into()
        };
        anyhow::ensure!(
            host_version == self.host_version,
            "Marketplace host compatibility mismatch"
        );
        // Current protocol may carry per-service platform claims; earlier committed SDKs omit
        // the field. Compare the canonical serialized manifest in either case without relying
        // on another uncommitted protocol change.
        let service_claims = serde_json::to_value(&package.manifest.services)?;
        let mut sets = service_claims
            .as_object()
            .into_iter()
            .flat_map(|services| services.values())
            .filter_map(|service| service.get("platforms")?.as_array())
            .filter(|platforms| !platforms.is_empty())
            .map(|platforms| {
                platforms
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect::<BTreeSet<_>>()
            });
        let platforms = sets
            .next()
            .map(|first| sets.fold(first, |a, b| a.intersection(&b).cloned().collect()))
            .unwrap_or_default();
        anyhow::ensure!(
            platforms == self.platforms.iter().cloned().collect(),
            "Marketplace platforms mismatch"
        );
        control.check()?;
        package.source = Some(self.url.clone());
        Ok(package)
    }
}

fn is_loopback(url: &str) -> bool {
    url.parse::<ureq::http::Uri>().ok().is_some_and(|uri| {
        uri.scheme_str() == Some("http")
            && uri.host().is_some_and(|h| {
                h.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
            })
    })
}

fn validate_url(url: &str) -> anyhow::Result<()> {
    let uri: ureq::http::Uri = url.parse()?;
    anyhow::ensure!(
        uri.authority().is_some_and(|a| !a.as_str().contains('@'))
            && (uri.scheme_str() == Some("https") || is_loopback(url)),
        "Marketplace requires HTTPS without credentials"
    );
    Ok(())
}

/// A bounded producer makes cancellation responsive even while DNS/TLS or a socket read is blocked.
fn receive(
    url: &str,
    limit: usize,
    control: &InstallControl,
    mut progress: impl FnMut(u64),
) -> anyhow::Result<Vec<u8>> {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    static ACTIVE: AtomicUsize = AtomicUsize::new(0);
    validate_url(url)?;
    control.check()?;
    ACTIVE
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < 4).then_some(n + 1)
        })
        .map_err(|_| anyhow::anyhow!("Marketplace downloads busy"))?;
    struct Permit;
    impl Drop for Permit {
        fn drop(&mut self) {
            ACTIVE.fetch_sub(1, Ordering::AcqRel);
        }
    }
    let permit = Permit;
    let url = url.to_owned();
    let producer_control = control.clone();
    let (tx, rx) = mpsc::sync_channel(2);
    std::thread::Builder::new()
        .name("marketplace-download".into())
        .spawn(move || {
            let _permit = permit;
            let result = (|| -> anyhow::Result<()> {
                let agent = ureq::Agent::config_builder()
                    .timeout_global(Some(Duration::from_secs(90)))
                    .timeout_connect(Some(Duration::from_secs(10)))
                    .timeout_resolve(Some(Duration::from_secs(10)))
                    .https_only(url.starts_with("https://"))
                    .max_redirects(if is_loopback(&url) { 0 } else { 5 })
                    .build()
                    .new_agent();
                let mut response = agent.get(&url).call()?;
                let mut reader = response.body_mut().as_reader();
                let mut buffer = [0; 32768];
                loop {
                    producer_control.check()?;
                    let count = reader.read(&mut buffer)?;
                    if count == 0 {
                        return Ok(());
                    }
                    tx.send(Ok(buffer[..count].to_vec()))
                        .context("Download cancelled")?;
                }
            })();
            let _ = tx.send(result.map(|_| Vec::new()).map_err(|e| format!("{e:#}")));
        })?;
    let mut bytes = Vec::new();
    loop {
        control.check()?;
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(chunk)) if chunk.is_empty() => return Ok(bytes),
            Ok(Ok(chunk)) => {
                anyhow::ensure!(
                    bytes.len() + chunk.len() <= limit,
                    "Marketplace response quota exceeded"
                );
                bytes.extend(chunk);
                progress(bytes.len() as u64);
            }
            Ok(Err(error)) => anyhow::bail!("{error}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => anyhow::bail!("Marketplace download interrupted"),
        }
    }
}
