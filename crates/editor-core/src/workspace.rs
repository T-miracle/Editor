use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace does not exist: {0}")]
    Missing(PathBuf),
    #[error("workspace is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("failed to resolve workspace {path}: {source}")]
    Resolve {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFile {
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
}

/// One ignore-aware traversal supplies both explorer files and empty directories.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    pub files: Vec<WorkspaceFile>,
    pub directories: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, WorkspaceError> {
        let root = root.into();
        if !root.exists() {
            return Err(WorkspaceError::Missing(root));
        }
        if !root.is_dir() {
            return Err(WorkspaceError::NotDirectory(root));
        }
        let canonical = root
            .canonicalize()
            .map_err(|source| WorkspaceError::Resolve { path: root, source })?;
        Ok(Self { root: canonical })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn files(&self) -> Vec<WorkspaceFile> {
        self.snapshot().files
    }

    /// Empty folders stay visible even when no files are present.
    pub fn directories(&self) -> Vec<PathBuf> {
        self.snapshot().directories
    }

    /// Build an explorer snapshot in a single filesystem walk.
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        let mut walker = WalkBuilder::new(&self.root);
        walker
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            // A folder opened directly is not necessarily a Git repository yet.
            // Treat its .gitignore as a workspace ignore file in either case.
            .add_custom_ignore_filename(".gitignore")
            .filter_entry(|entry| entry.file_name() != ".git" && entry.file_name() != "target");

        let mut snapshot = WorkspaceSnapshot::default();
        for entry in walker.build().filter_map(Result::ok) {
            let Some(kind) = entry.file_type() else {
                continue;
            };
            let absolute_path = entry.into_path();
            if kind.is_dir() {
                snapshot.directories.push(absolute_path);
            } else if kind.is_file()
                && let Ok(relative_path) = absolute_path.strip_prefix(&self.root)
            {
                snapshot.files.push(WorkspaceFile {
                    relative_path: relative_path.to_path_buf(),
                    absolute_path,
                });
            }
        }
        snapshot
            .files
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        snapshot.directories.sort();
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_honors_gitignore_and_target_exclusion() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(directory.path().join("visible.rs"), "fn main() {}").unwrap();
        fs::write(directory.path().join("ignored.txt"), "secret").unwrap();
        fs::create_dir(directory.path().join("target")).unwrap();
        fs::write(directory.path().join("target/generated.rs"), "generated").unwrap();

        let workspace = Workspace::open(directory.path()).unwrap();
        let names = workspace
            .files()
            .into_iter()
            .map(|file| file.relative_path)
            .collect::<Vec<_>>();

        assert!(names.contains(&PathBuf::from("visible.rs")));
        assert!(!names.contains(&PathBuf::from("ignored.txt")));
        assert!(!names.contains(&PathBuf::from("target/generated.rs")));
    }

    #[test]
    fn scan_includes_empty_directories() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("empty")).unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        assert!(
            workspace
                .directories()
                .contains(&workspace.root().join("empty"))
        );
    }
}
