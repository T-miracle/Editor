//! Builds and locates entries in the workspace explorer tree.

use gpui_base::TreeItem;
use pinyin::ToPinyin;
use std::path::{Path, PathBuf};

pub(crate) fn restore_expanded(items: Vec<TreeItem>, expanded: &[String]) -> Vec<TreeItem> {
    items
        .into_iter()
        .map(|item| {
            let children = restore_expanded(item.children, expanded);
            let id = item.id.to_string();
            TreeItem::new(id.clone(), item.label.clone())
                .children(children)
                .expanded(expanded.iter().any(|path| path == &id))
        })
        .collect()
}

pub(crate) fn find_tree_item<'a>(items: &'a [TreeItem], path: &Path) -> Option<&'a TreeItem> {
    items.iter().find_map(|item| {
        if Path::new(item.id.as_str()) == path {
            Some(item)
        } else {
            find_tree_item(&item.children, path)
        }
    })
}

pub(crate) fn tree_items<'a>(
    root: &Path,
    files: impl Iterator<Item = &'a Path>,
    directories: impl Iterator<Item = &'a Path>,
) -> Vec<TreeItem> {
    #[derive(Default)]
    struct Node {
        path: PathBuf,
        children: std::collections::BTreeMap<String, Node>,
        file: bool,
    }

    fn into_items(node: Node) -> Vec<TreeItem> {
        let mut children = node
            .children
            .into_iter()
            .map(|(label, child)| {
                let is_folder = !child.file;
                let item = if child.file {
                    TreeItem::new(child.path.to_string_lossy().to_string(), label.clone())
                } else {
                    TreeItem::new(child.path.to_string_lossy().to_string(), label.clone())
                        .children(into_items(child))
                };
                (is_folder, tree_sort_key(&label), label.to_lowercase(), item)
            })
            .collect::<Vec<_>>();
        children.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
        });
        children.into_iter().map(|(_, _, _, item)| item).collect()
    }

    let mut root_node = Node {
        path: root.to_path_buf(),
        ..Default::default()
    };
    for (file, is_file) in files
        .map(|path| (path, true))
        .chain(directories.map(|path| (path, false)))
    {
        let Ok(relative) = file.strip_prefix(root) else {
            continue;
        };
        let mut node = &mut root_node;
        let mut current = root.to_path_buf();
        let parts = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        for (index, part) in parts.iter().enumerate() {
            current.push(part);
            node = node.children.entry(part.clone()).or_insert_with(|| Node {
                path: current.clone(),
                ..Default::default()
            });
            if index + 1 == parts.len() {
                node.file = is_file;
            }
        }
    }
    // A single workspace node keeps the project name visible above all of its contents.
    let project_name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    vec![
        TreeItem::new(root.to_string_lossy().into_owned(), project_name)
            .children(into_items(root_node))
            .expanded(true),
    ]
}

fn tree_sort_key(name: &str) -> String {
    name.chars().fold(String::new(), |mut key, character| {
        if let Some(pinyin) = character.to_pinyin() {
            key.push_str(pinyin.first_letter());
        } else {
            key.extend(character.to_lowercase());
        }
        key
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_directory_is_present_in_tree() {
        let root = Path::new("workspace");
        let folder = root.join("empty");
        let items = tree_items(root, std::iter::empty(), std::iter::once(folder.as_path()));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label.as_str(), "workspace");
        assert_eq!(items[0].children[0].label.as_str(), "empty");
        assert!(find_tree_item(&items, &folder).is_some());
    }

    #[test]
    fn project_root_is_visible_even_when_the_workspace_is_empty() {
        let root = Path::new("projects").join("Editor");
        let items = tree_items(&root, std::iter::empty(), std::iter::empty());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label.as_str(), "Editor");
        assert_eq!(Path::new(items[0].id.as_str()), root);
        assert!(items[0].is_expanded());
        assert!(items[0].children.is_empty());
    }
}
