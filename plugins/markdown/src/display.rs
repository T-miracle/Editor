//! Markdown owns source/split/preview intent, footer functions and its content palette.
use super::*;

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Mode {
    Source,
    #[default]
    Split,
    Preview,
}

#[derive(Clone, Copy)]
pub(super) struct Display {
    pub mode: Mode,
    pub sync: bool,
    pub toolbar: bool,
}
impl Default for Display {
    fn default() -> Self {
        Self {
            mode: Mode::Split,
            sync: true,
            toolbar: true,
        }
    }
}
impl Display {
    /// Unknown records are preserved and reported instead of silently replacing user intent.
    fn decode(data: Option<&serde_json::Value>) -> Result<Self, api::Failure> {
        let Some(data) = data else {
            return Ok(Self::default());
        };
        let invalid = || {
            api::Failure::new(
                api::ErrorCode::InvalidState,
                "Unknown Markdown display preference",
            )
        };
        // Versionless objects are finite legacy imports; future schemas are never rewritten as v1.
        if !data.is_object()
            || data
                .get("version")
                .is_some_and(|value| value.as_u64() != Some(1))
        {
            return Err(invalid());
        }
        let mode = match data.get("mode") {
            None => Mode::Split,
            Some(value) => match value.as_str() {
                Some("source") => Mode::Source,
                Some("split") => Mode::Split,
                Some("preview") => Mode::Preview,
                _ => return Err(invalid()),
            },
        };
        let boolean = |key| match data.get(key) {
            Some(value) => value.as_bool().ok_or_else(invalid),
            None => Ok(true),
        };
        Ok(Self {
            mode,
            sync: boolean("sync")?,
            toolbar: boolean("toolbar")?,
        })
    }
    fn encode(self) -> serde_json::Value {
        serde_json::json!({"version":1,"mode":match self.mode {Mode::Source=>"source",Mode::Split=>"split",Mode::Preview=>"preview"},"sync":self.sync,"toolbar":self.toolbar})
    }
}

