//! File information demo uses public native selection and exact-file reads, without workspace grants.
use plugin_protocol::{
    api::{self, Notification, RequestUpdate},
    interaction::{self, Operation, SelectionMode, Severity, Value},
};

/// The plugin owns policy and presentation; the host owns picker, resource identity and cancellation.
#[derive(Default)]
pub(super) struct Demo {
    pending: Option<api::ResourceHandle>,
}

impl Demo {
    /// Select only this flow's commands and updates, leaving unrelated plugin tasks independent.
    pub(super) fn handles(&self, event: &Notification) -> bool {
        matches!(event, Notification::Command { id, .. } if id.starts_with("interaction-file") || id == "interaction-directory" || id == "interaction-save")
            || matches!(event, Notification::Request { handle, .. } if self.pending.as_ref() == Some(handle))
    }

    /// User cancellation never produces a successful empty selection or a broad filesystem root.
    pub(super) fn handle(&mut self, event: Notification) -> String {
        match event {
            Notification::Command { id, .. } => {
                let mode = match id.as_str() {
                    "interaction-directory" => SelectionMode::Directory,
                    "interaction-save" => SelectionMode::Save,
                    _ => SelectionMode::File,
                };
                match interaction::start(
                    Operation::Select {
                        title: "选择目标 / Choose target".into(),
                        mode,
                        multiple: false,
                        suggested_name: (mode == SelectionMode::Save).then(|| "report.txt".into()),
                    },
                    60000,
                ) {
                    Ok(handle) => {
                        self.pending = Some(handle);
                        "等待原生选择 / Waiting for native selection".into()
                    }
                    Err(error) => error.to_string(),
                }
            }
            Notification::Request {
                update:
                    RequestUpdate::Completed {
                        result: Ok(api::EditorValue::Interaction(Value::Selected(resources))),
                    },
                ..
            } => {
                self.pending = None;
                let mut information = Vec::new();
                for resource in resources {
                    let result = if resource.kind == SelectionMode::File {
                        api::guest::read_file(&resource.handle, "")
                            .map(|bytes| format!("{}: {} bytes", resource.name, bytes.len()))
                    } else {
                        Ok(format!("{}: {:?} intent", resource.name, resource.kind))
                    };
                    information.push(result.unwrap_or_else(|error| error.to_string()));
                    let _ = api::guest::close_resource(resource.handle);
                }
                let text = information.join("\n");
                let _ = interaction::start(
                    Operation::Notify {
                        title: "目标信息 / Target information".into(),
                        message: text.clone(),
                        severity: Severity::Information,
                    },
                    60000,
                );
                text
            }
            Notification::Request { update, .. } if update.is_terminal() => {
                self.pending = None;
                serde_json::to_string(&update).unwrap()
            }
            _ => "原生选择处理中 / Native selection pending".into(),
        }
    }
}
