//! Parsed Markdown links own bounded navigation tasks; document text and editor sessions stay in the host.

use super::Source;
use opening::{Arrival, Opening};
use plugin_protocol::{api, ui};
use targets::Destination;

mod opening;
mod targets;
pub(super) use targets::Index;

/// Navigation feedback is independent of formatting and saved-image receipts.
#[derive(Default)]
pub(super) struct Navigation {
    pending: Option<Pending>,
    waiting: Option<Waiting>,
    issue: Option<Issue>,
    /// Preview priority outlives a Unit receipt: native reveal is queued before its actual paint.
    /// This identity contains no geometry or mutable text and is retired by source input or invalidation.
    viewport: Option<api::DocumentVersion>,
}

struct Pending {
    task: api::guest::EditorTask,
    document: api::DocumentVersion,
    phase: Phase,
}

enum Phase {
    Opening {
        fragment: Option<String>,
        transition: Opening,
    },
    Revealing,
    External,
}

/// A terminal Opened receipt may precede Preview. It retains no active host request or source text copy.
struct Waiting {
    origin: api::DocumentVersion,
    document: api::DocumentVersion,
    fragment: String,
}

#[derive(Clone, PartialEq, Eq)]
struct Issue {
    document: api::DocumentVersion,
    reason: Reason,
}

/// Domain causes remain localizable without interpreting a host diagnostic string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reason {
    Unsupported,
    InvalidUri,
    InvalidAnchor,
    SourceChanged,
    Host(api::ErrorCode),
}

impl Navigation {
    /// Only an exact current source version may retain the navigation's preview-first scroll policy.
    /// A successful queued reveal keeps this policy even after its EditorTask has completed.
    pub(super) fn controls_viewport(&self, document: Option<&api::DocumentVersion>) -> bool {
        self.viewport.is_some() && self.viewport.as_ref() == document
    }

    /// A real source translation supersedes the link intent and returns ordinary bidirectional ownership.
    pub(super) fn source_drives(&mut self) {
        if self.viewport.is_some() {
            self.cancel_pending();
        }
    }

    /// A preview translation supersedes a still-pending link while retaining preview-first reflow.
    /// The translation can also be the native reveal itself; neither case needs a guessed completion frame.
    pub(super) fn preview_drives(&mut self) {
        if self.viewport.is_some() {
            self.retire_task();
        }
    }

    /// Only an actual parsed link in this native node can start a user-initiated navigation request.
    /// A successful reveal uses the revision of the view being returned after old feedback is cleared.
    pub(super) fn start(
        &mut self,
        index: &Index,
        nodes: &[ui::Node],
        source: Option<&Source>,
        node: &str,
        uri: &str,
        revision: u64,
    ) -> bool {
        let Some(source) = source else { return false };
        // The event identifies an exact rendered href; only its matching parsed URI chooses the domain target.
        let Some(uri) = index.resolve(nodes, node, uri) else {
            return false;
        };
        self.cancel_pending();
        let previous = self.issue.take();
        let resulting_revision = revision.saturating_add(u64::from(previous.is_some()));
        let result = targets::destination(uri).and_then(|destination| match destination {
            Destination::Anchor(fragment) => {
                self.reveal(&source.version, index, nodes, &fragment, resulting_revision)
            }
            Destination::Relative { path, fragment } => self.begin(
                &source.version,
                api::NavigationTarget::RelativeDocument { path },
                Phase::Opening {
                    fragment,
                    transition: Opening::default(),
                },
            ),
            Destination::External(url) => self.begin(
                &source.version,
                api::NavigationTarget::ExternalUrl { url },
                Phase::External,
            ),
        });
        if let Err(reason) = result {
            self.issue = Some(Issue {
                document: source.version.clone(),
                reason,
            });
        }
        previous != self.issue
    }

    /// Preparation/new instance state cancels all transient navigation ownership and feedback.
    pub(super) fn reset(&mut self) {
        self.cancel_pending();
        self.issue = None;
    }

    /// The first relative-open Preview can precede its receipt. Any further switch, edit or close cancels it.
    /// Waiting receipts accept only their exact opened version, never a path guessed from the clicked URI.
    pub(super) fn source_changed(&mut self, next: Option<&api::DocumentVersion>) {
        let retain = if let Some(pending) = &mut self.pending {
            match &mut pending.phase {
                Phase::Opening { transition, .. } => transition.observe(&pending.document, next),
                _ => false,
            }
        } else {
            self.waiting
                .as_ref()
                .is_some_and(|waiting| next == Some(&waiting.document))
        };
        if !retain {
            self.cancel_pending();
        } else if self.viewport.is_some() {
            // The actual Preview transition, never a filename-derived guess, moves scroll ownership.
            self.viewport = next.cloned();
        }
        self.issue = None;
    }

