//! Method-bound range acceptance keeps standard LSP equality and generic semantic safety distinct.
use super::*;
use lsp_types::{LinkedEditingRanges, Position};

/// Construct wire-shaped proposals from known byte intervals using the public UTF-16 conversion.
fn proposal(
    source: &str,
    ranges: Vec<Range<usize>>,
    authority: LinkedEditingAuthority,
) -> LinkedEditingProposal {
    LinkedEditingProposal {
        response: LinkedEditingRanges {
            ranges: ranges
                .into_iter()
                .map(|range| lsp_types::Range {
                    start: navigation::position_at_byte(source, range.start),
                    end: navigation::position_at_byte(source, range.end),
                })
                .collect(),
            word_pattern: Some("[A-Za-z]+".into()),
        },
        authority,
    }
}

/// An untrusted additional response field cannot bypass the standard protocol's identical-text rule.
#[test]
fn standard_linked_ranges_reject_different_names_even_with_wire_flag() {
    let source = "<DIV></div>";
    let ordinary = proposal(source, vec![1..4, 7..10], LinkedEditingAuthority::Standard);
    let mut wire = serde_json::to_value(ordinary.response).unwrap();
    wire["semantic"] = serde_json::json!(true);
    let response = serde_json::from_value(wire).unwrap();
    assert!(
        Group::from_response(
            source,
            2,
            LinkedEditingProposal {
                response,
                authority: LinkedEditingAuthority::Standard,
            }
        )
        .is_err()
    );
    let equal = "<div></div>";
    assert!(
        Group::from_response(
            equal,
            2,
            proposal(equal, vec![1..4, 7..10], LinkedEditingAuthority::Standard)
        )
        .is_ok()
    );
}

/// Semantic authority permits unequal initial lengths while retaining every endpoint's safety guards.
#[test]
fn semantic_linked_ranges_accept_different_names_and_reject_unsafe_peers() {
    let source = "🙂 <long></x>";
    let ranges = vec![6..10, 13..14];
    let valid = || proposal(source, ranges.clone(), LinkedEditingAuthority::Semantic);
    assert!(Group::from_response(source, 8, valid()).is_ok());
    assert!(
        Group::from_response(source, 0, valid()).is_err(),
        "caret must belong to one name"
    );

    let mut overlapping = valid();
    overlapping.response.ranges[1] = overlapping.response.ranges[0];
    assert!(Group::from_response(source, 8, overlapping).is_err());
    let mut empty = valid();
    empty.response.ranges[1].end = empty.response.ranges[1].start;
    assert!(Group::from_response(source, 8, empty).is_err());
    let mut surrogate_midpoint = valid();
    surrogate_midpoint.response.ranges[1].start = Position::new(0, 1);
    assert!(Group::from_response(source, 8, surrogate_midpoint).is_err());
    let mut absent_pattern = valid();
    absent_pattern.response.word_pattern = None;
    assert!(Group::from_response(source, 8, absent_pattern).is_err());

    let invalid = "<long></bad name>";
    assert!(
        Group::from_response(
            invalid,
            2,
            proposal(invalid, vec![1..5, 8..16], LinkedEditingAuthority::Semantic)
        )
        .is_err(),
        "each peer must match the provider pattern"
    );
    let oversized = format!("<x></{}>", "y".repeat(64 * 1024 + 1));
    assert!(
        Group::from_response(
            &oversized,
            2,
            proposal(
                &oversized,
                vec![1..2, 5..oversized.len() - 1],
                LinkedEditingAuthority::Semantic
            )
        )
        .is_err(),
        "every initial name has its own quota"
    );
}
