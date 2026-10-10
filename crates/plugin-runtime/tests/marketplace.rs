//! Catalog-to-Manager tests use actual HTTP bytes and ZIP validation, never an installed-state stub.
use plugin_runtime::{InstallControl, Manager, marketplace::Catalog, plugin_protocol::Environment};
use std::io::{Read, Write};

/// Cached browsing never authorizes a new download, and older replies cannot revive that authority.
#[test]
fn refresh_failure_blocks_installs_and_late_success_cannot_overwrite_newer_state() {
    use plugin_runtime::marketplace::CatalogState;
    let empty = || Catalog::parse(br#"{"schema":1,"plugins":[]}"#).unwrap();
    let mut state = CatalogState::cached(Some(empty()));
    assert!(!state.fresh());
    let old = state.begin_refresh();
    let current = state.begin_refresh();
    assert!(!state.finish_refresh(old, Ok(empty())));
    assert!(state.finish_refresh(current, Err("offline".into())));
    assert!(!state.fresh());
    assert!(state.catalog.is_some());
    let retry = state.begin_refresh();
    assert!(state.finish_refresh(retry, Ok(empty())));
    assert!(state.fresh());
    let fetched_at = state.fetched_at.unwrap();
    let cache = plugin_runtime::marketplace::CatalogCache {
        fetched_at,
        catalog: state.catalog.as_deref().unwrap().clone(),
    };
    let restored =
        plugin_runtime::marketplace::CatalogCache::parse(&serde_json::to_vec(&cache).unwrap())
            .unwrap();
    assert_eq!(restored.fetched_at, fetched_at);
    let generation = state.begin_refresh();
    state.finish_refresh(generation, Err("offline".into()));
    assert_eq!(state.fetched_at, Some(fetched_at));
}

/// The only adapter is an HTTP server; installation remains the public production transaction.
#[test]
fn reviewed_catalog_download_installs_a_real_declarative_package() {
    let bytes = include_bytes!("fixtures/marketplace/notes.zip");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    // Only transport changes: contract and package bytes come directly from the registry builder.
    let mut catalog = Catalog::parse(include_bytes!("fixtures/marketplace/catalog.json")).unwrap();
    catalog.plugins[0].versions[0].url = format!("http://{address}/notes.zip");
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        socket.read(&mut request).unwrap();
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .unwrap();
        socket.write_all(bytes).unwrap();
    });
    let version = catalog.plugins[0]
        .compatible("0.1.0", "windows-x86_64")
        .unwrap();
    let package = version
        .download(&InstallControl::default(), |_| {})
        .unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        home.path().join("runtime"),
        Environment {
            workspace: home.path().to_string_lossy().into_owned(),
            ..Environment::default()
        },
    )
    .unwrap();
    manager
        .install_with_control(
            &package,
            package.manifest.permissions.clone(),
            &InstallControl::default(),
        )
        .unwrap();
    assert_eq!(manager.installed["test.notes"].manifest.version, "1.0.0");
    server.join().unwrap();
}

/// Five search fields, category/tag filters and fallback selection share the catalog consumer rules.
#[test]
fn searchable_metadata_and_compatible_stable_fallback_are_consistent() {
    use plugin_runtime::marketplace::Sort;
    let mut catalog = Catalog::parse(include_bytes!("fixtures/marketplace/catalog.json")).unwrap();
    let first = &mut catalog.plugins[0];
    first.summary = "Distinct summary".into();
    first.tags = vec!["tag-only".into()];
    for query in ["Notes", "test.notes", "Distinct", "author", "tag-only"] {
        assert_eq!(
            catalog
                .search(query, "tools", "tag-only", Sort::Relevance)
                .len(),
            1,
            "{query}"
        );
    }
    assert!(catalog.search("", "other", "", Sort::Name).is_empty());
    assert!(
        catalog
            .search("", "", "missing", Sort::Downloads)
            .is_empty()
    );
    let mut newer = catalog.plugins[0].versions[0].clone();
    newer.version = "2.0.0".into();
    newer.manifest.version = newer.version.clone();
    newer.host_version = ">=2.0.0".into();
    catalog.plugins[0].versions.push(newer);
    assert_eq!(
        catalog.plugins[0]
            .compatible("0.1.0", "windows-x86_64")
            .unwrap()
            .version,
        "1.0.0"
    );
    assert_eq!(
        catalog.plugins[0]
            .compatible("2.0.0", "windows-x86_64")
            .unwrap()
            .version,
        "2.0.0"
    );
    catalog.plugins[0].versions[1].withdrawn = true;
    assert_eq!(
        catalog.plugins[0]
            .compatible("2.0.0", "windows-x86_64")
            .unwrap()
            .version,
        "1.0.0"
    );
}
