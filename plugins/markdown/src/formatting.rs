//! Markdown format and task intents share one correlated read-then-edit owner, never a mutable document or undo stack.

use super::{
    Source,
    format::{self, Command},
    tasks,
};
use plugin_protocol::api;

/// Transient task ownership ends on a newer intent, changed source or terminal host result.
#[derive(Default)]
pub(super) struct Formatting {
    pending: Option<Pending>,
    error: Option<Issue>,
}

struct Pending {
    task: api::guest::EditorTask,
    document: api::DocumentVersion,
    kind: IntentKind,
    phase: Phase,
}

enum Phase {
    Reading(Intent),
    Writing(api::TextRange),
}

enum Intent {
    Format(Command),
    Task(tasks::Change),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum IntentKind {
    Format,
    Task,
}

/// An error belongs to its accepted intent, so a task failure cannot masquerade as a toolbar format result.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Issue {
    kind: IntentKind,
    reason: Reason,
}

/// Stable causes allow localized explanations without parsing host diagnostic strings.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reason {
    SourceChanged,
    InvalidSelection,
    InvalidTask,
    Unavailable,
    Host(api::ErrorCode),
}

impl Formatting {
    /// Replace an older intent and read the exact source document's current selection asynchronously.
    /// Returns whether the visible error changed; acceptance alone does not change the UI revision.
    pub(super) fn start(&mut self, command: Command, source: Option<&Source>) -> bool {
        self.start_intent(Intent::Format(command), source)
    }

    /// A task action already resolved its parsed node; read the native selection before its one-byte edit.
    pub(super) fn start_task(&mut self, change: tasks::Change, source: Option<&Source>) -> bool {
        self.start_intent(Intent::Task(change), source)
    }

    /// One owner lets newer task, toolbar and image intents retire old text writes consistently.
    fn start_intent(&mut self, intent: Intent, source: Option<&Source>) -> bool {
        self.cancel_pending();
        let previous = self.error.take();
        let kind = intent.kind();
        if let Some(source) = source {
            match api::guest::EditorTask::start(
                api::EditorOperation::ReadDocumentSelection {
                    document: source.version.clone(),
                },
                30_000,
            ) {
                Ok(task) => {
                    self.pending = Some(Pending {
                        task,
                        document: source.version.clone(),
                        kind,
                        phase: Phase::Reading(intent),
                    });
                }
                Err(error) => {
                    self.error = Some(Issue {
                        kind,
                        reason: Reason::Host(error.code),
                    })
                }
            }
        } else {
            self.error = Some(Issue {
                kind,
                reason: Reason::Unavailable,
            });
        }
        previous != self.error
    }

    /// A new readonly snapshot retires both stages; it never applies an edit computed from old bytes.
    pub(super) fn source_changed(&mut self) {
        self.cancel_pending();
        self.error = None;
    }

    /// Consume only this intent's request updates; unrelated and cancelled-intent completions are ignored.
    /// The host remains responsible for atomic transactions, revision checks and selection conflicts.
    pub(super) fn request(
        &mut self,
        notification: &api::Notification,
        source: Option<&Source>,
        english: bool,
    ) -> bool {
        let Some(mut pending) = self.pending.take() else {
            return false;
        };
        let Some(update) = pending.task.update(notification) else {
            self.pending = Some(pending);
            return false;
        };
        let previous = self.error;
        match update {
            api::RequestUpdate::Accepted | api::RequestUpdate::Progress { .. } => {
                self.pending = Some(pending);
            }
            api::RequestUpdate::Completed { result } => {
                let kind = pending.kind;
                let completion = result
                    .map_err(|error| Reason::Host(error.code))
                    .and_then(|value| self.complete(pending, value, source, english));
                if let Err(reason) = completion {
                    self.error = Some(Issue { kind, reason });
                }
            }
            api::RequestUpdate::Cancelled { reason, .. } => {
                self.error = Some(Issue {
                    kind: pending.kind,
                    reason: Reason::Host(reason),
                });
            }
        }
        previous != self.error
    }

