//! Real LemMinX resource loading proves opt-in networking, private cache reuse and visible failures.
use super::*;
use plugin_runtime::plugin_protocol::settings::Scope;
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const REMOTE_SCHEMA: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="root"><xs:complexType><xs:sequence><xs:element name="remote-entry" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:attribute name="state"><xs:simpleType><xs:restriction base="xs:string"><xs:enumeration value="cached-value"/></xs:restriction></xs:simpleType></xs:attribute></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#;

/// One installation exercises real settings changes; the HTTP endpoint counts actual downloads.
pub(super) fn verify(
    manager: &mut Manager,
    server: &navigation::LanguageServer,
    workspace: &std::path::Path,
    store: &std::path::Path,
) {
    let endpoint = SchemaEndpoint::new(false);
    let remote = format!(
        "<root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"{}\">",
        endpoint.url
    );
    let missing = diagnostics(
        server,
        workspace,
        "missing.xml",
        "<root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"missing.xsd\"/>",
    );
    assert!(
        missing
            .iter()
            .any(|item| item.message.contains("missing.xsd")),
        "explicit missing schema must report a load problem: {missing:?}"
    );
    same_document_history(server, workspace, "missing.xml", "missing.xsd");
    // A later valid rule source on the same URI must not inherit the old load-failure summary.
    let valid_source = "<root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"model.xsd\"><entry alien=\"x\"/><entry |/></root>";
    let valid_items = completion_items(server, workspace, "missing.xml", valid_source);
    assert!(valid_items.iter().any(|item| item.label == "status"));
    assert!(
        !valid_items.iter().any(|item| item.label == "alien"),
        "successful XSD constraints cannot inherit failed-rule history"
    );
    let disabled = diagnostics(server, workspace, "remote.xml", &format!("{remote}</root>"));
    assert!(
        disabled
            .iter()
            .any(|item| item.message.to_lowercase().contains("disabled")),
        "remote opt-out must remain visible: {disabled:?}"
    );
    assert_eq!(
        endpoint.requests.load(Ordering::Acquire),
        0,
        "disabled downloads cannot touch the HTTP endpoint"
    );
    same_document_history(server, workspace, "remote.xml", &endpoint.url);
    // Exact absolute associations acquire an equal-boundary alias pattern through the XML configuration hook.
    let pattern = workspace
        .join("associated.xml")
        .display()
        .to_string()
        .replace('\\', "/");
    // Rules in an owned sibling directory require an actual parent step before URI encoding.
    let rules = tempfile::Builder::new()
        .prefix("xml-rules#%-")
        .tempdir_in(workspace.parent().unwrap())
        .unwrap();
    let relative_rules = format!("../{}", rules.path().file_name().unwrap().to_str().unwrap());
    std::fs::write(
        rules.path().join("model#%.xsd"),
        std::fs::read(workspace.join("model.xsd")).unwrap(),
    )
    .unwrap();
    setting(
        manager,
        "schema_associations",
        json!(
            json!([
            {"pattern":pattern,"systemId":format!("{relative_rules}/model#%.xsd")},
            {"pattern":"nested/associated.xml","systemId":format!("{relative_rules}/model#%.xsd")}
            ])
            .to_string()
        ),
    );
    let associated = ready_server(manager);
    let children = completion_items(&associated, workspace, "associated.xml", "<root><");
    assert!(
        children.iter().any(|item| item.label == "entry"),
        "absolute association must match immutable wire paths: {children:?}"
    );
    let constrained = completion_items(
        &associated,
        workspace,
        "associated.xml",
        "<root><entry alien=\"x\"/><entry |/></root>",
    );
    assert!(constrained.iter().any(|item| item.label == "status"));
    assert!(!constrained.iter().any(|item| item.label == "alien"));
    std::fs::create_dir_all(workspace.join("outer/nested")).unwrap();
    let nested = completion_items(
        &associated,
        workspace,
        "outer/nested/associated.xml",
        "<root><entry alien=\"x\"/><entry |/></root>",
    );
    assert!(nested.iter().any(|item| item.label == "status"));
    assert!(
        !nested.iter().any(|item| item.label == "alien"),
        "relative association suffixes must obey the same native directory boundary"
    );
    let unrelated = "<root><item history=\"x\"/><item |/></root>";
    assert!(
        completion_items(&associated, workspace, "unrelated.xml", unrelated)
            .iter()
            .any(|item| item.label == "history"),
        "an absolute association for another document cannot suppress its schema-free attributes"
    );
    assert!(
        completion_items(
            &associated,
            workspace,
            "unrelated.xml",
            "<root xmlns:data=\"urn:ordinary\" data:schemaLocation=\"unused\"><item history=\"x\"/><item |/></root>"
        ).iter().any(|item| item.label == "history"),
        "an attribute in an unrelated namespace is not a Schema declaration"
    );
    std::fs::write(
        rules.path().join("model.dtd"),
        std::fs::read(workspace.join("model.dtd")).unwrap(),
    )
    .unwrap();
    std::fs::write(rules.path().join("catalog#%.xml"), "<catalog xmlns=\"urn:oasis:names:tc:entity:xmlns:xml:catalog\"><system systemId=\"https://schemas.invalid/editor-probe.dtd\" uri=\"model.dtd\"/></catalog>").unwrap();
    setting(
        manager,
        "catalogs",
        json!(json!([format!("{relative_rules}/catalog#%.xml")]).to_string()),
    );
    let catalog = ready_server(manager);
    assert!(
        completion_items(&catalog, workspace, "unrelated.xml", unrelated)
            .iter()
            .any(|item| item.label == "history"),
        "an unused catalog cannot suppress a schema-free document's attributes"
    );
    let catalog_values = completion_items(
        &catalog,
        workspace,
        "catalog-bound.xml",
        "<!DOCTYPE root SYSTEM \"https://schemas.invalid/editor-probe.dtd\"><root><entry status=\"|\"/></root>",
    );
    assert!(
        catalog_values.iter().any(|item| item.label == "draft")
            && catalog_values.iter().any(|item| item.label == "published"),
        "parent-relative catalog must resolve its own relative DTD, with networking disabled: {catalog_values:?}"
    );
    setting(manager, "schema_associations", json!("[]"));
    setting(manager, "download_schemas", json!(true));
    let online = ready_server(manager);
    wait_candidate(
        &online,
        workspace,
        "remote.xml",
        &format!("{remote}<"),
        "remote-entry",
    );
    wait_candidate(
        &online,
        workspace,
        "remote.xml",
        &format!("{remote}<remote-entry state=\"|\"/></root>"),
        "cached-value",
    );
    assert!(
        endpoint.requests.load(Ordering::Acquire) >= 1,
        "opt-in must actually fetch schema bytes"
    );
    let service = manager.language_services()["xml/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    let cache = PathBuf::from(
        service.provider.initialization_options["settings"]["xml"]["server"]["workDir"]
            .as_str()
            .unwrap(),
    );
    assert!(
        cache
            .canonicalize()
            .unwrap()
            .starts_with(store.canonicalize().unwrap())
    );
    assert!(
        contains_schema(&cache),
        "downloaded XSD must be in the instance's private cache, not the project"
    );
    assert!(!workspace.join("remote.xsd").exists());
    // Shut down the origin and restart with downloads disabled: suggestions must come from saved private bytes.
    drop(endpoint);
    setting(manager, "download_schemas", json!(false));
    let offline = ready_server(manager);
    wait_candidate(
        &offline,
        workspace,
        "remote.xml",
        &format!("{remote}<remote-entry state=\"|\"/></root>"),
        "cached-value",
    );
    assert!(
        diagnostics(
            &offline,
            workspace,
            "remote.xml",
            &format!("{remote}<remote-entry state=\"cached-value\"/></root>")
        )
        .is_empty()
    );
    verify_failed_resource(manager, workspace);
    // An explicit local executable also prepares through the approved resolver, without changing global PATH.
    let executable = find_native_service(store)
        .expect("managed installation contains the actual native executable");
    setting(
        manager,
        "executable",
        json!(executable.display().to_string()),
    );
    let local = ready_server(manager);
    assert!(
        completion_items(&local, workspace, "local.xml", "<root><item></")
            .iter()
            .any(|item| item.label.contains("item"))
    );
    // Invalid explicit tools are service preparation errors; the independently registered grammar remains live.
    setting(
        manager,
        "executable",
        json!(workspace.join("missing-server.exe").display().to_string()),
    );
    assert!(manager.language_services()["xml/analysis"].is_err());
    assert!(code_highlighting::selected("xml").is_some());
    manager
        .update_setting("xml", Scope::Project, "executable", None)
        .unwrap();
    assert!(
        ready_server(manager).is_active(),
        "clearing the override reuses the approved managed installation"
    );
}

