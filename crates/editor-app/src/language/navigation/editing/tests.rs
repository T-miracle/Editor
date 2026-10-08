//! Standard wire proposals are validated whole before the native revision-gated transaction.
use super::*;

/// A versioned or null-version TextDocumentEdit produces the same paired plan as changes.
#[test]
fn standard_rename_forms_preserve_one_complete_document_plan() {
    let uri: Uri = "file:///sample.novel".parse().unwrap();
    let changes = json!([
        {"range":{"start":{"line":0,"character":1},"end":{"line":0,"character":4}},"newText":"renamed"},
        {"range":{"start":{"line":0,"character":8},"end":{"line":0,"character":11}},"newText":"renamed"}
    ]);
    for response in [
        json!({"changes":{uri.as_str():changes.clone()}}),
        json!({"documentChanges":[{"textDocument":{"uri":uri,"version":7},"edits":changes.clone()}]}),
        json!({"documentChanges":[{"textDocument":{"uri":uri,"version":null},"edits":changes}]}),
    ] {
        let edits = rename_edits(response, &uri, 7, Clone::clone).unwrap();
        assert_eq!(edits.len(), 2);
        let merged = merge_edits("<one>x</one>", &edits).unwrap().unwrap();
        assert_eq!(merged.new_text, "renamed>x</renamed");
    }
}

/// Invalid metadata in a later edit cannot authorize a valid prefix or bypass confirmation.
#[test]
fn rename_rejects_stale_foreign_resource_mixed_and_annotated_plans() {
    let uri: Uri = "file:///sample.novel".parse().unwrap();
    let range = json!({"start":{"line":0,"character":1},"end":{"line":0,"character":4}});
    let good = json!({"textDocument":{"uri":uri,"version":7},"edits":[{"range":range,"newText":"renamed"}]});
    let mut stale = good.clone();
    stale["textDocument"]["version"] = json!(6);
    let mut foreign = good.clone();
    foreign["textDocument"]["uri"] = json!("file:///other.novel");
    let mut annotated = good.clone();
    annotated["edits"][0]["annotationId"] = json!("needs-confirmation");
    for response in [
        json!({"documentChanges":[good.clone(),stale]}),
        json!({"documentChanges":[good.clone(),foreign]}),
        json!({"documentChanges":[good.clone(),{"kind":"delete","uri":uri}]}),
        json!({"changes":{},"documentChanges":[good.clone()]}),
        json!({"documentChanges":[good,annotated]}),
        json!({"changes":{uri.as_str():[{"range":range,"newText":"renamed","annotationId":"needs-confirmation"}]}}),
    ] {
        assert!(rename_edits(response, &uri, 7, Clone::clone).is_err());
    }
}

/// Safe metadata does not make overlapping text ranges safe; the whole proposal is still refused.
#[test]
fn versioned_rename_overlap_still_fails_before_native_application() {
    let uri: Uri = "file:///sample.novel".parse().unwrap();
    let response = json!({"documentChanges":[{"textDocument":{"uri":uri,"version":7},"edits":[
        {"range":{"start":{"line":0,"character":1},"end":{"line":0,"character":4}},"newText":"renamed"},
        {"range":{"start":{"line":0,"character":3},"end":{"line":0,"character":5}},"newText":"bad suffix"}
    ]}]});
    let edits = rename_edits(response, &uri, 7, Clone::clone).unwrap();
    assert!(merge_edits("<one>x</one>", &edits).is_err());
}
