//! Workspace discovery owns traversal budgets and never converts a glob into filesystem authority.
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use plugin_protocol::api::{ErrorCode, Failure, FileMatches, FileQuery};
use std::{collections::BTreeSet, io::Read, path::Path, time::Instant};

const MAX_ENTRIES: usize = 50_000;
const MAX_DEPTH: usize = 64;
const MAX_BYTES: usize = 512 * 1024;
const MAX_SKIPPED: usize = 64;
const MAX_IGNORE_FILE_BYTES: usize = 64 * 1024;
const MAX_IGNORE_LINES: usize = 2048;

/// Authorization is checked by the caller; every subsequent path stays below this canonical root.
pub(super) fn find(
    root: &Path,
    query: &FileQuery,
    deadline: Instant,
) -> Result<FileMatches, Failure> {
    if query.include.is_empty() {
        return Err(Failure::new(
            ErrorCode::InvalidRequest,
            "File discovery needs an include glob",
        ));
    }
    if query.max_results == 0
        || query.max_results > 4096
        || query.include.len() + query.exclude.len() > 32
    {
        return Err(limit("File discovery query quota exceeded"));
    }
    let include = compile(&query.include)?;
    let exclude = compile(&query.exclude)?;
    let root = root.canonicalize().map_err(|error| {
        Failure::new(
            if error.kind() == std::io::ErrorKind::NotFound {
                ErrorCode::NotFound
            } else {
                ErrorCode::OperationFailed
            },
            "Workspace root is unavailable",
        )
    })?;
    let mut discovery = Discovery {
        root: &root,
        include,
        exclude,
        deadline,
        max_results: query.max_results as usize,
        visited: 0,
        bytes: 32,
        ignore_bytes: 0,
        ignore_lines: 0,
        paths: BTreeSet::new(),
        skipped: BTreeSet::new(),
    };
    discovery.directory(&root, 0, &mut Vec::new())?;
    discovery.check_time()?;
    Ok(FileMatches {
        paths: discovery.paths.into_iter().collect(),
        skipped: discovery.skipped.into_iter().collect(),
    })
}

/// Globs have portable root-relative syntax; shell expansion and native drive syntax are rejected.
fn compile(patterns: &[String]) -> Result<GlobSet, Failure> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if pattern.len() > 1024 {
            return Err(limit("File discovery glob quota exceeded"));
        }
        if pattern.is_empty()
            || pattern.starts_with('/')
            || pattern.contains(['\\', ':', '\0'])
            || pattern
                .split('/')
                .any(|segment| segment == "." || segment == ".." || segment.is_empty())
        {
            return Err(Failure::new(
                ErrorCode::InvalidPath,
                "Discovery globs must be workspace-relative",
            ));
        }
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(false)
            .build()
            .map_err(|_| Failure::new(ErrorCode::InvalidPath, "Invalid file discovery glob"))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|_| Failure::new(ErrorCode::InvalidPath, "Invalid file discovery glob set"))
}

/// Ignore rules are scoped to their directory; `.ignore` takes precedence over `.gitignore`.
struct IgnoreRules {
    regular: Option<Gitignore>,
    git: Option<Gitignore>,
}

/// Limits cover inspected entries, metadata, ignored entries and encoded results, not just matches.
struct Discovery<'a> {
    root: &'a Path,
    include: GlobSet,
    exclude: GlobSet,
    deadline: Instant,
    max_results: usize,
    visited: usize,
    bytes: usize,
    ignore_bytes: usize,
    ignore_lines: usize,
    paths: BTreeSet<String>,
    skipped: BTreeSet<String>,
}

