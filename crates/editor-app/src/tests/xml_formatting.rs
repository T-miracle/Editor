//! Real XML formatting options preserve significant text through the installed plugin and native Undo.
use super::{
    language_editing::{open, shell},
    tag_editing::{assert_text, await_condition, await_text, text},
};
use crate::*;
use gpui_kit::{TestAppContext, gpui};
use plugin_runtime::{Package, plugin_protocol::settings::Scope};
use serde_json::json;

/// Mixed text, xml:space, indentation, attributes and empty-element styles use the current public package.
#[gpui::test]
#[ignore = "build current xml.zip; prepares the approved private native LemMinX service"]
fn installed_xml_formatting_preserves_text_and_honors_project_options(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("format.xml");
    let source = "<root a=\"1\" b=\"2\"><mixed>Hello <b>world</b> !</mixed><space xml:space=\"preserve\">  中文🙂\n x  </space><empty/></root>";
    std::fs::write(&path, source).unwrap();
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/xml.zip"),
    )
    .unwrap();
    let (app, visual, mut manager) = shell(cx, directory.path(), vec![package]);
    open(&app, &path, visual);
    let server = visual.update(|_, cx| app.read(cx).language_edits.formatters["xml"].clone());
    server.prepare_until_ready().unwrap();
    visual.update(|window, cx| {
        app.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.focus(window, cx));
        window.refresh();
        window.draw(cx).clear(cx);
    });
    visual.simulate_keystrokes("ctrl-s");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        source,
        "save formatting starts off"
    );
    visual.simulate_keystrokes("shift-alt-f");
    await_condition(
        visual,
        |visual| text(&app, visual) != source,
        "default XML formatter",
    );
    let formatted = text(&app, visual);
    assert!(
        formatted.contains("\n    <mixed>"),
        "indentation: {formatted:?}"
    );
    assert!(
        formatted.contains("<mixed>Hello <b>world</b> !</mixed>"),
        "mixed text must remain byte-for-byte: {formatted:?}"
    );
    assert!(
        formatted.contains("<space xml:space=\"preserve\">  中文🙂\n x  </space>"),
        "xml:space content: {formatted:?}"
    );
    assert!(formatted.contains("<empty/>") || formatted.contains("<empty />"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        source,
        "manual formatting remains an unsaved native edit"
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-y");
    await_text(&app, &formatted, visual, "one-step format Redo");
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    manager
        .update_setting(
            "xml",
            Scope::Project,
            "format_attributes",
            Some(json!("splitNewLine")),
        )
        .unwrap();
    manager
        .update_setting(
            "xml",
            Scope::Project,
            "format_empty_elements",
            Some(json!("expand")),
        )
        .unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    visual.simulate_keystrokes("shift-alt-f");
    await_condition(
        visual,
        |visual| text(&app, visual).contains("<empty></empty>"),
        "empty-element expansion",
    );
    let expanded = text(&app, visual);
    // LemMinX 0.31.2 measures splitAttributesIndentSize in indent levels: 2 × tabSize 4.
    assert!(
        expanded.contains("\n        a=\"1\"") && expanded.contains("\n        b=\"2\""),
        "split attributes: {expanded:?}"
    );
    assert!(
        expanded.contains("<mixed>Hello <b>world</b> !</mixed>")
            && expanded.contains("  中文🙂\n x  ")
    );
    manager
        .update_setting(
            "xml",
            Scope::Project,
            "format_empty_elements",
            Some(json!("collapse")),
        )
        .unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    visual.simulate_keystrokes("shift-alt-f");
    await_condition(
        visual,
        |visual| {
            text(&app, visual).contains("<empty/>") || text(&app, visual).contains("<empty />")
        },
        "empty-element collapse",
    );
    // The actual XML consumer also uses the common opt-in save path, after a native whole-document paste.
    crate::language::providers::set_editing_preference("xml", true, Some(true)).unwrap();
    visual.update(|_, cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(source.into()));
    });
    visual.simulate_keystrokes("ctrl-a ctrl-v ctrl-s");
    await_condition(
        visual,
        |_| {
            let saved = std::fs::read_to_string(&path).unwrap();
            saved != source
                && saved.contains("\n    <mixed>")
                && saved.contains("\n        a=\"1\"")
        },
        "XML opt-in save formats the pasted native source before writing",
    );
    let saved = std::fs::read_to_string(&path).unwrap();
    assert_text(&app, &saved, visual);
    assert!(saved.contains("<mixed>Hello <b>world</b> !</mixed>"));
    assert!(saved.contains("<space xml:space=\"preserve\">  中文🙂\n x  </space>"));
}
