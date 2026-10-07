//! Independent language.structure 1.0: readonly document trees and folds, without an LSP process.
use crate::{
    api::{DocumentVersion, TextRange},
    language::SourceSnapshot,
    settings::Effective,
};
use serde::{Deserialize, Serialize};

/// A package may contribute structure alone; the host selects it separately from other language roles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub language: String,
}

impl Provider {
    /// Opaque contribution and language identifiers are bounded before package activation.
    pub fn valid(&self) -> bool {
        [&self.id, &self.language].into_iter().all(|value| {
            !value.is_empty()
                && value.len() <= 100
                && value.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
                })
        })
    }
}

/// Theme artwork is relative to this provider's own immutable package, never a URL or another package.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Icon {
    pub light: String,
    #[serde(default)]
    pub dark: Option<String>,
}

impl Icon {
    /// The runtime additionally canonicalizes paths and validates bounded geometry-only SVG bytes.
    pub fn valid(&self) -> bool {
        std::iter::once(&self.light)
            .chain(self.dark.iter())
            .all(|path| {
                !path.is_empty()
                    && path.len() <= 256
                    && !path.starts_with('/')
                    && !path.contains(['\\', ':'])
                    && path.split('/').all(|segment| {
                        !segment.is_empty()
                            && segment != "."
                            && segment != ".."
                            && !segment.chars().any(char::is_control)
                    })
            })
    }
}

/// Coverage identifies cursor ownership; definition identifies the precise navigation destination.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub name: String,
    /// Open vocabulary owned by the language plugin, not a host enum.
    pub kind: String,
    #[serde(default)]
    pub icon: Option<Icon>,
    pub range: TextRange,
    pub definition: TextRange,
    #[serde(default)]
    pub children: Vec<Node>,
}

/// An invocation receives only an immutable editor snapshot and resolved settings; no document handle or IO.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub request: u64,
    pub provider: String,
    pub source: SourceSnapshot,
    pub settings: Effective,
}

impl Request {
    /// Source quota and identity apply before any guest instruction runs.
    pub fn valid(&self) -> bool {
        self.request != 0
            && !self.provider.is_empty()
            && self.provider.len() <= 100
            && self.source.valid()
    }
}

/// A complete reply replaces the previous tree; comment folds need not appear among outline nodes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub request: u64,
    pub document: DocumentVersion,
    pub nodes: Vec<Node>,
    pub folds: Vec<TextRange>,
}

impl Proposal {
    /// Validate target/revision, bounded work, UTF-8 boundaries, nested coverage, and fold crossing.
    /// Invalid replies are rejected as a whole; icon load failure alone uses the host fallback artwork.
    pub fn valid_for(&self, request: &Request) -> bool {
        let text = &request.source.text;
        let valid_range = |range: TextRange| {
            range.start < range.end
                && range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end)
        };
        if !request.valid()
            || self.request != request.request
            || self.document != request.source.document
            || self.folds.len() > 4096
            || !self.folds.iter().copied().all(valid_range)
        {
            return false;
        }
        let mut count = 0;
        let mut pending = self
            .nodes
            .iter()
            .rev()
            .map(|node| (node, None, 0))
            .collect::<Vec<_>>();
        while let Some((node, parent, depth)) = pending.pop() {
            count += 1;
            if count > 4096
                || depth >= 128
                || node.name.is_empty()
                || node.name.len() > 512
                || node.kind.is_empty()
                || node.kind.len() > 100
                || node.name.chars().any(char::is_control)
                || node.kind.chars().any(char::is_control)
                || !valid_range(node.range)
                || !valid_range(node.definition)
                || node.definition.start < node.range.start
                || node.definition.end > node.range.end
                || node.icon.as_ref().is_some_and(|icon| !icon.valid())
                || parent.is_some_and(|range: TextRange| {
                    node.range.start < range.start || node.range.end > range.end
                })
            {
                return false;
            }
            if node
                .children
                .windows(2)
                .any(|pair| pair[0].range.end > pair[1].range.start)
            {
                return false;
            }
            pending.extend(
                node.children
                    .iter()
                    .rev()
                    .map(|child| (child, Some(node.range), depth + 1)),
            );
        }
        if self
            .nodes
            .windows(2)
            .any(|pair| pair[0].range.end > pair[1].range.start)
        {
            return false;
        }
        let mut folds = self.folds.clone();
        folds.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
        let mut ends = Vec::new();
        for range in folds {
            while ends.last().is_some_and(|end| *end <= range.start) {
                ends.pop();
            }
            if ends.last().is_some_and(|end| range.end > *end) {
                return false;
            }
            ends.push(range.end);
        }
        serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 512 * 1024)
    }
}
