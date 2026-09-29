//! Validate before allocating native controls; quotas include generated list/table cells.
use super::*;
use std::collections::BTreeSet;

pub(super) fn document(document: &Document) -> Result<(), String> {
    if document.version != VERSION {
        return Err("Unsupported UI protocol version".into());
    }
    let mut validator = Validator {
        ids: BTreeSet::new(),
        count: 0,
        bytes: 0,
    };
    validator.node(&document.root, 0)?;
    if let Some(dialog) = &document.dialog {
        validator.id(&dialog.id)?;
        validator.text(&dialog.title)?;
        dimension(dialog.width, 240., 1200.)?;
        validator.node(&dialog.content, 0)?;
    }
    Ok(())
}

fn dimension(value: f32, min: f32, max: f32) -> Result<(), String> {
    if !value.is_finite() || value < min || value > max {
        Err("Invalid UI dimension/value".into())
    } else {
        Ok(())
    }
}

struct Validator {
    ids: BTreeSet<String>,
    count: usize,
    bytes: usize,
}
impl Validator {
    fn budget(&mut self, count: usize) -> Result<(), String> {
        self.count += count;
        if self.count > 2048 {
            Err("UI node quota exceeded".into())
        } else {
            Ok(())
        }
    }
    fn id(&mut self, id: &str) -> Result<(), String> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            return Err("UI IDs must use 1..128 ASCII letters, digits, '.', '_' or '-'".into());
        }
        if !self.ids.insert(id.into()) {
            return Err(format!("Duplicate UI ID: {id}"));
        }
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<(), String> {
        self.bytes += text.len();
        if text.len() > 65536 || self.bytes > 1024 * 1024 {
            Err("UI text quota exceeded".into())
        } else {
            Ok(())
        }
    }
    fn node(&mut self, node: &Node, depth: usize) -> Result<(), String> {
        if depth > 24 {
            return Err("UI nesting quota exceeded".into());
        }
        self.budget(1)?;
        self.id(&node.id)?;
        if node.role.len() > 128 {
            return Err("UI theme role too long".into());
        }
        let layout = &node.layout;
        for value in [layout.width, layout.height].into_iter().flatten() {
            dimension(value, 0., 10000.)?;
        }
        dimension(layout.gap, 0., 256.)?;
        dimension(layout.padding, 0., 256.)?;
        match &node.kind {
            Kind::Column { children } | Kind::Row { children } => {
                for child in children {
                    self.node(child, depth + 1)?;
                }
            }
            Kind::Scroll { content } => self.node(content, depth + 1)?,
            Kind::Text { text } => self.text(text)?,
            Kind::Button { label } | Kind::Checkbox { label, .. } => self.text(label)?,
            Kind::Input(input) => {
                self.text(&input.value)?;
                self.text(&input.placeholder)?;
            }
            Kind::Choice { options, selected } => {
                self.budget(options.len())?;
                let mut ids = BTreeSet::new();
                for option in options {
                    if option.id.is_empty() || option.id.len() > 128 || !ids.insert(&option.id) {
                        return Err("Invalid/duplicate choice ID".into());
                    }
                    self.text(&option.label)?;
                }
                if selected.as_ref().is_some_and(|id| !ids.contains(id)) {
                    return Err("Unknown choice selection".into());
                }
            }
            Kind::Tabs { tabs, selected } => {
                self.budget(tabs.len())?;
                let mut ids = BTreeSet::new();
                for tab in tabs {
                    if tab.id.is_empty() || tab.id.len() > 128 || !ids.insert(&tab.id) {
                        return Err("Invalid/duplicate tab ID".into());
                    }
                    self.text(&tab.label)?;
                    self.node(&tab.content, depth + 1)?;
                }
                if !ids.contains(selected) {
                    return Err("Unknown tab selection".into());
                }
            }
            Kind::List { items } => {
                self.budget(items.len())?;
                for text in items {
                    self.text(text)?;
                }
            }
            Kind::Table { headers, rows } => {
                if headers.is_empty() || headers.len() > 32 {
                    return Err("Table needs 1..32 columns".into());
                }
                self.budget(headers.len() + rows.len() * headers.len())?;
                for text in headers {
                    self.text(text)?;
                }
                for row in rows {
                    if row.len() != headers.len() {
                        return Err("Table row width differs from headers".into());
                    }
                    for text in row {
                        self.text(text)?;
                    }
                }
            }
            Kind::Progress { label, value } => {
                self.text(label)?;
                dimension(*value, 0., 100.)?;
            }
            Kind::Separator | Kind::Spacer => {}
        }
        Ok(())
    }
}
