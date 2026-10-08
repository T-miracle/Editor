//! Revision deltas reuse validated subtrees without granting authority to a new node or source.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// An unchanged subtree may move in source bytes while retaining native control identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reuse {
    pub id: String,
    pub source_shift: i64,
}

/// Shift only derived source coordinates; controls and link destinations remain unchanged.
pub fn shift_source(node: &mut Node, delta: i64) -> Result<(), String> {
    if let Some(range) = &mut node.source_range {
        range.start = range
            .start
            .checked_add_signed(delta.try_into().map_err(|_| "Invalid shift")?)
            .ok_or("Source shift overflow")?;
        range.end = range
            .end
            .checked_add_signed(delta.try_into().map_err(|_| "Invalid shift")?)
            .ok_or("Source shift overflow")?;
    }
    children_mut(node, &mut |child| shift_source(child, delta))
}

/// Replace unchanged roots with small placeholders. Receivers must restore before normal validation.
pub fn compact(previous: &Document, next: &mut Document) -> Vec<Reuse> {
    let mut old = BTreeMap::new();
    index(&previous.root, &mut old);
    let mut reused = Vec::new();
    fn visit(node: &mut Node, old: &BTreeMap<String, &Node>, reused: &mut Vec<Reuse>) {
        if let Some(before) = old.get(&node.id) {
            let shift = match (before.source_range, node.source_range) {
                (Some(a), Some(b)) => b.start as i64 - a.start as i64,
                _ => 0,
            };
            let mut candidate = (**before).clone();
            if shift_source(&mut candidate, shift).is_ok() && candidate == *node {
                reused.push(Reuse {
                    id: node.id.clone(),
                    source_shift: shift,
                });
                *node = Node::text(node.id.clone(), "");
                return;
            }
        }
        let _ = children_mut(node, &mut |child| {
            visit(child, old, reused);
            Ok(())
        });
    }
    visit(&mut next.root, &old, &mut reused);
    reused
}

/// Restore each reference exactly once from the prior root. Caller checks base revision and authority.
pub fn restore(previous: &Document, next: &mut Document, reused: &[Reuse]) -> Result<(), String> {
    if reused.len() > 2048 {
        return Err("Too many subtree references".into());
    }
    let mut old = BTreeMap::new();
    index(&previous.root, &mut old);
    let mut refs = BTreeMap::new();
    for item in reused {
        if refs.insert(item.id.clone(), item.source_shift).is_some() {
            return Err("Duplicate subtree reference".into());
        }
    }
    let mut used = BTreeSet::new();
    let mut restored_bytes = 0;
    fn visit(
        node: &mut Node,
        old: &BTreeMap<String, &Node>,
        refs: &BTreeMap<String, i64>,
        used: &mut BTreeSet<String>,
        restored_bytes: &mut usize,
    ) -> Result<(), String> {
        if let Some(shift) = refs.get(&node.id) {
            if !used.insert(node.id.clone()) || *node != Node::text(node.id.clone(), "") {
                return Err("Invalid subtree placeholder".into());
            }
            let before = old.get(&node.id).ok_or("Unknown subtree reference")?;
            // Charge expansion before copying: overlapping ancestor references cannot amplify a small payload.
            *restored_bytes += serde_json::to_vec(*before)
                .map_err(|error| error.to_string())?
                .len();
            if *restored_bytes > 2 * 1024 * 1024 {
                return Err("Subtree expansion exceeds UI byte budget".into());
            }
            *node = (**before).clone();
            return shift_source(node, *shift);
        }
        children_mut(node, &mut |child| {
            visit(child, old, refs, used, restored_bytes)
        })
    }
    visit(&mut next.root, &old, &refs, &mut used, &mut restored_bytes)?;
    if used.len() != refs.len() {
        return Err("Unused subtree reference".into());
    }
    Ok(())
}

/// Borrow prior nodes rather than cloning every ancestor subtree while building the lookup table.
fn index<'a>(node: &'a Node, nodes: &mut BTreeMap<String, &'a Node>) {
    nodes.insert(node.id.clone(), node);
    match &node.kind {
        Kind::Column { children } | Kind::Row { children } => {
            for child in children {
                index(child, nodes);
            }
        }
        Kind::Scroll { content } => index(content, nodes),
        Kind::Tabs { tabs, .. } => {
            for tab in tabs {
                index(&tab.content, nodes);
            }
        }
        _ => {}
    }
}

/// Every container kind participates, including inactive tabs whose state must remain owned.
fn children_mut(
    node: &mut Node,
    visit: &mut impl FnMut(&mut Node) -> Result<(), String>,
) -> Result<(), String> {
    match &mut node.kind {
        Kind::Column { children } | Kind::Row { children } => {
            for child in children {
                visit(child)?;
            }
        }
        Kind::Scroll { content } => visit(content)?,
        Kind::Tabs { tabs, .. } => {
            for tab in tabs {
                visit(&mut tab.content)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Rebuilt guests that do not negotiate deltas keep their original Output wire shape.
    #[test]
    fn unused_incremental_capability_does_not_change_output_wire_shape() {
        let output = serde_json::to_value(crate::api::Output::default()).unwrap();
        assert!(output.get("view_patches").is_none());
    }
    /// A changed paragraph travels normally while a later paragraph reuses its shifted source mapping.
    #[test]
    fn delta_round_trip_and_invalid_references() {
        let old = Document::new(Node::column(
            "body",
            vec![
                Node::text("a", "first").source_range(0..5),
                Node::text("b", "last").source_range(7..11),
            ],
        ))
        .revision(4);
        let next = Document::new(Node::column(
            "body",
            vec![
                Node::text("a", "first中文").source_range(0..11),
                Node::text("b", "last").source_range(13..17),
            ],
        ))
        .revision(5);
        let mut wire = next.clone();
        let refs = compact(&old, &mut wire);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_shift, 6);
        restore(&old, &mut wire, &refs).unwrap();
        assert_eq!(wire, next);
        let mut wire = next.clone();
        assert!(
            restore(
                &old,
                &mut wire,
                &[Reuse {
                    id: "missing".into(),
                    source_shift: 0
                }]
            )
            .is_err()
        );
        let mut wire = next.clone();
        assert!(
            restore(
                &old,
                &mut wire,
                &[Reuse {
                    id: "b".into(),
                    source_shift: -100
                }]
            )
            .is_err()
        );
    }

    /// Invalid references never supply unchecked geometry or multiply the admitted scene's memory budget.
    #[test]
    fn delta_rejects_duplicates_overflow_and_expansion_budget() {
        let old = Document::new(Node::text("a", "known").source_range(0..5));
        let item = Reuse {
            id: "a".into(),
            source_shift: 0,
        };
        let placeholder = Document::new(Node::text("a", ""));
        assert!(restore(&old, &mut placeholder.clone(), &[item.clone(), item]).is_err());
        assert!(
            restore(
                &old,
                &mut placeholder.clone(),
                &[Reuse {
                    id: "a".into(),
                    source_shift: -1
                }]
            )
            .is_err()
        );
        let large = Document::new(Node::text("a", "x".repeat(2 * 1024 * 1024)));
        assert!(
            restore(
                &large,
                &mut placeholder.clone(),
                &[Reuse {
                    id: "a".into(),
                    source_shift: 0
                }]
            )
            .is_err()
        );
        let unknown = Document::new(Node::text("missing", ""));
        assert!(
            restore(
                &old,
                &mut unknown.clone(),
                &[Reuse {
                    id: "missing".into(),
                    source_shift: 0
                }]
            )
            .is_err()
        );
    }
}
