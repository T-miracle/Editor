//! Declarative discovery of runnable targets.
//!
//! A plugin says what it can run and how to recognize it; the host reads the project's files and
//! applies that description. Nothing here knows about any particular language or build system: a
//! provider declares files to look at, where inside them a value lives, and which of those values
//! identify a target, and the host turns that into candidates with stable identities.
//!
//! The vocabulary is deliberately small — sections, keys, and file names — because every additional
//! shape is a promise the host has to keep for providers it has never seen. A provider that needs
//! something richer should say so through the platform's service interfaces rather than grow a
//! language-specific branch in the host.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[cfg(test)]
#[path = "run_targets_tests.rs"]
mod tests;

/// Format of a provider's discovery file. A newer file is refused, not guessed at.
pub const RUN_TARGET_DISCOVERY_VERSION: u32 = 1;
/// Most candidates one provider may offer; the discovery list is a choice, not a file listing.
pub const MAX_DISCOVERED_TARGETS: usize = 256;
/// Longest accepted identifier or label, matching the configuration rules they become.
const MAX_TEXT_BYTES: usize = 4096;

/// A provider's declaration of how to find what it can run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunTargetDiscovery {
    #[serde(default = "current_version")]
    pub version: u32,
    /// Stable identity of this provider's declaration, so a saved configuration can name it.
    pub id: String,
    /// Shown to the user when choosing among providers.
    pub name: String,
    /// The kind of configuration this provider produces, for grouping and for the form's wording.
    pub target_type: String,
    /// Where to look, and what makes a match a target.
    pub rules: Vec<DiscoveryRule>,
}

fn current_version() -> u32 {
    RUN_TARGET_DISCOVERY_VERSION
}

/// One file shape a provider recognizes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryRule {
    /// The file to read, relative to the workspace; `*` matches one path segment.
    pub file: String,
    /// A table that must exist before this rule applies at all, as `a.b`.
    ///
    /// This is how a provider says "only describe this shape when the file declares it", which is
    /// what keeps a general rule from also firing on a file that names its entries explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_present: Option<String>,
    /// Values this rule must find before it offers anything.
    pub fields: Vec<FieldShape>,
    /// One target per match, named from these fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<TargetShape>,
}

/// Where one value lives inside a recognized file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldShape {
    /// Name the host reports this value under, so a form can label it.
    pub name: String,
    /// Table to read, as `a.b`; empty means the file's own root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// The key holding the value.
    pub key: String,
    /// Whether a file without this value is still a candidate.
    #[serde(default)]
    pub optional: bool,
}

/// What one recognized entry contributes to a target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetShape {
    /// Field whose value names the target; it also makes the target's identity stable.
    pub name_from: String,
    /// Program to run for this target, when the provider knows it without a build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_from: Option<String>,
    /// Values copied into the candidate's reported fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
}

/// One runnable target a provider's own files describe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveredTarget {
    /// Identity that stays the same while the target keeps its name in its own file.
    pub id: String,
    /// The provider that offered this target.
    pub provider: String,
    /// The provider's target type, so the form can word itself for this kind.
    pub target_type: String,
    /// Shown in the discovery list.
    pub label: String,
    /// Values the provider asked to report, in the order it declared them.
    pub fields: BTreeMap<String, String>,
    /// The file this target was found in, relative to the workspace.
    pub found_in: String,
}

/// Why a provider's discovery file cannot be used.
#[derive(Debug)]
pub enum DiscoveryError {
    /// The bytes are not a valid declaration.
    Malformed(serde_json::Error),
    /// The declaration is readable but violates a rule.
    Invalid(String),
    /// The file was written by a newer build.
    UnsupportedVersion { found: u32 },
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(error) => {
                write!(formatter, "Run target discovery is unreadable: {error}")
            }
            Self::Invalid(message) => {
                write!(formatter, "Run target discovery is invalid: {message}")
            }
            Self::UnsupportedVersion { found } => write!(
                formatter,
                "Run target discovery is version {found}, newer than this build understands"
            ),
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// A declaration that could not be used, kept with the provider that offered it.
#[derive(Debug)]
pub struct ProviderFailure {
    pub provider: String,
    pub error: String,
}

impl RunTargetDiscovery {
    /// Read a provider's declaration, refusing anything this build cannot honour.
    pub fn from_json(bytes: &[u8]) -> Result<Self, DiscoveryError> {
        let discovery: Self = serde_json::from_slice(bytes).map_err(DiscoveryError::Malformed)?;
        if discovery.version > RUN_TARGET_DISCOVERY_VERSION {
            return Err(DiscoveryError::UnsupportedVersion {
                found: discovery.version,
            });
        }
        discovery.validate()?;
        Ok(discovery)
    }

