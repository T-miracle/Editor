//! Versioned semantic viewport contracts carry positions without changing text, focus or selection.
use super::{DocumentVersion, ErrorCode, Failure};
use crate::ui::SourceRange;
use serde::{Deserialize, Serialize};

/// Readonly position of the first visible native source caret in the exact owning preview scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceViewport {
    pub document: DocumentVersion,
    pub ui_revision: u64,
    /// UTF-8 byte boundary at the top visual row; never a percentage of the complete document.
    pub offset: usize,
    /// Portion of that visual row above the viewport, in the inclusive range 0..=1.
    pub line_fraction: f32,
    /// A programmatic locate echoes its nonzero origin; manual input has no origin.
    pub origin: Option<u64>,
    /// True for geometry/reflow changes without manual vertical scrolling.
    pub layout: bool,
}

/// Native preview position is measured against one rendered source block and its current height.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewViewport {
    pub block: String,
    pub source_range: SourceRange,
    pub fraction: f32,
    pub origin: Option<u64>,
    pub layout: bool,
}

/// Locate one source caret or one derived preview block, without changing the editor's caret.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewportTarget {
    Source { offset: usize, line_fraction: f32 },
    Preview { node: String, fraction: f32 },
}

impl ViewportTarget {
    /// Shape validation precedes authority and actual UTF-8/scene checks on the owning UI thread.
    pub fn validate(&self) -> Result<(), Failure> {
        let valid = match self {
            Self::Source {
                offset,
                line_fraction,
            } => *offset <= 1024 * 1024 && fraction(*line_fraction),
            Self::Preview {
                node,
                fraction: position,
            } => identity(node) && fraction(*position),
        };
        if valid { Ok(()) } else { Err(invalid()) }
    }
}

impl PreviewViewport {
    /// An event's exact range and active scroll owner are additionally checked against its UI tree.
    pub fn validate(&self) -> Result<(), Failure> {
        if identity(&self.block)
            && fraction(self.fraction)
            && self.source_range.start <= self.source_range.end
            && self.source_range.end <= 1024 * 1024
            && self.origin != Some(0)
        {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}

impl SourceViewport {
    /// The manager additionally matches source/UI identity against this instance's accepted view.
    pub fn validate(&self) -> Result<(), Failure> {
        if self.offset <= 1024 * 1024 && fraction(self.line_fraction) && self.origin != Some(0) {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}

/// All positions are bounded finite normalized geometry, including host-generated notifications.
fn fraction(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
fn identity(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
fn invalid() -> Failure {
    Failure::new(
        ErrorCode::InvalidRequest,
        "Invalid semantic viewport position",
    )
}
