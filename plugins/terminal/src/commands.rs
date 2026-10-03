//! Plugin-owned parameters for named terminal creation and future editor project actions.
use super::*;

/// Requests keep their own intent until completion; acceptance cannot launch a project command.
pub(super) enum PendingEditor {
    Effect,
    Paste(api::ResourceHandle),
    Selection(api::ResourceHandle),
    Directory,
    BeforeSave(OpenOptions),
    AfterSave(OpenOptions),
}

/// Optional JSON arguments accepted by `terminal.new` and `terminal.run`.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct OpenOptions {
    /// Overrides the tab label; a blank value falls back to the selected shell tool name.
    pub name: Option<String>,
    /// Starts the new shell here instead of the editor workspace directory.
    pub cwd: Option<String>,
    /// Zero-based index into the user's configured shell profiles.
    pub profile: Option<usize>,
    /// Explicit project command; only `terminal.run` executes it after saving the editor file.
    pub command: Option<String>,
}
impl OpenOptions {
    /// Validate a host payload without creating a process or interpreting parameters in the host.
    fn parse(arguments: Option<serde_json::Value>, cwd: Option<String>) -> Result<Self, String> {
        let mut options: Self = match arguments.filter(|value| !value.is_null()) {
            Some(arguments) => serde_json::from_value(arguments)
                .map_err(|error| format!("终端调用参数无效：{error}"))?,
            None => Self::default(),
        };
        if options.cwd.is_none() {
            options.cwd = cwd;
        }
        if options.cwd.as_ref().is_some_and(|cwd| cwd.contains('\0'))
            || options
                .command
                .as_ref()
                .is_some_and(|command| command.trim().is_empty() || command.contains('\0'))
        {
            return Err("终端工作目录不能包含 NUL，运行命令不能为空或包含 NUL".into());
        }
        Ok(options)
    }
}
impl Terminal {
    /// User-triggered asynchronous operations report admission failures in the same terminal view.
    pub(super) fn queue_editor(&mut self, operation: api::EditorOperation, purpose: PendingEditor) {
        if let Err(error) = self.editor_request(operation, purpose) {
            self.error = Some(error);
        }
    }
    /// Structured host invocations join existing guest actions while keeping parameters local.
    pub(super) fn invoke_command(
        &mut self,
        id: &str,
        cwd: Option<String>,
        text: Option<String>,
        arguments: Option<serde_json::Value>,
    ) {
        match id.trim_start_matches("terminal.") {
            "new" | "run" => {
                let options = match OpenOptions::parse(arguments, cwd) {
                    Ok(options) => options,
                    Err(error) => {
                        self.error = Some(error);
                        return;
                    }
                };
                // A corrected invocation replaces a previous parameter error before acquiring resources.
                self.error = None;
                if id.trim_start_matches("terminal.") == "new" {
                    self.add_named(
                        options.profile.unwrap_or(self.settings.default_profile),
                        options.cwd.unwrap_or(self.env.workspace.clone()),
                        options.name,
                    );
                } else {
                    // Capture the active document version before saving; each request retains its own task parameters.
                    match self.editor_request(
                        api::EditorOperation::ReadSelection,
                        PendingEditor::BeforeSave(options),
                    ) {
                        Ok(()) => {}
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            _ => self.command(id, cwd, text),
        }
    }
}

impl Terminal {
    /// Keep opaque request identities, never correlate completion by a string command suffix.
    pub(super) fn editor_request(
        &mut self,
        operation: api::EditorOperation,
        purpose: PendingEditor,
    ) -> Result<(), String> {
        let handle = host::editor(operation)?;
        self.pending_editor.insert(handle.resource, purpose);
        Ok(())
    }

    /// Execute continuation only after an actual success; cancellation or stale revisions never run a command.
    pub(super) fn editor_completion(
        &mut self,
        handle: api::ResourceHandle,
        update: api::RequestUpdate,
    ) {
        if !update.is_terminal() {
            return;
        }
        let Some(purpose) = self.pending_editor.remove(&handle.resource) else {
            return;
        };
        let value = match update {
            api::RequestUpdate::Completed { result: Ok(value) } => value,
            api::RequestUpdate::Completed { result: Err(error) } => {
                self.error = Some(error.to_string());
                return;
            }
            api::RequestUpdate::Cancelled { .. } => {
                self.error = Some("操作已取消，未继续运行项目".into());
                return;
            }
            _ => return,
        };
        let result = match (purpose, value) {
            (PendingEditor::Paste(target), api::EditorValue::Clipboard { text })
            | (PendingEditor::Selection(target), api::EditorValue::Selection { text, .. }) => {
                // A delayed response follows its original process, never the currently selected tab.
                if let Some(tab) = self
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.handle.as_ref() == Some(&target))
                {
                    tab.term.set_scrollback(0);
                    let bytes = input::paste(&text, tab.term.screen().bracketed_paste());
                    host::process(process::Operation::Write {
                        handle: target,
                        bytes,
                    })
                    .map(|_| ())
                } else {
                    Ok(())
                }
            }
            (PendingEditor::Directory, api::EditorValue::Directory { path }) => {
                let cwd = if path.is_empty() {
                    self.env.workspace.clone()
                } else {
                    format!(
                        "{}/{}",
                        self.env.workspace.trim_end_matches(['/', '\\']),
                        path
                    )
                };
                self.add(self.settings.default_profile, cwd);
                Ok(())
            }
            (PendingEditor::BeforeSave(options), api::EditorValue::Selection { document, .. }) => {
                self.editor_request(
                    api::EditorOperation::SaveDocument { document },
                    PendingEditor::AfterSave(options),
                )
            }
            (PendingEditor::AfterSave(options), api::EditorValue::Saved { .. }) => {
                self.run_project(options);
                Ok(())
            }
            (PendingEditor::Effect, _) => Ok(()),
            _ => Err("宿主返回了不匹配的操作结果".into()),
        };
        if let Err(error) = result {
            self.error = Some(error);
        }
    }
}