/// A fresh unavailable URL reports loading failure while ordinary syntax and closing completion survive.
fn verify_failed_resource(manager: &mut Manager, workspace: &std::path::Path) {
    let failed = SchemaEndpoint::new(true);
    setting(manager, "download_schemas", json!(true));
    let failing = ready_server(manager);
    let failure = format!(
        "<root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"{}\"/>",
        failed.url
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let items = diagnostics(&failing, workspace, "failed-remote.xml", &failure);
        if failed.requests.load(Ordering::Acquire) > 0
            && items.iter().any(|item| {
                item.message.contains("503")
                    || item.message.to_lowercase().contains("failed")
                    || item.message.to_lowercase().contains("cannot")
            })
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "remote fetch failure never became a load diagnostic: {items:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    same_document_history(&failing, workspace, "failed-remote.xml", &failed.url);
    let close = completion_items(&failing, workspace, "plain.xml", "<root><item></");
    assert!(close.iter().any(|item| item.label.contains("item")));
    assert!(!diagnostics(&failing, workspace, "plain.xml", "<root><item></root>").is_empty());
}

/// The failure belongs to the exact unsaved document, rather than a different plain-file completion request.
fn same_document_history(
    server: &navigation::LanguageServer,
    workspace: &std::path::Path,
    name: &str,
    schema: &str,
) {
    let source = format!(
        "<root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"{schema}\"><item 历史属性=\"中文🙂\"/><item |/></root>"
    );
    let current = source.replace('|', "");
    assert!(!diagnostics(server, workspace, name, &current).is_empty());
    let items = completion_items(server, workspace, name, &source);
    assert!(
        items.iter().any(|item| item.label == "历史属性"),
        "failed/disabled same-document Schema must retain attribute history: {items:?}"
    );
}

/// Real settings apply through the same manager transaction used by native preference controls.
fn setting(manager: &mut Manager, key: &str, value: serde_json::Value) {
    manager
        .update_setting("xml", Scope::Project, key, Some(value))
        .unwrap();
}

/// Providers are rebuilt after configuration changes; no stale adapter is borrowed across them.
fn ready_server(manager: &mut Manager) -> navigation::LanguageServer {
    let service = manager.language_services()["xml/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    let server = navigation::LanguageServer::from_service(service).unwrap();
    server.prepare_until_ready().unwrap();
    server
}

/// Download completion can follow an initial loading diagnostic; wait for real constrained candidates.
fn wait_candidate(
    server: &navigation::LanguageServer,
    root: &std::path::Path,
    name: &str,
    source: &str,
    label: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let items = completion_items(server, root, name, source);
        if items.iter().any(|item| item.label == label) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "cached/remote {label} suggestions missing: {items:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Small XML bytes must be observable beneath the declared cache directory, not inferred from capabilities.
fn contains_schema(path: &std::path::Path) -> bool {
    std::fs::read_dir(path)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                contains_schema(&path)
            } else {
                std::fs::read(path).is_ok_and(|bytes| bytes == REMOTE_SCHEMA.as_bytes())
            }
        })
}

