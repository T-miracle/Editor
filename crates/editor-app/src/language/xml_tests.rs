//! End-to-end XML checks use the shipped ZIP, its WASM grammar and its approved native service.
use super::{code_highlighting, navigation, providers};
use plugin_runtime::{Manager, Package, plugin_protocol::Environment};
use std::{
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
mod schema_loading;

/// Install the real distribution, rather than duplicating its manifest or borrowing a host grammar.
fn installed_xml() -> (tempfile::TempDir, tempfile::TempDir, Manager, Package) {
    // Literal URI punctuation in both native roots must survive configuration and bundled SVG lookup.
    let workspace = tempfile::Builder::new().prefix("xml#%-").tempdir().unwrap();
    let store = tempfile::Builder::new()
        .prefix("plugins#%-")
        .tempdir()
        .unwrap();
    let archive = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/xml.zip");
    let package =
        Package::read(&archive).expect("build the XML package with build-plugins.ps1 first");
    let mut manager = Manager::open(
        store.path().to_path_buf(),
        Environment {
            workspace: workspace.path().display().to_string(),
            os: std::env::consts::OS.into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    (workspace, store, manager, package)
}

/// Five file formats acquire real highlight captures solely from the installed package declaration.
#[test]
#[ignore = "build the actual XML package first; prepares the pinned native language-service dependency"]
fn xml_plugin_package_enters_existing_language_registry() {
    let (workspace, store, mut manager, package) = installed_xml();
    let declaration = plugin_schema::PluginManifest::parse(
        std::str::from_utf8(&package.files["plugin.toml"]).unwrap(),
    )
    .unwrap();
    let root = store
        .path()
        .join("packages/xml")
        .join(&package.digest)
        .canonicalize()
        .unwrap();
    super::plugins::register_plugin(&root)
        .expect("actual XML grammar and query must load through the native registry");
    providers::configure(store.path(), workspace.path());
    providers::refresh(
        store.path(),
        vec![(
            "xml".into(),
            root,
            declaration.language_definitions,
            declaration.highlighters,
        )],
        vec![("xml".into(), package.manifest.language_servers.clone())],
    );
    let source = "<svg id=\"中文🙂\"><path d=\"M0 0\"/></svg>";
    for extension in ["xml", "svg", "xsd", "xsl", "xslt"] {
        assert_eq!(
            providers::language_for_path(&workspace.path().join(format!("sample.{extension}")))
                .as_deref(),
            Some("xml")
        );
        let selection = code_highlighting::selected(extension)
            .expect("installed XML recognition must select a grammar");
        let tokens = code_highlighting::highlight(
            &selection,
            source,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(30),
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.capture == "tag" && &source[token.range.clone()] == "svg"),
            "XML {extension} captures: {tokens:?}"
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.capture == "attribute" && &source[token.range.clone()] == "id")
        );
        assert!(tokens.iter().any(
            |token| token.capture == "string" && source[token.range.clone()].contains("中文🙂")
        ));
    }
    let services = manager.language_services();
    let service = services["xml/analysis"]
        .as_ref()
        .expect("authorized native dependency must prepare")
        .clone();
    let server = navigation::LanguageServer::from_service(service).unwrap();
    server.prepare_until_ready().unwrap();
    // Schema-free documents still learn local element/attribute names and close the nearest tag.
    let plain = completion_items(
        &server,
        workspace.path(),
        "plain.xml",
        "<root><item name=\"x\"/><",
    );
    assert!(
        plain.iter().any(|item| item.label.contains("item")),
        "missing document element suggestions: {plain:?}"
    );
    let close = completion_items(&server, workspace.path(), "plain.xml", "<root><item></");
    assert!(
        close.iter().any(|item| item.label.contains("item")),
        "missing closing-tag suggestion: {close:?}"
    );
    let attributes = completion_items(
        &server,
        workspace.path(),
        "plain.xml",
        "<root><item name=\"x\"/><item |/></root>",
    );
    assert!(
        attributes.iter().any(|item| item.label == "name"),
        "missing document attribute suggestions: {attributes:?}"
    );
    let eof = completion_items(
        &server,
        workspace.path(),
        "plain.xml",
        "<root><item 名称=\"中文🙂\"/><item ",
    );
    assert!(
        eof.iter().any(|item| item.label == "名称"),
        "missing Unicode EOF attribute suggestions: {eof:?}"
    );
    let svg = completion_items(
        &server,
        workspace.path(),
        "picture.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><",
    );
    assert!(
        svg.iter().any(|item| item.label == "path"),
        "missing bundled SVG suggestions: {svg:?}"
    );
    verify_schema_and_navigation(&server, workspace.path());
    schema_loading::verify(&mut manager, &server, workspace.path(), store.path());
    manager.uninstall("xml", false).unwrap();
    assert!(
        !server.is_active(),
        "uninstall must retire the installed service lease"
    );
    providers::refresh(store.path(), Vec::new(), Vec::new());
    assert!(code_highlighting::selected("xml").is_none());
}

/// The small bundled vocabulary supplies names without rejecting standard presentation inheritance.
#[test]
#[ignore = "build the actual XML package first; prepares its pinned native language-service dependency"]
fn xml_svg_assistance_preserves_valid_presentation_values() {
    let (workspace, _store, mut manager, _package) = installed_xml();
    let service = manager.language_services()["xml/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    let server = navigation::LanguageServer::from_service(service).unwrap();
    server.prepare_until_ready().unwrap();
    let attributes = completion_items(
        &server,
        workspace.path(),
        "inherit.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><path |/></svg>",
    );
    assert!(attributes.iter().any(|item| item.label == "d"));
    assert!(attributes.iter().any(|item| item.label == "stroke-linecap"));
    let source = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0\" stroke-linecap=\"inherit\" stroke-linejoin=\"inherit\" fill-rule=\"inherit\"/><text text-anchor=\"inherit\">合法🙂</text></svg>";
    let items = diagnostics(&server, workspace.path(), "inherit.svg", source);
    assert!(
        items.is_empty(),
        "bundled assistance must not reject legal SVG presentation values: {items:?}"
    );
    assert!(
        !diagnostics(
            &server,
            workspace.path(),
            "inherit.svg",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g></svg>"
        )
        .is_empty(),
        "presentation assistance must retain ordinary XML syntax diagnostics"
    );
}

/// Requests use production document leases and UTF-16 positions from the live unsaved text.
fn completion_items(
    server: &navigation::LanguageServer,
    root: &std::path::Path,
    name: &str,
    source: &str,
) -> Vec<lsp_types::CompletionItem> {
    // A cursor marker exercises an unfinished name inside the surrounding live document.
    let offset = source.find('|').unwrap_or(source.len());
    let source = source.replace('|', "");
    let document = server.open_document(navigation::file_uri(&root.join(name)).unwrap());
    let version = plugin_runtime::plugin_protocol::api::DocumentVersion {
        id: format!("xml-test-{name}"),
        path: name.into(),
        revision: offset as u64,
    };
    let response = server
        .completions_at_version(
            document,
            source.clone(),
            navigation::position_at_byte(&source, offset),
            version,
        )
        .unwrap();
    match response {
        lsp_types::CompletionResponse::Array(items) => items,
        lsp_types::CompletionResponse::List(list) => list.items,
    }
}

/// Actual local XSD/DTD bytes constrain unsaved completions and diagnostics, and identify navigation targets.
fn verify_schema_and_navigation(server: &navigation::LanguageServer, root: &std::path::Path) {
    let valid = "<root><item name=\"中文\"/></root>";
    assert!(
        diagnostics(server, root, "ordinary.xml", valid).is_empty(),
        "absence of schema must not be a warning"
    );
    for broken in [
        "<root><item></root>",
        "<root a=\"1\" a=\"2\"/>",
        "<unbound:root/>",
    ] {
        assert!(
            !diagnostics(server, root, "syntax.xml", broken).is_empty(),
            "missing syntax diagnostic for {broken}"
        );
    }
    let schema = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root"><xs:complexType><xs:sequence><xs:element ref="entry" minOccurs="0" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element>
  <xs:element name="entry"><xs:annotation><xs:documentation>An item in the catalog.</xs:documentation></xs:annotation><xs:complexType>
    <xs:attribute name="status" use="required"><xs:simpleType><xs:restriction base="xs:string"><xs:enumeration value="draft"/><xs:enumeration value="published"/></xs:restriction></xs:simpleType></xs:attribute>
  </xs:complexType></xs:element>
</xs:schema>"#;
    std::fs::write(root.join("model.xsd"), schema).unwrap();
    let prefix = "<root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"model.xsd\">";
    let children = completion_items(server, root, "catalog.xml", &format!("{prefix}<"));
    assert!(
        children.iter().any(|item| item.label == "entry"),
        "XSD child suggestions: {children:?}"
    );
    let attributes = completion_items(
        server,
        root,
        "catalog.xml",
        &format!("{prefix}<entry |/></root>"),
    );
    assert!(
        attributes.iter().any(|item| item.label == "status"),
        "XSD attribute suggestions: {attributes:?}"
    );
    let values = completion_items(
        server,
        root,
        "catalog.xml",
        &format!("{prefix}<entry status=\"|\"/></root>"),
    );
    assert!(
        values.iter().any(|item| item.label == "draft")
            && values.iter().any(|item| item.label == "published"),
        "XSD enum suggestions: {values:?}"
    );
    assert!(
        !diagnostics(
            server,
            root,
            "catalog.xml",
            &format!("{prefix}<entry status=\"invalid\"/></root>")
        )
        .is_empty()
    );
    let source = format!("{prefix}<entry status=\"draft\"/></root>");
    assert!(diagnostics(server, root, "catalog.xml", &source).is_empty());
    let uri = navigation::file_uri(&root.join("catalog.xml")).unwrap();
    let position = navigation::position_at_byte(&source, source.find("<entry").unwrap() + 2);
    let document = server.open_document(uri);
    let hover = server
        .hover_for(document.clone(), source.clone(), position)
        .unwrap()
        .expect("XSD-backed element hover");
    assert!(
        serde_json::to_string(&hover)
            .unwrap()
            .contains("An item in the catalog.")
    );
    let definitions = server
        .definitions_for(document, source.clone(), position)
        .unwrap();
    assert!(
        definitions
            .iter()
            .any(|location| location.target_uri.as_str().ends_with("/model.xsd")),
        "XSD definition target: {definitions:?}"
    );
    let target = definitions
        .iter()
        .find(|location| location.target_uri.as_str().ends_with("/model.xsd"))
        .unwrap();
    assert_eq!(
        target.target_selection_range.start.line, 3,
        "definition selects the actual XSD entry declaration"
    );
    // DTD remains a rule source, without registering a new .dtd editor language.
    std::fs::write(root.join("model.dtd"), "<!ELEMENT root (entry*)>\n<!ELEMENT entry EMPTY>\n<!ATTLIST entry status (draft|published) #REQUIRED>\n").unwrap();
    let dtd_prefix = "<!DOCTYPE root SYSTEM \"model.dtd\"><root>";
    let children = completion_items(server, root, "dtd.xml", &format!("{dtd_prefix}<"));
    assert!(
        children.iter().any(|item| item.label == "entry"),
        "DTD child suggestions: {children:?}"
    );
    let attributes = completion_items(
        server,
        root,
        "dtd.xml",
        &format!("{dtd_prefix}<entry |/></root>"),
    );
    assert!(
        attributes.iter().any(|item| item.label == "status"),
        "DTD attribute suggestions: {attributes:?}"
    );
    let values = completion_items(
        server,
        root,
        "dtd.xml",
        &format!("{dtd_prefix}<entry status=\"|\"/></root>"),
    );
    assert!(
        values.iter().any(|item| item.label == "draft")
            && values.iter().any(|item| item.label == "published"),
        "DTD enum suggestions: {values:?}"
    );
    assert!(
        !diagnostics(
            server,
            root,
            "dtd.xml",
            &format!("{dtd_prefix}<entry status=\"invalid\"/></root>")
        )
        .is_empty()
    );
    assert!(
        diagnostics(
            server,
            root,
            "dtd.xml",
            &format!("{dtd_prefix}<entry status=\"draft\"/></root>")
        )
        .is_empty()
    );
    let plain = "<root><item/></root>";
    let document =
        server.open_document(navigation::file_uri(&root.join("no-definition.xml")).unwrap());
    assert!(
        server
            .definitions_for(
                document,
                plain.into(),
                navigation::position_at_byte(plain, 9)
            )
            .unwrap()
            .is_empty(),
        "schema-free elements must not acquire invented definition targets"
    );
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><";
    let elements = completion_items(server, root, "picture.svg", svg);
    assert!(
        elements.iter().any(|item| item.label == "path"),
        "SVG element suggestions: {elements:?}"
    );
    let attributes = completion_items(
        server,
        root,
        "picture.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><path |/></svg>",
    );
    assert!(
        attributes.iter().any(|item| item.label == "d")
            && attributes.iter().any(|item| item.label == "fill"),
        "SVG attribute suggestions: {attributes:?}"
    );
}

/// Diagnostic pushes are asynchronous; poll the current document lease without accepting an older revision.
fn diagnostics(
    server: &navigation::LanguageServer,
    root: &std::path::Path,
    name: &str,
    source: &str,
) -> Vec<lsp_types::Diagnostic> {
    let document = server.open_document(navigation::file_uri(&root.join(name)).unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(items) = server.diagnostics_for(document.clone(), source).unwrap() {
            return items;
        }
        assert!(
            Instant::now() < deadline,
            "XML service never published current diagnostics for {name}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
