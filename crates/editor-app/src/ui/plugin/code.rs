//! Readonly code requests retain only capture ranges; native text and theme remain the renderer's owners.
use super::*;
use crate::language::code_highlighting::{self as language, Selection, Token};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyElement, InteractiveElement as _, ParentElement as _, Styled as _, StyledText, div,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

/// Bound active WASM batches across all plugin views; queued owners check cancellation before acquiring.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Permit {
    fn acquire() -> Option<Self> {
        ACTIVE
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 4).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Request {
    node: String,
    text: String,
    selection: Selection,
}
#[derive(Clone, PartialEq, Eq)]
struct Scene {
    source: plugin_runtime::plugin_protocol::api::DocumentVersion,
    revision: u64,
    requests: Vec<Request>,
}

/// One immutable scene owns one cancellable batch. This is neither a document nor an Undo stack.
#[derive(Default)]
pub(super) struct CodeHighlights {
    scene: Option<Scene>,
    generation: u64,
    tokens: BTreeMap<String, Vec<Token>>,
    cancelled: Option<Arc<AtomicBool>>,
    task: Option<gpui_kit::Task<()>>,
}
impl Drop for CodeHighlights {
    fn drop(&mut self) {
        if let Some(cancelled) = &self.cancelled {
            cancelled.store(true, Ordering::Release);
        }
    }
}

/// Only visible code blocks request work; hidden tabs, disabled branches and modal-covered content stay plain.
fn collect(node: &plugin_runtime::plugin_protocol::ui::Node, requests: &mut Vec<Request>) {
    if node.disabled || requests.len() >= 64 {
        return;
    }
    match &node.kind {
        Kind::CodeBlock {
            text,
            language: Some(name),
        } if !text.is_empty() => {
            if let Some(selection) = language::selected(name) {
                requests.push(Request {
                    node: node.id.clone(),
                    text: text.clone(),
                    selection,
                });
            }
        }
        Kind::Column { children } | Kind::Row { children } => {
            for child in children {
                collect(child, requests);
            }
        }
        Kind::Scroll { content } => collect(content, requests),
        Kind::Tabs { tabs, selected } => {
            if let Some(tab) = tabs.iter().find(|tab| &tab.id == selected) {
                collect(&tab.content, requests);
            }
        }
        _ => {}
    }
}

/// Admit one bounded batch, then evaluate literals on the background executor without owning any GPUI state.
async fn run_batch(
    scene: Scene,
    cancelled: Arc<AtomicBool>,
    background: gpui_kit::BackgroundExecutor,
) -> BTreeMap<String, Vec<Token>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let permit = loop {
        if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
            return BTreeMap::new();
        }
        if let Some(permit) = Permit::acquire() {
            break permit;
        }
        background.timer(Duration::from_millis(16)).await;
    };
    background
        .spawn(async move {
            let _permit = permit;
            let mut result = BTreeMap::new();
            let mut remaining = 16384;
            for request in &scene.requests {
                if remaining == 0 || cancelled.load(Ordering::Acquire) || Instant::now() >= deadline
                {
                    break;
                }
                let mut tokens =
                    language::highlight(&request.selection, &request.text, &cancelled, deadline);
                tokens.truncate(remaining);
                remaining -= tokens.len();
                result.insert(request.node.clone(), tokens);
            }
            result
        })
        .await
}

impl PluginView {
    /// Revoke source/provider results immediately, even when a publication gap retains control focus.
    pub(crate) fn invalidate_code_highlighting(&mut self, cx: &mut Context<Self>) {
        if let Some(cancelled) = self.code_highlights.cancelled.take() {
            cancelled.store(true, Ordering::Release);
        }
        self.code_highlights.generation = self.code_highlights.generation.wrapping_add(1);
        self.code_highlights.scene = None;
        self.code_highlights.tokens.clear();
        self.code_highlights.task = None;
        cx.notify();
    }

    /// A declarative CodeBlock request starts outside paint; equal scenes reuse immutable ranges.
    pub(super) fn sync_code_highlighting(&mut self, cx: &mut Context<Self>) {
        let mut requests = Vec::new();
        let source = self
            .document
            .source
            .clone()
            .filter(|_| self.document.code_highlighting);
        if source.is_some() {
            if let Some(dialog) = &self.document.dialog {
                collect(&dialog.content, &mut requests);
            } else if self.document.menu.is_none() {
                collect(&self.document.root, &mut requests);
                if let Some(toolbar) = &self.document.editor_toolbar {
                    collect(toolbar, &mut requests);
                }
            }
        }
        let scene = source.map(|source| Scene {
            source,
            revision: self.document.revision,
            requests,
        });
        if scene == self.code_highlights.scene {
            return;
        }
        self.invalidate_code_highlighting(cx);
        self.code_highlights.scene = scene.clone();
        let Some(scene) = scene.filter(|scene| !scene.requests.is_empty()) else {
            return;
        };
        let generation = self.code_highlights.generation;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.code_highlights.cancelled = Some(cancelled.clone());
        self.code_highlights.task = Some(cx.spawn(async move |this, cx| {
            let tokens = run_batch(
                scene.clone(),
                cancelled.clone(),
                cx.background_executor().clone(),
            )
            .await;
            let _ = this.update(cx, |view, cx| {
                // The entity carries owner lifetime; the scene and monotonic provider epoch reject ABA and stale text.
                if cancelled.load(Ordering::Acquire)
                    || view.code_highlights.generation != generation
                    || view.code_highlights.scene.as_ref() != Some(&scene)
                    || scene
                        .requests
                        .iter()
                        .any(|request| !language::is_current(&request.selection))
                {
                    return;
                }
                view.code_highlights.tokens = tokens;
                cx.notify();
            });
        }));
    }

    /// Clip UTF-8 captures against the actual line bytes; theme changes recolor without a new document or parser.
    pub(super) fn code_line(
        &self,
        node: &str,
        index: usize,
        line: &str,
        start: usize,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let mut first = None;
        let highlights = self
            .code_highlights
            .tokens
            .get(node)
            .into_iter()
            .flatten()
            .filter_map(|token| {
                let from = token.range.start.max(start);
                let to = token.range.end.min(start + line.len());
                if from >= to
                    || !line.is_char_boundary(from - start)
                    || !line.is_char_boundary(to - start)
                {
                    return None;
                }
                let style = theme.highlight_theme.style.syntax.style(&token.capture)?;
                first.get_or_insert_with(|| token.capture.replace('.', "-"));
                Some((from - start..to - start, style))
            })
            .collect::<Vec<_>>();
        let mut content = div().child(
            StyledText::new(if line.is_empty() { " " } else { line }.to_owned())
                .with_highlights(highlights),
        );
        if let Some(capture) = first {
            let selector = format!("plugin-code-capture-{node}-{index}-{capture}");
            content = content.debug_selector(move || selector.clone());
            let selector = format!("plugin-code-highlight-{node}-{index}");
            return div()
                .debug_selector(move || selector.clone())
                .flex_shrink_0()
                .whitespace_nowrap()
                .child(content)
                .into_any_element();
        }
        div()
            .flex_shrink_0()
            .whitespace_nowrap()
            .child(content)
            .into_any_element()
    }
}