    /// Translate a bounded domain reason in the current UI locale, including errors from either stage.
    pub(super) fn message(&self, english: bool) -> Option<&'static str> {
        let issue = self.error?;
        if issue.kind == IntentKind::Task {
            return Some(task_message(issue.reason, english));
        }
        let messages = match issue.reason {
            Reason::SourceChanged | Reason::Host(api::ErrorCode::StaleRevision) => (
                "文档已改变，格式操作未完成，请重试。",
                "The document changed before formatting. Please try again.",
            ),
            Reason::InvalidSelection
            | Reason::InvalidTask
            | Reason::Host(api::ErrorCode::InvalidRequest) => (
                "当前选区不能生成有效 Markdown 格式，请调整选区。",
                "The current selection cannot form valid Markdown. Please adjust the selection.",
            ),
            Reason::Unavailable
            | Reason::Host(
                api::ErrorCode::InvalidHandle
                | api::ErrorCode::NotFound
                | api::ErrorCode::InvalidState
                | api::ErrorCode::InvalidPath,
            ) => (
                "源文档已关闭或不可用，无法应用格式。",
                "The source document is closed or unavailable.",
            ),
            Reason::Host(api::ErrorCode::Conflict) => (
                "选区已改变，格式操作未完成，请重试。",
                "The selection changed before formatting. Please try again.",
            ),
            Reason::Host(api::ErrorCode::PermissionDenied) => (
                "没有文档读写权限，无法应用格式。",
                "Formatting requires permission to read and edit the document.",
            ),
            Reason::Host(api::ErrorCode::TimedOut) => (
                "格式操作超时，请重试。",
                "Formatting timed out. Please try again.",
            ),
            Reason::Host(api::ErrorCode::Cancelled) => {
                ("格式操作已取消。", "Formatting was cancelled.")
            }
            Reason::Host(api::ErrorCode::LimitExceeded) => (
                "格式操作超过文档或请求大小限制。",
                "Formatting exceeds the document or request size limits.",
            ),
            Reason::Host(
                api::ErrorCode::UnsupportedOperation | api::ErrorCode::CapabilityUnavailable,
            ) => (
                "当前宿主不支持所需的格式编辑能力。",
                "The host does not support the required formatting capability.",
            ),
            Reason::Host(api::ErrorCode::OperationFailed) => (
                "无法完成格式操作，请重试。",
                "Formatting could not be completed. Please try again.",
            ),
        };
        Some(if english { messages.1 } else { messages.0 })
    }

    /// A completed read starts exactly one replacement; successful edits wait for a fresh Preview snapshot.
    fn complete(
        &mut self,
        pending: Pending,
        value: api::EditorValue,
        source: Option<&Source>,
        english: bool,
    ) -> Result<(), Reason> {
        match (pending.phase, value) {
            (
                Phase::Reading(intent),
                api::EditorValue::DocumentSelection {
                    document,
                    range,
                    text,
                },
            ) => {
                let source = source.ok_or(Reason::Unavailable)?;
                if document != pending.document || source.version != document {
                    return Err(Reason::SourceChanged);
                }
                let selection = range.start..range.end;
                if source.text.get(selection.clone()) != Some(text.as_str()) {
                    return Err(Reason::InvalidSelection);
                }
                let invalid = match pending.kind {
                    IntentKind::Format => Reason::InvalidSelection,
                    IntentKind::Task => Reason::InvalidTask,
                };
                let edit = intent
                    .plan(&source.text, selection, english)
                    .ok_or(invalid)?;
                let resulting_selection = api::TextRange {
                    start: edit.selection.start,
                    end: edit.selection.end,
                };
                let task = api::guest::EditorTask::start(
                    api::EditorOperation::ReplaceDocumentRange {
                        document: document.clone(),
                        range: api::TextRange {
                            start: edit.range.start,
                            end: edit.range.end,
                        },
                        text: edit.text,
                        selection: resulting_selection,
                        expected_selection: Some(range),
                    },
                    30_000,
                )
                .map_err(|error| Reason::Host(error.code))?;
                self.pending = Some(Pending {
                    task,
                    document,
                    kind: pending.kind,
                    phase: Phase::Writing(resulting_selection),
                });
                Ok(())
            }
            (
                Phase::Writing(expected),
                api::EditorValue::Edited {
                    document,
                    selection,
                },
            ) => {
                // Never replace our readonly source with this receipt: only Preview supplies source bytes.
                if document.id != pending.document.id
                    || document.path != pending.document.path
                    || document.revision <= pending.document.revision
                    || selection != expected
                {
                    return Err(Reason::SourceChanged);
                }
                Ok(())
            }
            _ => Err(Reason::Host(api::ErrorCode::OperationFailed)),
        }
    }

    /// Best-effort cancellation retires correlation even if an already-started transaction cannot stop.
    fn cancel_pending(&mut self) {
        if let Some(pending) = self.pending.take() {
            let _ = pending.task.cancel(api::CancelMode::TryTerminate);
        }
    }
}