impl State {
    /// Both Markdown suffixes share guest intent, while each file's native document remains separate.
    pub(super) fn bind_display(
        &mut self,
        file: Option<api::FileContext>,
    ) -> Result<(), api::Failure> {
        let changed = self
            .preferences
            .bind(file.as_ref().map(|_| api::PreferenceKey {
                file_type: "markdown".into(),
                name: "display".into(),
            }))?;
        if changed {
            self.display = Display::decode(self.preferences.value().data.as_ref())?;
            if self.preferences.value().data.is_none()
                && let Some(file) = &file
            {
                let imported = api::guest::read_preference(
                    api::PreferenceKey {
                        file_type: file.file_type.clone(),
                        name: "imported-presentation".into(),
                    },
                    false,
                )?;
                if let Some(data) = &imported.value.data {
                    let intent = Display::decode(Some(data))?;
                    self.display =
                        Display::decode(self.preferences.write(intent.encode())?.data.as_ref())?;
                }
            }
            self.scrolling.reset();
        }
        // Keep the readonly origin until Preview so an intervening Opened receipt can still
        // correlate its request. Dispatch suppresses pending snapshots; old blocks cannot
        // render under the new file's identity or authorize an edit there.
        if self.source.as_ref().map(|source| &source.version)
            != file.as_ref().and_then(|file| file.text.as_ref())
        {
            self.blocks.clear();
            self.scrolling.reset();
            if file.is_none() {
                self.source = None;
            }
        }
        self.file = file;
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Functions are validated against this exact live file before the guest changes its own schema.
    pub(super) fn display_tool(&mut self, event: ui::ToolEvent) -> Result<(), api::Failure> {
        if event.revision != self.revision
            || self.file.as_ref().is_none_or(|file| {
                event.target
                    != (ui::ToolTarget::File {
                        version: file.version.clone(),
                    })
            })
        {
            return Ok(());
        }
        let mut display = self.display;
        match event.tool.as_str() {
            "display-source" => display.mode = Mode::Source,
            "display-split" => display.mode = Mode::Split,
            "display-preview" => display.mode = Mode::Preview,
            "display-sync" if display.mode == Mode::Split => display.sync = !display.sync,
            "display-toolbar" if display.mode != Mode::Preview => {
                display.toolbar = !display.toolbar
            }
            _ => return Ok(()),
        }
        self.display = Display::decode(self.preferences.write(display.encode())?.data.as_ref())?;
        self.scrolling.reset();
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Geometry references the existing source once; the plugin decides which siblings are mounted.
    pub(super) fn compose_display(&self, mut document: ui::Document) -> ui::Document {
        let Some(file) = &self.file else {
            return document;
        };
        document.file = Some(file.version.clone());
        document.source = file.text.clone();
        document.editor_layout = true;
        if let Some(source) = &file.text {
            let editor = ui::Node::new(
                "markdown-native-editor",
                ui::Kind::NativeEditor {
                    document: source.clone(),
                },
            )
            .grow();
            document.root = match self.display.mode {
                Mode::Source => editor,
                Mode::Split => ui::Node::row("markdown-layout", vec![editor, document.root])
                    .grow()
                    .resizable(),
                Mode::Preview => document.root,
            };
        }
        if self.display.mode != Mode::Split || !self.display.sync {
            document.editor_viewport = None;
        }
        if self.display.mode == Mode::Preview || !self.display.toolbar {
            document.editor_toolbar = None;
        }
        let target = ui::ToolTarget::File {
            version: file.version.clone(),
        };
        document.tools = [
            (
                "source",
                "view-source",
                "编辑",
                "Edit",
                self.display.mode == Mode::Source,
                false,
            ),
            (
                "split",
                "view-split",
                "分栏",
                "Split",
                self.display.mode == Mode::Split,
                false,
            ),
            (
                "preview",
                "view-preview",
                "预览",
                "Preview",
                self.display.mode == Mode::Preview,
                false,
            ),
            (
                "sync",
                "sync-scroll",
                "同步滚动",
                "Synchronized scrolling",
                self.display.sync,
                self.display.mode != Mode::Split,
            ),
            (
                "toolbar",
                "format-toolbar",
                "格式工具栏",
                "Formatting toolbar",
                self.display.toolbar,
                self.display.mode == Mode::Preview,
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(order, (id, icon, zh, en, selected, disabled))| {
            let text = ui::LocalizedText {
                zh_cn: zh.into(),
                en: en.into(),
            };
            ui::ToolButton {
                id: format!("display-{id}"),
                label: text.clone(),
                tooltip: text,
                icon: ui::ToolIcon {
                    light: format!("icons/{icon}.svg"),
                    dark: format!("icons/{icon}.svg"),
                },
                target: target.clone(),
                visible: true,
                selected,
                disabled,
                order: order as i32,
            }
        })
        .collect();
        // Domain content defaults reside here. User tokens override them through ui.content_colors.
        let (fg, bg, border, link) = if self.environment.dark {
            (0xdfe1e5, 0x2b2d30, 0x43454a, 0x7aa5f8)
        } else {
            (0x24292f, 0xffffff, 0xd0d7de, 0x0969da)
        };
        for (key, color) in [
            ("foreground", fg),
            ("background", bg),
            ("border", border),
            ("accent", link),
        ] {
            document
                .content_colors
                .insert(format!("rich_text.{key}"), color);
        }
        document
    }

    /// Scoped notifications alter intent alone and cannot revise the readonly source or undo state.
    pub(super) fn display_changed(
        &mut self,
        event: &api::Notification,
    ) -> Result<(), api::Failure> {
        if self.preferences.changed(event)? {
            self.display = Display::decode(self.preferences.value().data.as_ref())?;
            self.scrolling.reset();
            self.revision = self.revision.saturating_add(1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finite imports keep all known switches; invalid and future records must not be rewritten.
    #[test]
    fn display_schema_preserves_known_legacy_intent_and_rejects_unknown_data() {
        let legacy = serde_json::json!({"mode":"source","sync":false,"toolbar":false});
        let display = Display::decode(Some(&legacy)).unwrap();
        assert!(display.mode == Mode::Source && !display.sync && !display.toolbar);
        assert_eq!(display.encode()["version"], 1);
        for invalid in [
            serde_json::json!(null),
            serde_json::json!({"version":2,"mode":"split"}),
            serde_json::json!({"mode":"future"}),
            serde_json::json!({"sync":"false"}),
        ] {
            assert!(Display::decode(Some(&invalid)).is_err());
        }
    }
}
