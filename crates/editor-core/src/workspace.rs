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
        self.scan(false)
    }

    /// Directories are scanned separately so empty folders remain visible in the explorer.
    pub fn directories(&self) -> Vec<PathBuf> {
        self.scan(true)
            .into_iter()
            .map(|entry| entry.absolute_path)
            .collect()
    }

    fn scan(&self, directories: bool) -> Vec<WorkspaceFile> {
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

        let mut files = walker
            .build()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_type().is_some_and(|kind| {
                    if directories {
                        kind.is_dir()
                    } else {
                        kind.is_file()
                    }
                })
            })
            .filter_map(|entry| {
                let absolute_path = entry.into_path();
                let relative_path = absolute_path.strip_prefix(&self.root).ok()?.to_path_buf();
                Some(WorkspaceFile {
                    absolute_path,
                    relative_path,
                })
            })
            .collect::<Vec<_>>();

        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        files
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
