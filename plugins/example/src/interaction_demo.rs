//! Guided check demonstrates quick pick, input, confirmation and an independently cancellable task.
use plugin_protocol::{
    api::{self, Notification, RequestUpdate},
    interaction::{self, Operation, PickItem, Value},
};

#[derive(Clone, Copy)]
enum Step {
    Pick,
    Name,
    Confirm,
    Check,
}

/// Each next step consumes the preceding result; a cancelled flow cannot advance on a late reply.
#[derive(Default)]
pub(super) struct Demo {
    pending: Option<(api::ResourceHandle, Step)>,
    label: String,
    choice: String,
}

impl Demo {
    /// This flow cannot consume file-selection or unrelated panel events from another plugin.
    pub(super) fn handles(&self, event: &Notification) -> bool {
        matches!(event, Notification::Command { id, .. } if id == "interaction-check" || id == "interaction-finish-check")
            || matches!(event, Notification::Request { handle, .. } if self.pending.as_ref().is_some_and(|(pending, _)| pending == handle))
    }

    /// Prepare a typed native step; validation errors become readable plugin output.
    fn start(&mut self, operation: Operation, step: Step) -> String {
        match interaction::start(operation, 60000) {
            Ok(handle) => {
                self.pending = Some((handle, step));
                "等待用户 / Waiting for user".into()
            }
            Err(error) => {
                self.pending = None;
                error.to_string()
            }
        }
    }

    /// Business choices remain guest state; the host owns input, focus and cancellation.
    pub(super) fn handle(&mut self, event: Notification) -> String {
        match event {
            Notification::Command { id, .. } if id == "interaction-check" => self.start(
                Operation::QuickPick {
                    title: "检查类型 / Check type".into(),
                    items: vec![
                        PickItem {
                            id: "brief".into(),
                            label: "快速检查 / Quick".into(),
                            description: Some("仅检查名称 / Name only".into()),
                        },
                        PickItem {
                            id: "full".into(),
                            label: "完整检查 / Full".into(),
                            description: Some("检查名称与选项 / Name and option".into()),
                        },
                    ],
                },
                Step::Pick,
            ),
            Notification::Command { .. } => {
                let Some((request, Step::Check)) = self.pending.take() else {
                    return "尚无活动检查 / No active check".into();
                };
                match interaction::finish(request) {
                    Ok(()) => format!(
                        "检查完成 / Check completed: {} ({})",
                        self.label, self.choice
                    ),
                    Err(error) => error.to_string(),
                }
            }
            Notification::Request {
                update:
                    RequestUpdate::Completed {
                        result: Ok(api::EditorValue::Interaction(value)),
                    },
                ..
            } => {
                let Some((_, step)) = self.pending.take() else {
                    return "已结束 / Ended".into();
                };
                match (step, value) {
                    (Step::Pick, Value::Picked(choice)) => { self.choice = choice; self.start(Operation::Input { title: "检查名称 / Check name".into(), value: String::new(), placeholder: Some("输入中文名称 / Enter a name".into()), password: false, max_bytes: 128 }, Step::Name) }
                    (Step::Name, Value::Input(label)) => { self.label = label; self.start(Operation::Confirm { title: "开始检查 / Start check".into(), message: format!("{} ({})", self.label, self.choice) }, Step::Confirm) }
                    (Step::Confirm, Value::Confirmed) => self.start(Operation::Progress { title: "检查进行中 / Checking".into(), message: "已检查名称；可取消或使用“完成检查”命令 / Name checked; cancel or use Finish check".into(), cancellable: true }, Step::Check),
                    (Step::Check, Value::Finished) => "检查完成 / Check completed".into(),
                    _ => "步骤结果无效 / Invalid step result".into(),
                }
            }
            Notification::Request { update, .. } if update.is_terminal() => {
                self.pending = None;
                serde_json::to_string(&update).unwrap()
            }
            Notification::Request { .. } => {
                if let Some((request, Step::Check)) = &self.pending {
                    let _ = interaction::update(
                        request.clone(),
                        "名称检查完成，等待结束 / Name checked; waiting to finish",
                        Some(50),
                    );
                }
                "检查处理中 / Check in progress".into()
            }
            _ => String::new(),
        }
    }
}
