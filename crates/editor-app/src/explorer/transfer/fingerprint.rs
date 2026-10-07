//! An immutable content tree evolves only from published changes, never from unreviewed later scans.
use super::snapshot::{self, Stamp};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(super) enum Node {
    Missing,
    File(Stamp),
    Directory(BTreeMap<OsString, Node>),
    Link(PathBuf),
}
impl Node {
    /// Child links contribute their identity without following them; a root link remains an invalid target.
    pub fn read(path: &Path) -> Result<Self, String> {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Self::Missing),
            Err(error) => return Err(error.to_string()),
        };
        if snapshot::is_link(&meta) {
            return fs::read_link(path)
                .map(Self::Link)
                .map_err(|error| error.to_string());
        }
        if meta.is_dir() {
            let mut children = BTreeMap::new();
            for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                children.insert(entry.file_name(), Self::read(&entry.path())?);
            }
            return Ok(Self::Directory(children));
        }
        snapshot::stamp(path).map(Self::File)
    }

    /// Use the same content encoding as disk fingerprints, with directory structure held as immutable metadata.
    pub fn stamp(&self) -> Result<Stamp, String> {
        match self {
            Self::Missing => Ok(Stamp::Missing),
            Self::File(stamp) => Ok(stamp.clone()),
            Self::Link(_) => Err(rust_i18n::t!("transfer.link_target").to_string()),
            Self::Directory(children) => {
                let mut digest = Sha256::new();
                for (name, child) in children {
                    digest.update(name.as_encoded_bytes().len().to_le_bytes());
                    digest.update(name.as_encoded_bytes());
                    if let Self::Link(path) = child {
                        digest.update([3]);
                        digest.update(path.as_os_str().as_encoded_bytes());
                        continue;
                    }
                    match child.stamp()? {
                        Stamp::File(hash, readonly) => {
                            digest.update([1, readonly as u8]);
                            digest.update(hash);
                        }
                        Stamp::Directory(hash) => {
                            digest.update([2]);
                            digest.update(hash);
                        }
                        Stamp::Missing => return Err(rust_i18n::t!("transfer.changed").to_string()),
                    }
                }
                Ok(Stamp::Directory(digest.finalize().into()))
            }
        }
    }

    fn at(&self, parts: &[OsString]) -> Option<&Node> {
        if parts.is_empty() {
            return Some(self);
        }
        if let Self::Directory(children) = self {
            children
                .get(&parts[0])
                .and_then(|child| child.at(&parts[1..]))
        } else {
            None
        }
    }
    fn replace(&mut self, parts: &[OsString], value: Node) -> Result<(), String> {
        if parts.is_empty() {
            *self = value;
            return Ok(());
        }
        let Self::Directory(children) = self else {
            return Err(rust_i18n::t!("transfer.changed").to_string());
        };
        if parts.len() == 1 {
            if matches!(value, Self::Missing) {
                children.remove(&parts[0]);
            } else {
                children.insert(parts[0].clone(), value);
            }
            return Ok(());
        }
        children
            .get_mut(&parts[0])
            .ok_or_else(|| rust_i18n::t!("transfer.changed").to_string())?
            .replace(&parts[1..], value)
    }
}

/// Roots are disjoint, so a large replaced directory is inspected once instead of once per child record.
#[derive(Default)]
pub(super) struct Tree(BTreeMap<PathBuf, Node>);
impl Tree {
    pub fn read(paths: impl Iterator<Item = PathBuf>) -> Result<Self, String> {
        let mut paths: Vec<_> = paths.collect();
        paths.sort_by_key(|path| path.components().count());
        let mut roots: BTreeMap<PathBuf, Node> = BTreeMap::new();
        for path in paths {
            if !roots.keys().any(|root| path.starts_with(root)) {
                roots.insert(path.clone(), Node::read(&path)?);
            }
        }
        Ok(Self(roots))
    }
    pub fn stamp(&self, path: &Path) -> Result<Stamp, String> {
        let (root, node) = self
            .0
            .iter()
            .find(|(root, _)| path.starts_with(root))
            .ok_or_else(|| rust_i18n::t!("transfer.changed").to_string())?;
        let parts: Vec<_> = path
            .strip_prefix(root)
            .unwrap()
            .components()
            .map(|part| part.as_os_str().to_owned())
            .collect();
        node.at(&parts).unwrap_or(&Node::Missing).stamp()
    }
    /// Apply a prepared postimage, then derive all ancestor hashes without learning any external modification.
    pub fn replace(&mut self, path: &Path, value: Node) -> Result<(), String> {
        let (root, node) = self
            .0
            .iter_mut()
            .find(|(root, _)| path.starts_with(root.as_path()))
            .ok_or_else(|| rust_i18n::t!("transfer.changed").to_string())?;
        let parts: Vec<_> = path
            .strip_prefix(root)
            .unwrap()
            .components()
            .map(|part| part.as_os_str().to_owned())
            .collect();
        node.replace(&parts, value)
    }

    /// Transfer journals grow incrementally: created parent containers own only their published children.
    pub fn put(&mut self, path: &Path, value: Node) -> Result<(), String> {
        if self.0.keys().any(|root| path.starts_with(root)) {
            return self.replace(path, value);
        }
        self.0.retain(|child, _| !child.starts_with(path));
        self.0.insert(path.to_path_buf(), value);
        Ok(())
    }
}
