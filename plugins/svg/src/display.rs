//! Image owns editable SVG layouts; raster contexts never bind source-mode preferences.
use super::*;

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Mode {
    Source,
    #[default]
    Split,
    Preview,
}

#[derive(Default)]
pub(super) struct Display {
    pub mode: Mode,
    binding: api::PreferenceBinding,
}
impl Display {
    /// Preserve unknown data and report it; missing intent retains the former split default.
    fn decode(data: Option<&serde_json::Value>) -> Result<Mode, api::Failure> {
        if data.is_some_and(|data| {
            !data.is_object()
                || data
                    .get("version")
                    .is_some_and(|value| value.as_u64() != Some(1))
        }) {
            return Err(api::Failure::new(
                api::ErrorCode::InvalidState,
                "Unknown Image display schema",
            ));
        }
        match data.and_then(|data| data.get("mode")) {
            None => Ok(Mode::Split),
            Some(value) => match value.as_str() {
                Some("source") => Ok(Mode::Source),
                Some("split") => Ok(Mode::Split),
                Some("preview") => Ok(Mode::Preview),
                _ => Err(api::Failure::new(
                    api::ErrorCode::InvalidState,
                    "Unknown Image display preference",
                )),
            },
        }
    }
    /// Only an actual SVG text capability grants a source layout; other image suffixes stay readonly.
    pub(super) fn bind(&mut self, file: Option<&api::FileContext>) -> Result<(), api::Failure> {
        let editable = file.filter(|file| file.file_type == "svg" && file.text.is_some());
        if self.binding.bind(editable.map(|_| api::PreferenceKey {
            file_type: "svg".into(),
            name: "display".into(),
        }))? {
            self.mode = Self::decode(self.binding.value().data.as_ref())?;
            if editable.is_some() && self.binding.value().data.is_none() {
                let imported = api::guest::read_preference(
                    api::PreferenceKey {
                        file_type: "svg".into(),
                        name: "imported-presentation".into(),
                    },
                    false,
                )?;
                if let Some(data) = imported.value.data {
                    self.write(Self::decode(Some(&data))?)?;
                }
            }
        }
        Ok(())
    }
    fn write(&mut self, mode: Mode) -> Result<(), api::Failure> {
        let mode = match mode {
            Mode::Source => "source",
            Mode::Split => "split",
            Mode::Preview => "preview",
        };
        self.mode = Self::decode(
            self.binding
                .write(serde_json::json!({"version":1,"mode":mode}))?
                .data
                .as_ref(),
        )?;
        Ok(())
    }
    /// Toolbar target/revision is checked against the current publication before selecting guest intent.
    pub(super) fn select(&mut self, event: &ui::ToolEvent) -> Result<bool, api::Failure> {
        let mode = match event.tool.as_str() {
            "display-source" => Mode::Source,
            "display-split" => Mode::Split,
            "display-preview" => Mode::Preview,
            _ => return Ok(false),
        };
        self.write(mode)?;
        Ok(true)
    }
    pub(super) fn changed(&mut self, event: &api::Notification) -> Result<bool, api::Failure> {
        if !self.binding.changed(event)? {
            return Ok(false);
        }
        self.mode = Self::decode(self.binding.value().data.as_ref())?;
        Ok(true)
    }
    /// One existing source reference and optional SVG content form the plugin's chosen center tree.
    pub(super) fn compose(
        &self,
        mut document: ui::Document,
        file: &api::FileContext,
    ) -> ui::Document {
        document.editor_layout = true;
        document.file = Some(file.version.clone());
        document.source = file.text.clone();
        let Some(source) = &file.text else {
            return document;
        };
        let editor = ui::Node::new(
            "image-native-editor",
            ui::Kind::NativeEditor {
                document: source.clone(),
            },
        )
        .grow();
        document.root = match self.mode {
            Mode::Source => editor,
            Mode::Split => ui::Node::row("image-layout", vec![editor, document.root])
                .grow()
                .resizable(),
            Mode::Preview => document.root,
        };
        document.tools = [
            ("source", "编辑", "Edit", Mode::Source),
            ("split", "分栏", "Split", Mode::Split),
            ("preview", "预览", "Preview", Mode::Preview),
        ]
        .into_iter()
        .enumerate()
        .map(|(order, (id, zh, en, mode))| {
            let label = ui::LocalizedText {
                zh_cn: zh.into(),
                en: en.into(),
            };
            ui::ToolButton {
                id: format!("display-{id}"),
                label: label.clone(),
                tooltip: label,
                icon: ui::ToolIcon {
                    light: format!("icons/view-{id}.svg"),
                    dark: format!("icons/view-{id}.svg"),
                },
                target: ui::ToolTarget::File {
                    version: file.version.clone(),
                },
                visible: true,
                selected: self.mode == mode,
                disabled: false,
                order: order as i32,
            }
        })
        .collect();
        document
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only known source intent is imported; malformed or newer schemas retain their original data.
    #[test]
    fn display_schema_accepts_legacy_svg_and_rejects_future_intent() {
        assert!(Display::decode(None).unwrap() == Mode::Split);
        assert!(
            Display::decode(Some(&serde_json::json!({"mode":"source"}))).unwrap() == Mode::Source
        );
        for invalid in [
            serde_json::json!(false),
            serde_json::json!({"version":2}),
            serde_json::json!({"mode":"unknown"}),
        ] {
            assert!(Display::decode(Some(&invalid)).is_err());
        }
    }
}