impl Intent {
    /// Retain the action family after planning, so write-stage errors keep their original explanation.
    fn kind(&self) -> IntentKind {
        match self {
            Self::Format(_) => IntentKind::Format,
            Self::Task(_) => IntentKind::Task,
        }
    }

    /// Both plans consume the same immutable snapshot; task plans preserve the host-read selection exactly.
    fn plan(
        self,
        source: &str,
        selection: std::ops::Range<usize>,
        english: bool,
    ) -> Option<format::Edit> {
        match self {
            Self::Format(command) => format::plan(command, source, selection, english),
            Self::Task(change) => change.plan(source, selection),
        }
    }
}

/// Task feedback describes task writes, while the existing toolbar formatting messages remain unchanged.
fn task_message(reason: Reason, english: bool) -> &'static str {
    let messages = match reason {
        Reason::SourceChanged | Reason::Host(api::ErrorCode::StaleRevision) => (
            "文档已改变，任务未更新，请重试。",
            "The document changed before the task was updated. Please try again.",
        ),
        Reason::InvalidTask | Reason::Host(api::ErrorCode::InvalidRequest) => (
            "当前预览任务已过期或无效，请重试。",
            "The preview task is stale or invalid. Please try again.",
        ),
        Reason::InvalidSelection | Reason::Host(api::ErrorCode::Conflict) => (
            "选区已改变或不可用，任务未更新，请重试。",
            "The selection changed or is unavailable. The task was not updated. Please try again.",
        ),
        Reason::Unavailable
        | Reason::Host(
            api::ErrorCode::InvalidHandle
            | api::ErrorCode::NotFound
            | api::ErrorCode::InvalidState
            | api::ErrorCode::InvalidPath,
        ) => (
            "源文档已关闭或不可用，任务未更新。",
            "The source document is closed or unavailable. The task was not updated.",
        ),
        Reason::Host(api::ErrorCode::PermissionDenied) => (
            "没有文档读写权限，无法更新任务。",
            "Updating a task requires permission to read and edit the document.",
        ),
        Reason::Host(api::ErrorCode::TimedOut) => (
            "任务更新超时，请重试。",
            "Task update timed out. Please try again.",
        ),
        Reason::Host(api::ErrorCode::Cancelled) => {
            ("任务更新已取消。", "Task update was cancelled.")
        }
        Reason::Host(api::ErrorCode::LimitExceeded) => (
            "任务更新超过文档或请求大小限制。",
            "Task update exceeds the document or request size limits.",
        ),
        Reason::Host(
            api::ErrorCode::UnsupportedOperation | api::ErrorCode::CapabilityUnavailable,
        ) => (
            "当前宿主不支持所需的任务编辑能力。",
            "The host does not support the required task-editing capability.",
        ),
        Reason::Host(api::ErrorCode::OperationFailed) => (
            "无法更新任务，请重试。",
            "The task could not be updated. Please try again.",
        ),
    };
    if english { messages.1 } else { messages.0 }
}