    /// Every rule is checked before it is used, so a malformed one is reported rather than ignored.
    fn validate(&self) -> Result<(), DiscoveryError> {
        let bounded = |text: &str| !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES;
        if !bounded(&self.id) || !bounded(&self.name) || !bounded(&self.target_type) {
            return Err(DiscoveryError::Invalid(
                "provider id, name and target type must be non-empty and bounded".into(),
            ));
        }
        if self.rules.is_empty() {
            return Err(DiscoveryError::Invalid(
                "a provider must declare at least one rule".into(),
            ));
        }
        for rule in &self.rules {
            if let Some(section) = &rule.when_present
                && !bounded(section)
            {
                return Err(DiscoveryError::Invalid(format!(
                    "rule {} has an unusable guard section",
                    rule.file
                )));
            }
            if !bounded(&rule.file) || Path::new(&rule.file).is_absolute() {
                return Err(DiscoveryError::Invalid(format!(
                    "rule file must be workspace-relative: {}",
                    rule.file
                )));
            }
            let mut names = Vec::new();
            for field in &rule.fields {
                if !bounded(&field.name) || !bounded(&field.key) {
                    return Err(DiscoveryError::Invalid(
                        "a field needs a name and a key".into(),
                    ));
                }
                if let Some(section) = &field.section
                    && !bounded(section)
                {
                    return Err(DiscoveryError::Invalid(format!(
                        "field {} has an unusable section",
                        field.name
                    )));
                }
                names.push(field.name.as_str());
            }
            if names.is_empty() {
                return Err(DiscoveryError::Invalid(format!(
                    "rule {} declares no fields",
                    rule.file
                )));
            }
            for target in &rule.targets {
                // A target named from a field the rule never reads could never be identified.
                if !names.contains(&target.name_from.as_str()) {
                    return Err(DiscoveryError::Invalid(format!(
                        "rule {} names targets from unknown field {}",
                        rule.file, target.name_from
                    )));
                }
                if let Some(program) = &target.program_from
                    && !names.contains(&program.as_str())
                {
                    return Err(DiscoveryError::Invalid(format!(
                        "rule {} takes the program from unknown field {program}",
                        rule.file
                    )));
                }
                for field in &target.fields {
                    if !names.contains(&field.as_str()) {
                        return Err(DiscoveryError::Invalid(format!(
                            "rule {} reports unknown field {field}",
                            rule.file
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// Turn the workspace's own files into the targets this declaration describes.
    ///
    /// `files` are the workspace-relative names the caller is willing to offer, and `read` resolves
    /// one of them to its contents. Both come from the caller, so the provider sees only what the
    /// host chose to show it and this stays a decision about names rather than a second traversal of
    /// the project. A file that does not exist or cannot be read simply offers nothing.
    pub fn discover(
        &self,
        files: &[String],
        read: impl Fn(&str) -> Option<String>,
    ) -> Vec<DiscoveredTarget> {
        let mut targets = Vec::new();
        // Each rule is applied on its own, so two rules reading the same file describe two shapes
        // instead of one rule shadowing the other.
        for rule in &self.rules {
            for path in resolve_pattern(&rule.file, files) {
                let Some(contents) = read(&path) else {
                    continue;
                };
                // The whole file is read as one document, not as a bare value.
                let document: toml::Value = match toml::from_str(&contents) {
                    Ok(document) => document,
                    // A file that is not readable as this shape is not a candidate; a provider is not
                    // told about parse errors it did not ask to report.
                    Err(_) => continue,
                };
                if !rule_applies(rule, &document) {
                    continue;
                }
                let Some(values) = self.values_of(rule, &document) else {
                    continue;
                };
                for target in self.targets_of(rule, &values, &path) {
                    if targets.len() >= MAX_DISCOVERED_TARGETS {
                        return targets;
                    }
                    targets.push(target);
                }
            }
        }
        targets
    }

    /// Read one rule's declared values, or `None` when a required one is missing.
    fn values_of(
        &self,
        rule: &DiscoveryRule,
        document: &toml::Value,
    ) -> Option<BTreeMap<String, String>> {
        let mut values = BTreeMap::new();
        for field in &rule.fields {
            let found =
                value_at(document, field).and_then(|value| value.as_str().map(str::to_owned));
            match found {
                Some(value) => {
                    values.insert(field.name.clone(), value);
                }
                None if field.optional => {}
                None => return None,
            }
        }
        Some(values)
    }

    /// The targets one rule's values describe.
    fn targets_of(
        &self,
        rule: &DiscoveryRule,
        values: &BTreeMap<String, String>,
        path: &str,
    ) -> Vec<DiscoveredTarget> {
        // Without a target shape the file itself is the target, named by its first present field: a
        // provider that runs one thing per file does not have to repeat itself.
        if rule.targets.is_empty() {
            let Some(name) = rule.fields.iter().find_map(|field| values.get(&field.name)) else {
                return Vec::new();
            };
            return vec![self.target(name, values, rule, path)];
        }
        rule.targets
            .iter()
            .filter_map(|shape| {
                let name = values.get(&shape.name_from)?;
                Some(self.target_with(name, values, shape, path))
            })
            .collect()
    }

    fn target(
        &self,
        name: &str,
        values: &BTreeMap<String, String>,
        rule: &DiscoveryRule,
        path: &str,
    ) -> DiscoveredTarget {
        let shape = TargetShape {
            name_from: name.to_owned(),
            program_from: None,
            fields: rule.fields.iter().map(|field| field.name.clone()).collect(),
        };
        self.target_with(name, values, &shape, path)
    }

    fn target_with(
        &self,
        name: &str,
        values: &BTreeMap<String, String>,
        shape: &TargetShape,
        path: &str,
    ) -> DiscoveredTarget {
        let mut fields = BTreeMap::new();
        for field in &shape.fields {
            if let Some(value) = values.get(field) {
                fields.insert(field.clone(), value.clone());
            }
        }
        DiscoveredTarget {
            // Identity is the provider and the target's own name inside its file: a later run of the
            // same discovery offers the same identity, so a saved configuration keeps its link.
            id: format!("{}:{}", self.id, name),
            provider: self.id.clone(),
            target_type: self.target_type.clone(),
            label: name.to_owned(),
            fields,
            found_in: path.to_owned(),
        }
    }
}

/// Resolve one declared pattern against the files the caller offers.
///
/// A pattern without a wildcard names one file directly; otherwise only the offered names are
/// considered, so a rule cannot reach a file the caller did not list.
fn resolve_pattern(pattern: &str, files: &[String]) -> Vec<String> {
    if !pattern.contains('*') {
        return vec![pattern.to_owned()];
    }
    files
        .iter()
        .filter(|path| pattern_matches(pattern, path))
        .cloned()
        .collect()
}

/// Whether one workspace-relative path matches a declared pattern.
///
/// `*` matches within one path segment and `**` matches any number of them, so a rule can name either
/// one directory's files or a whole tree without reaching anything it did not describe.
pub fn pattern_matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let path = path.replace('\\', "/");
    let pattern_segments = pattern.split('/').collect::<Vec<_>>();
    let path_segments = path.split('/').collect::<Vec<_>>();
    matches_segments(&pattern_segments, &path_segments)
}

/// Match one segment list against another, where `**` may consume any number of segments.
///
/// A `*` inside a segment matches any run of characters within that segment, so `*.rs` names the Rust
/// files of one directory, while `**` alone spans directories.
fn matches_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((head, rest)) if *head == "**" => {
            // A tree pattern matches here or at any later depth, so `crates/**/x` finds `crates/x`
            // as well as deeper ones.
            (0..=path.len()).any(|skip| matches_segments(rest, &path[skip..]))
        }
        Some((head, rest)) => match path.split_first() {
            Some((segment, tail)) if segment_matches(head, segment) => matches_segments(rest, tail),
            _ => false,
        },
    }
}

/// Whether one path segment matches one pattern segment, where `*` matches a run of characters.
fn segment_matches(pattern: &str, segment: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == segment;
    }
    let parts = pattern.split('*').collect::<Vec<_>>();
    let mut rest = segment;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if index == 0 {
            // The first part is anchored at the start; everything else may be anywhere after it.
            let Some(tail) = rest.strip_prefix(part) else {
                return false;
            };
            rest = tail;
        } else if index + 1 == parts.len() {
            return rest.ends_with(part);
        } else if let Some(position) = rest.find(part) {
            rest = &rest[position + part.len()..];
        } else {
            return false;
        }
    }
    true
}

/// Whether a rule's guard is satisfied by this file.
fn rule_applies(rule: &DiscoveryRule, document: &toml::Value) -> bool {
    let Some(section) = &rule.when_present else {
        return true;
    };
    let mut current = document;
    for part in section.split('.') {
        match current.get(part) {
            Some(next) => current = next,
            None => return false,
        }
    }
    true
}

/// Read one declared value from a parsed file.
fn value_at<'a>(document: &'a toml::Value, field: &FieldShape) -> Option<&'a toml::Value> {
    let mut current = document;
    if let Some(section) = &field.section {
        for part in section.split('.') {
            current = current.get(part)?;
        }
    }
    current.get(&field.key)
}