impl Discovery<'_> {
    /// Each filesystem step is bounded between OS calls by the enclosing guest invocation deadline.
    fn check_time(&self) -> Result<(), Failure> {
        if Instant::now() >= self.deadline {
            return Err(Failure::new(
                ErrorCode::TimedOut,
                "File discovery deadline exceeded",
            ));
        }
        Ok(())
    }

    /// Depth-first traversal retains only one branch and never opens an excluded directory.
    fn directory(
        &mut self,
        directory: &Path,
        depth: usize,
        rules: &mut Vec<IgnoreRules>,
    ) -> Result<(), Failure> {
        self.check_time()?;
        if depth > MAX_DEPTH {
            return Err(limit("File discovery depth quota exceeded"));
        }
        rules.push(IgnoreRules {
            regular: self.ignore_file(directory, ".ignore")?,
            git: self.ignore_file(directory, ".gitignore")?,
        });
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => {
                self.skip(directory)?;
                rules.pop();
                return Ok(());
            }
        };
        for entry in entries {
            self.check_time()?;
            self.visited += 1;
            if self.visited > MAX_ENTRIES {
                return Err(limit("File discovery traversal quota exceeded"));
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    self.skip(directory)?;
                    continue;
                }
            };
            let path = entry.path();
            let Some(relative) = self.relative(&path) else {
                self.skip(directory)?;
                continue;
            };
            // Checking exclusions before metadata also avoids work on intentionally pruned subtrees.
            if self.exclude.is_match(&relative) || self.exclude.is_match(format!("{relative}/")) {
                continue;
            }
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    self.skip(&path)?;
                    continue;
                }
            };
            if linked(&metadata) || ignored(rules, &path, metadata.is_dir()) {
                continue;
            }
            // Canonical containment checks cover platform aliases in addition to ordinary symlinks.
            let canonical = match path.canonicalize() {
                Ok(canonical) if canonical.starts_with(self.root) => canonical,
                _ => {
                    self.skip(&path)?;
                    continue;
                }
            };
            if metadata.is_dir() {
                self.directory(&canonical, depth + 1, rules)?;
            } else if metadata.is_file()
                && self.include.is_match(&relative)
                && !self.paths.contains(&relative)
            {
                if self.paths.len() >= self.max_results {
                    return Err(limit("File discovery result quota exceeded"));
                }
                self.account(&relative)?;
                self.paths.insert(relative);
            }
        }
        rules.pop();
        Ok(())
    }

    /// Read only regular, contained ignore files; parent and global rules carry no workspace authority.
    fn ignore_file(&mut self, directory: &Path, name: &str) -> Result<Option<Gitignore>, Failure> {
        self.check_time()?;
        let path = directory.join(name);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                self.skip(&path)?;
                return Ok(None);
            }
        };
        if linked(&metadata) || !metadata.is_file() {
            return Ok(None);
        }
        if metadata.len() > MAX_IGNORE_FILE_BYTES as u64 {
            return Err(limit("File discovery ignore-file quota exceeded"));
        }
        if !path
            .canonicalize()
            .is_ok_and(|path| path.starts_with(self.root))
        {
            self.skip(&path)?;
            return Ok(None);
        }
        let contents = (|| {
            let mut contents = String::new();
            std::fs::File::open(&path)?
                .take(MAX_IGNORE_FILE_BYTES as u64 + 1)
                .read_to_string(&mut contents)?;
            Ok::<_, std::io::Error>(contents)
        })();
        let contents = match contents {
            Ok(contents) => contents,
            Err(_) => {
                self.skip(&path)?;
                return Ok(None);
            }
        };
        self.ignore_bytes += contents.len();
        if contents.len() > MAX_IGNORE_FILE_BYTES || self.ignore_bytes > MAX_BYTES {
            return Err(limit("File discovery ignore-file quota exceeded"));
        }
        let mut builder = GitignoreBuilder::new(directory);
        for line in contents.lines() {
            self.check_time()?;
            self.ignore_lines += 1;
            if self.ignore_lines > MAX_IGNORE_LINES || line.len() > 1024 {
                return Err(limit("File discovery ignore-rule quota exceeded"));
            }
            if builder.add_line(Some(path.clone()), line).is_err() {
                self.skip(&path)?;
            }
        }
        match builder.build() {
            Ok(ignore) => Ok(Some(ignore)),
            Err(_) => {
                self.skip(&path)?;
                Ok(None)
            }
        }
    }

    /// Unreadable paths are relative diagnostics and consume the same bounded response allocation.
    fn skip(&mut self, path: &Path) -> Result<(), Failure> {
        let relative = self
            .relative(path)
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| ".".into());
        if !self.skipped.contains(&relative) {
            if self.skipped.len() >= MAX_SKIPPED {
                return Err(limit("File discovery skipped-path quota exceeded"));
            }
            self.account(&relative)?;
            self.skipped.insert(relative);
        }
        Ok(())
    }

    /// A path must round-trip through the public UTF-8 contract; lossy names cannot become handles.
    fn relative(&self, path: &Path) -> Option<String> {
        path.strip_prefix(self.root)
            .ok()?
            .to_str()
            .map(|path| path.replace('\\', "/"))
    }

    /// Account JSON escapes, delimiters and both arrays before retaining a discovered path.
    fn account(&mut self, path: &str) -> Result<(), Failure> {
        self.bytes += serde_json::to_vec(path).expect("strings serialize").len() + 1;
        if self.bytes > MAX_BYTES {
            return Err(limit("File discovery response quota exceeded"));
        }
        Ok(())
    }
}

/// The nearest matching rule wins within each ignore family; negations are retained by the parser.
fn ignored(rules: &[IgnoreRules], path: &Path, directory: bool) -> bool {
    for regular in [true, false] {
        for rules in rules.iter().rev() {
            if let Some(matcher) = if regular { &rules.regular } else { &rules.git } {
                let matched = matcher.matched(path, directory);
                if !matched.is_none() {
                    return matched.is_ignore();
                }
            }
        }
    }
    false
}

/// Windows junctions and other reparse points are links too, even if `is_symlink` is false.
fn linked(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// Budget exhaustion never returns a successful, silently truncated list.
fn limit(message: &str) -> Failure {
    Failure::new(ErrorCode::LimitExceeded, message)
}