/// Locate only a fixture-owned prepared artifact, then use the public explicit-tool preference path.
fn find_native_service(path: &std::path::Path) -> Option<PathBuf> {
    std::fs::read_dir(path)
        .ok()?
        .filter_map(Result::ok)
        .find_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                find_native_service(&path)
            } else {
                (path.file_name()?.to_str()? == "lemminx-win32.exe").then_some(path)
            }
        })
}

/// A bounded local origin isolates network policy from public network availability.
struct SchemaEndpoint {
    url: String,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl SchemaEndpoint {
    /// Reply with known bytes or HTTP 503; this listener is solely an external resource fixture.
    fn new(fail: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/remote.xsd", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let stopped = stop.clone();
        let count = requests.clone();
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                if let Ok((mut socket, _)) = listener.accept() {
                    socket
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut request = [0u8; 8192];
                    if socket.read(&mut request).is_ok() {
                        count.fetch_add(1, Ordering::Release);
                        let status = if fail {
                            "503 Service Unavailable"
                        } else {
                            "200 OK"
                        };
                        let body = if fail {
                            "fixture resource unavailable"
                        } else {
                            REMOTE_SCHEMA
                        };
                        let _ = write!(
                            socket,
                            "HTTP/1.1 {status}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                    }
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for SchemaEndpoint {
    /// Closing the origin makes the later cache check actually offline and joins the bounded listener.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}