    /// After the target tree is rebuilt, a previously received Opened identity can locate its actual heading.
    /// No receipt becomes mutable document authority, and unrelated/reopened revisions discard the follow-up.
    pub(super) fn preview(
        &mut self,
        source: Option<&Source>,
        index: &Index,
        nodes: &[ui::Node],
        revision: u64,
    ) -> bool {
        let Some(waiting) = self.waiting.take() else {
            return false;
        };
        let current = source.map(|source| &source.version);
        if current == Some(&waiting.origin) {
            self.waiting = Some(waiting);
            return false;
        }
        if current != Some(&waiting.document) {
            self.viewport = None;
            return false;
        }
        let previous = self.issue.clone();
        if let Err(reason) =
            self.reveal(&waiting.document, index, nodes, &waiting.fragment, revision)
        {
            self.viewport = None;
            self.issue = Some(Issue {
                document: waiting.document,
                reason,
            });
        }
        previous != self.issue
    }

    /// Every owner inspects correlation independently; cancelled or completed older requests cannot trigger a reveal.
    pub(super) fn request(
        &mut self,
        event: &api::Notification,
        source: Option<&Source>,
        index: &Index,
        nodes: &[ui::Node],
        revision: u64,
    ) -> bool {
        let Some(mut pending) = self.pending.take() else {
            return false;
        };
        let Some(update) = pending.task.update(event) else {
            self.pending = Some(pending);
            return false;
        };
        let previous = self.issue.clone();
        match update {
            api::RequestUpdate::Accepted | api::RequestUpdate::Progress { .. } => {
                self.pending = Some(pending)
            }
            api::RequestUpdate::Completed { result } => {
                let result = result
                    .map_err(|error| Issue {
                        document: pending.document.clone(),
                        reason: Reason::Host(error.code),
                    })
                    .and_then(|value| {
                        self.complete(pending, value, source, index, nodes, revision)
                    });
                if let Err(issue) = result {
                    self.viewport = None;
                    self.issue = Some(issue);
                }
            }
            api::RequestUpdate::Cancelled { reason, .. } => {
                self.viewport = None;
                self.issue = Some(Issue {
                    document: pending.document,
                    reason: Reason::Host(reason),
                });
            }
        }
        previous != self.issue
    }

