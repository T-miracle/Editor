//! Plugin-owned parameters for named terminal creation and future editor project actions.
use super::*;

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
                    // One FIFO entry per save request keeps simultaneous task names and commands paired.
                    match host(Request::Editor {
                        command: "save".into(),
                    }) {
                        Ok(_) => self.pending_runs.push_back(options),
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            _ => self.command(id, cwd, text),
        }
    }
}
