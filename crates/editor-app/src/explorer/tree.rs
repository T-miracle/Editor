//! Builds and locates entries in the workspace explorer tree.

use gpui_kit::component::tree::TreeItem;
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

pub(crate) fn tree_items<'a>(root: &Path, files: impl Iterator<Item = &'a Path>) -> Vec<TreeItem> {
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
    for file in files {
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
            node.file = index + 1 == parts.len();
        }
    }
    into_items(root_node)
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