    /// An old document's navigation error cannot replace the current document's independent feedback.
    pub(super) fn message(&self, source: Option<&Source>, english: bool) -> Option<&'static str> {
        let issue = self.issue.as_ref()?;
        let source = source?;
        if source.version.id != issue.document.id || source.version.path != issue.document.path {
            return None;
        }
        let messages = match issue.reason {
            Reason::Unsupported => (
                "仅支持标题锚点、相对 Markdown 文档和 HTTP(S) 网页链接。",
                "Only heading anchors, relative Markdown documents and HTTP(S) web links are supported.",
            ),
            Reason::InvalidUri => (
                "链接地址的百分号编码或 UTF-8 无效。",
                "The link has invalid percent encoding or UTF-8.",
            ),
            Reason::InvalidAnchor => (
                "链接的标题锚点不存在或无效。",
                "The linked heading anchor is missing or invalid.",
            ),
            Reason::SourceChanged
            | Reason::Host(
                api::ErrorCode::StaleRevision
                | api::ErrorCode::Conflict
                | api::ErrorCode::Cancelled,
            ) => (
                "文档或链接已改变，导航已停止，请重试。",
                "The document or link changed. Navigation stopped; please try again.",
            ),
            Reason::Host(api::ErrorCode::PermissionDenied) => (
                "没有导航权限，无法打开链接。",
                "Permission to navigate this link is missing.",
            ),
            Reason::Host(api::ErrorCode::TimedOut) => (
                "链接导航超时，请重试。",
                "Link navigation timed out. Please try again.",
            ),
            Reason::Host(
                api::ErrorCode::NotFound
                | api::ErrorCode::InvalidHandle
                | api::ErrorCode::InvalidState,
            ) => (
                "目标或源文档已关闭或不存在。",
                "The source or target document is closed or missing.",
            ),
            Reason::Host(api::ErrorCode::InvalidPath | api::ErrorCode::InvalidRequest) => (
                "链接路径无效或超出授权工作区。",
                "The link path is invalid or outside the authorized workspace.",
            ),
            Reason::Host(
                api::ErrorCode::UnsupportedOperation | api::ErrorCode::CapabilityUnavailable,
            ) => (
                "当前宿主不支持所需的链接导航能力。",
                "The host does not support the required link-navigation capability.",
            ),
            Reason::Host(api::ErrorCode::LimitExceeded) => (
                "链接导航超过请求大小或资源限制。",
                "Link navigation exceeds the request or resource limits.",
            ),
            Reason::Host(api::ErrorCode::OperationFailed) => (
                "无法打开链接，请重试。",
                "The link could not be opened. Please try again.",
            ),
        };
        Some(if english { messages.1 } else { messages.0 })
    }

    /// Receipt validation chooses the exact currently rendered target before starting a second host effect.
    fn complete(
        &mut self,
        pending: Pending,
        value: api::EditorValue,
        source: Option<&Source>,
        index: &Index,
        nodes: &[ui::Node],
        revision: u64,
    ) -> Result<(), Issue> {
        let failed = |reason| Issue {
            document: pending.document.clone(),
            reason,
        };
        match (pending.phase, value) {
            (
                Phase::Opening {
                    fragment,
                    transition,
                },
                api::EditorValue::Opened { document },
            ) => {
                let arrival = transition
                    .complete(
                        &pending.document,
                        &document,
                        source.map(|source| &source.version),
                    )
                    .map_err(failed)?;
                if let Some(fragment) = fragment {
                    match arrival {
                        Arrival::Ready => self
                            .reveal(&document, index, nodes, &fragment, revision)
                            .map_err(|reason| Issue { document, reason })?,
                        Arrival::Waiting => {
                            self.waiting = Some(Waiting {
                                origin: pending.document,
                                document,
                                fragment,
                            })
                        }
                    }
                }
                Ok(())
            }
            (Phase::Revealing | Phase::External, api::EditorValue::Unit) => {
                if source.is_some_and(|source| source.version == pending.document) {
                    // Unit acknowledges a queued native effect, not its geometry. Preview priority survives it.
                    Ok(())
                } else {
                    Err(failed(Reason::SourceChanged))
                }
            }
            _ => Err(failed(Reason::Host(api::ErrorCode::OperationFailed))),
        }
    }

    /// Reveal only an existing parsed heading node in this source version and published scene revision.
    fn reveal(
        &mut self,
        document: &api::DocumentVersion,
        index: &Index,
        nodes: &[ui::Node],
        fragment: &str,
        revision: u64,
    ) -> Result<(), Reason> {
        let node = index
            .heading(nodes, fragment)
            .ok_or(Reason::InvalidAnchor)?;
        self.begin(
            document,
            api::NavigationTarget::PreviewNode {
                panel: "preview".into(),
                node,
                ui_revision: revision,
            },
            Phase::Revealing,
        )
    }

    /// All native effects are asynchronous and bounded; no parsing, layout or HTML event calls this automatically.
    fn begin(
        &mut self,
        document: &api::DocumentVersion,
        target: api::NavigationTarget,
        phase: Phase,
    ) -> Result<(), Reason> {
        let task = api::guest::EditorTask::start(
            api::EditorOperation::NavigateDocument {
                document: document.clone(),
                target,
            },
            30_000,
        )
        .map_err(|error| Reason::Host(error.code))?;
        if matches!(
            &phase,
            Phase::Opening {
                fragment: Some(_),
                ..
            } | Phase::Revealing
        ) {
            self.viewport = Some(document.clone());
        }
        self.pending = Some(Pending {
            task,
            document: document.clone(),
            phase,
        });
        Ok(())
    }

    /// Cancellation retires request and receipt correlation even when an already-started open cannot be rolled back.
    fn cancel_pending(&mut self) {
        self.viewport = None;
        self.retire_task();
    }

    /// Scroll input can replace receipt correlation while keeping the current preview as the reflow driver.
    fn retire_task(&mut self) {
        self.waiting = None;
        if let Some(pending) = self.pending.take() {
            let _ = pending.task.cancel(api::CancelMode::TryTerminate);
        }
    }
}

#[cfg(test)]
mod link_targets_tests;
#[cfg(test)]
mod scrolling_tests;
#[cfg(test)]
mod tests;
