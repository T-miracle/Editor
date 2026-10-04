//! Where one configuration's program should stop, kept with the configuration that runs it.
//!
//! A breakpoint is a source and a line, which is all a provider needs to resolve it in its own
//! target. Nothing here knows a target language, a debugger or an adapter: a location is a location,
//! and whether it can be bound is the provider's answer, recorded rather than predicted.
use serde::{Deserialize, Serialize};

#[cfg(test)]
#[path = "breakpoints_tests.rs"]
mod tests;

/// Bound on breakpoints one configuration may hold; a list is not an unbounded upload.
pub const MAX_RUN_BREAKPOINTS: usize = 512;
/// Bound on a source reference; long enough for an absolute path, short enough to stay a value.
pub const MAX_BREAKPOINT_SOURCE_BYTES: usize = 4096;

/// One place a configuration's program should stop.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBreakpoint {
    /// Source file, workspace-relative when the user set it from this project.
    pub source: String,
    /// One-based line. A breakpoint on line zero is not a location any provider can bind.
    pub line: u32,
}

/// Why a breakpoint could not be stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BreakpointError {
    EmptySource,
    SourceTooLong {
        bytes: usize,
    },
    /// Line numbers are one-based, so zero is a mistake rather than a location.
    NoSuchLine,
    TooMany {
        count: usize,
    },
    AlreadySet,
}

impl std::fmt::Display for BreakpointError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySource => write!(formatter, "A breakpoint needs a source file"),
            Self::SourceTooLong { bytes } => write!(
                formatter,
                "Breakpoint source is {bytes} bytes, over the {MAX_BREAKPOINT_SOURCE_BYTES} limit"
            ),
            Self::NoSuchLine => write!(formatter, "Line numbers start at 1"),
            Self::TooMany { count } => write!(
                formatter,
                "A configuration holds at most {MAX_RUN_BREAKPOINTS} breakpoints, not {count}"
            ),
            Self::AlreadySet => write!(formatter, "That breakpoint is already set"),
        }
    }
}

impl std::error::Error for BreakpointError {}

/// The breakpoints of one configuration, in the order they are presented.
///
/// Stored as a list rather than as a map keyed by source, so the file stays readable and a hand
/// edit that puts the entries in another order is still valid.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBreakpoints {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entries: Vec<RunBreakpoint>,
}

impl RunBreakpoints {
    pub fn entries(&self) -> &[RunBreakpoint] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether this exact location is set.
    pub fn contains(&self, source: &str, line: u32) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.line == line && same_source(&entry.source, source))
    }

    /// Set one breakpoint, or report why it cannot be stored.
    ///
    /// A location already set is refused rather than duplicated: two entries for one line would be
    /// one breakpoint the provider sets twice and the user has to remove twice.
    pub fn insert(&mut self, source: &str, line: u32) -> Result<(), BreakpointError> {
        validate(source, line)?;
        if self.contains(source, line) {
            return Err(BreakpointError::AlreadySet);
        }
        if self.entries.len() >= MAX_RUN_BREAKPOINTS {
            return Err(BreakpointError::TooMany {
                count: self.entries.len() + 1,
            });
        }
        self.entries.push(RunBreakpoint {
            source: source.to_owned(),
            line,
        });
        self.sort();
        Ok(())
    }

    /// Remove one breakpoint, reporting whether it was set.
    pub fn remove(&mut self, source: &str, line: u32) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|entry| !(entry.line == line && same_source(&entry.source, source)));
        self.entries.len() != before
    }

    /// Remove every breakpoint in one source, which is what closing its file or clearing it means.
    pub fn remove_source(&mut self, source: &str) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|entry| !same_source(&entry.source, source));
        before - self.entries.len()
    }

    /// Keep only the locations a provider acknowledged, preserving the user's own order.
    ///
    /// Used when a configuration moves to another machine or another target: a location that cannot
    /// be bound is dropped rather than left looking set, because a breakpoint that will never hit is
    /// worse than no breakpoint at all.
    pub fn retain_acknowledged(&mut self, acknowledged: &[(String, u32)]) {
        self.entries.retain(|entry| {
            acknowledged
                .iter()
                .any(|(source, line)| *line == entry.line && same_source(source, &entry.source))
        });
    }

    /// Order by source then line, so the list reads like the project rather than like insert order.
    fn sort(&mut self) {
        self.entries.sort();
        self.entries.dedup();
    }

    /// Validate every stored entry, for a file that arrived from somewhere else.
    pub fn validate(&self) -> Result<(), BreakpointError> {
        if self.entries.len() > MAX_RUN_BREAKPOINTS {
            return Err(BreakpointError::TooMany {
                count: self.entries.len(),
            });
        }
        for entry in &self.entries {
            validate(&entry.source, entry.line)?;
        }
        Ok(())
    }
}

fn validate(source: &str, line: u32) -> Result<(), BreakpointError> {
    if source.trim().is_empty() {
        return Err(BreakpointError::EmptySource);
    }
    if source.len() > MAX_BREAKPOINT_SOURCE_BYTES {
        return Err(BreakpointError::SourceTooLong {
            bytes: source.len(),
        });
    }
    if line == 0 {
        return Err(BreakpointError::NoSuchLine);
    }
    Ok(())
}

/// Whether two source references name the same file.
///
/// The comparison is case-insensitive and treats both separators as one, because Windows paths reach
/// the editor with either and a breakpoint set through one spelling has to be removed through it.
fn same_source(left: &str, right: &str) -> bool {
    let normalize = |value: &str| value.replace('\\', "/").to_ascii_lowercase();
    normalize(left) == normalize(right)
}
