//! Extend the native input handler with plugin-provided linked ranges, keeping Base text and history.
use crate::language::{
    navigation::{
        self,
        editing::{LinkedEditingAuthority, LinkedEditingProposal, byte_at_position, merge_edits},
    },
    providers,
};
use crate::*;
use gpui_kit::EntityInputHandler;
mod input_handler;
mod queue;
#[cfg(test)]
mod tests;
use lsp_types::TextEdit;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// Pair metadata is tied to the native entity, a document lease and a content fingerprint.
pub(crate) struct Bridge {
    editor: Entity<EditorState>,
    server: Arc<navigation::LanguageServer>,
    document: navigation::DocumentLease,
    /// Semantic lease revocation does not close the native tab or cancel already-entered commands.
    /// Asynchronous replay refreshes this proof from the existing OpenTab ownership before mutation.
    native_open: bool,
    language: String,
    active: bool,
    generation: u64,
    group: Option<Group>,
    composing: Option<usize>,
    observed: Option<([u8; 32], usize)>,
    in_flight: Option<u64>,
    pending: Option<PendingInput>,
    replaying: bool,
    /// Native history dispatch runs outside entity updates but is still part of the entered sequence.
    history_in_flight: bool,
    history_forwarding: Rc<Cell<bool>>,
    /// Retired input drains into its original entity, then lets the app bind the latest selected service.
    owner: WeakEntity<EditorApp>,
    _observer: Subscription,
}

/// Buffered platform commands preserve a first fast keystroke until semantic ranges arrive.
/// This is a bounded event queue, never a second text buffer or Undo stack.
struct PendingInput {
    source: String,
    selection: Range<usize>,
    commands: Vec<NativeInput>,
    /// Semantic waiting ends at its quota/deadline; the same FIFO then drains into ordinary Base input.
    /// A platform paste can exceed that waiting budget without being truncated or stored in another queue.
    native_only: bool,
}
enum NativeInput {
    Replace(Option<Range<usize>>, String),
    Mark(Option<Range<usize>>, String, Option<Range<usize>>),
    Unmark,
    Bytes(Range<usize>, String),
    Delete(bool),
    Cut,
    History(bool),
}

/// Byte ranges are safe only for their fingerprint; no mutable document copy is retained here.
struct Group {
    ranges: Vec<Range<usize>>,
    pattern: regex::Regex,
    fingerprint: [u8; 32],
}

impl Bridge {
    /// Bind to the exact native session; observers refresh semantic data after cursor/text changes.
    pub(crate) fn new(
        editor: Entity<EditorState>,
        path: PathBuf,
        language: String,
        server: Arc<navigation::LanguageServer>,
        history_forwarding: Rc<Cell<bool>>,
        owner: WeakEntity<EditorApp>,
        cx: &mut Context<Self>,
    ) -> Self {
        let document =
            server.open_document(navigation::file_uri(&path).expect("open native file URI"));
        let observer = cx.observe(&editor, |bridge, _, cx| bridge.refresh(cx));
        let mut bridge = Self {
            editor,
            server,
            document,
            native_open: true,
            language,
            active: true,
            generation: 0,
            group: None,
            composing: None,
            observed: None,
            in_flight: None,
            pending: None,
            replaying: false,
            history_in_flight: false,
            history_forwarding,
            owner,
            _observer: observer,
        };
        bridge.refresh(cx);
        bridge
    }

    /// Replacement or tab activation revokes the old bridge synchronously, before queued work completes.
    pub(crate) fn retire(&mut self) {
        self.active = false;
        self.generation += 1;
        self.group = None;
        self.composing = None;
        self.in_flight = None;
        self.observed = None;
    }
    /// Retired bridges remain reachable until entered user commands have returned to native input.
    pub(crate) fn has_pending(&self) -> bool {
        self.pending.is_some() || self.history_in_flight
    }
    /// Waiting document commands stop when the captured native lifetime has been closed.
    pub(crate) fn document_is_open(&self) -> bool {
        self.native_open
    }

    /// Read only before entering a Bridge update, so a parent EditorApp lease is never reborrowed.
    fn native_owner_is_open(&self, cx: &App) -> bool {
        self.owner.upgrade().is_some_and(|owner| {
            owner
                .read(cx)
                .tabs
                .iter()
                .any(|tab| tab.owns_editor(&self.editor))
        })
    }

    pub(crate) fn matches(
        &self,
        editor: &Entity<EditorState>,
        server: &Arc<navigation::LanguageServer>,
    ) -> bool {
        self.active && self.editor == *editor && Arc::ptr_eq(&self.server, server)
    }

    fn enabled(&self) -> bool {
        self.active
            && self.server.is_active()
            && self.document.is_active()
            && providers::editing_preferences(&self.language).linked_editing
    }

    /// A delayed answer must match the complete native snapshot and current caret before it can link.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        // Native handoff and history finish in this event's deferred effects, without another semantic wait.
        if self.history_in_flight
            || self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.native_only)
        {
            return;
        }
        if !self.enabled() {
            self.group = None;
            self.observed = None;
            self.in_flight = None;
            self.generation += 1;
            return;
        }
        if self.composing.is_some() {
            return;
        }
        let state = self.editor.read(cx);
        let source = state.text().to_string();
        let cursor = state.cursor();
        let fingerprint = digest(&source);
        if self.group.as_ref().is_some_and(|group| {
            group.fingerprint == fingerprint
                && group
                    .ranges
                    .iter()
                    .any(|range| range.start <= cursor && cursor <= range.end)
        }) {
            return;
        }
        if self.observed == Some((fingerprint, cursor)) {
            return;
        }
        self.observed = Some((fingerprint, cursor));
        self.group = None;
        self.generation += 1;
        let generation = self.generation;
        self.in_flight = Some(generation);
        let server = self.server.clone();
        let document = self.document.clone();
        let position = navigation::position_at_byte(&source, cursor);
        cx.spawn(async move |this, cx| {
            let snapshot = source.clone();
            let result = cx
                .background_executor()
                .scheduler_executor()
                .spawn_dedicated(move |_| async move {
                    server.linked_ranges_for(document, snapshot, position)
                })
                .await;
            let _ = this.update(cx, |bridge, cx| {
                if !bridge.enabled()
                    || generation != bridge.generation
                    || bridge.composing.is_some()
                {
                    return;
                }
                bridge.in_flight = None;
                let state = bridge.editor.read(cx);
                if state.cursor() != cursor || digest(&state.text().to_string()) != fingerprint {
                    return;
                }
                bridge.group = result
                    .ok()
                    .flatten()
                    .and_then(|linked| Group::from_response(&source, cursor, linked).ok());
            });
        })
        .detach();
    }

    /// Apply every paired replacement as one Base edit, so one Undo restores every endpoint.
    pub(crate) fn edit_bytes(
        &mut self,
        range: Range<usize>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.queue(NativeInput::Bytes(range.clone(), text.into()), window, cx) {
            return true;
        }
        if !self.enabled() {
            self.group = None;
            return false;
        }
        if !self.editor.read(cx).is_editable() {
            return false;
        }
        let source = self.editor.read(cx).text().to_string();
        let Some(group) = self
            .group
            .as_ref()
            .filter(|group| group.fingerprint == digest(&source))
        else {
            return false;
        };
        let Some(primary) = group
            .ranges
            .iter()
            .position(|name| range.start >= name.start && range.end <= name.end)
        else {
            self.group = None;
            return false;
        };
        let name_range = &group.ranges[primary];
        let mut name = source[name_range.clone()].to_string();
        name.replace_range(
            range.start - name_range.start..range.end - name_range.start,
            text,
        );
        // Empty intermediate names are permitted for deletion, but delimiter edits leave linked mode.
        if !name.is_empty() && !group.pattern.is_match(&name) {
            self.group = None;
            return false;
        }
        let edits = group
            .ranges
            .iter()
            .map(|name_range| {
                TextEdit::new(
                    lsp_types::Range::new(
                        navigation::position_at_byte(&source, name_range.start),
                        navigation::position_at_byte(&source, name_range.end),
                    ),
                    name.clone(),
                )
            })
            .collect::<Vec<_>>();
        let Ok(Some(edit)) = merge_edits(&source, &edits) else {
            return false;
        };
        let Ok(edit) = super::language_edits::native_edit(&source, edit) else {
            return false;
        };
        let before_primary = group.ranges[..primary]
            .iter()
            .map(|range| name.len() as isize - range.len() as isize)
            .sum();
        let cursor = shift(range.start + text.len(), before_primary);
        self.editor.update(cx, |editor, cx| {
            editor.apply_lsp_edits(&vec![edit], window, cx);
            editor.set_selected_range(cursor..cursor, cx);
        });
        let mut offset = 0isize;
        let group = self.group.as_mut().unwrap();
        for name_range in &mut group.ranges {
            let delta = name.len() as isize - name_range.len() as isize;
            let start = shift(name_range.start, offset);
            *name_range = start..start + name.len();
            offset += delta;
        }
        group.fingerprint = digest(&self.editor.read(cx).text().to_string());
        // Deleting the complete name ends semantic authority; do not retain a speculative empty binding.
        if name.is_empty() {
            self.group = None;
        }
        self.observed = None;
        self.composing = None;
        true
    }

    /// UTF-16 platform replacements use the native marked range, exactly as Base does.
    fn replacement(
        &self,
        range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let source = self.editor.read(cx).text().to_string();
        let range = range.or_else(|| {
            self.editor
                .update(cx, |editor, cx| editor.marked_text_range(window, cx))
        });
        match range {
            Some(range) => Some(utf16_byte(&source, range.start)?..utf16_byte(&source, range.end)?),
            None => Some(self.editor.read(cx).selected_range()),
        }
    }

    /// Keep ranges aligned during preedit while peers retain their committed name until IME commits.
    fn align_preedit(
        &mut self,
        before: &str,
        range: Range<usize>,
        primary: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let after = self.editor.read(cx).text().to_string();
        let Some(primary) = primary else {
            self.group = None;
            self.composing = None;
            return;
        };
        let delta = after.len() as isize - before.len() as isize;
        let Some(group) = self.group.as_mut() else {
            return;
        };
        group.ranges[primary].end = shift(group.ranges[primary].end, delta);
        for name in &mut group.ranges[primary + 1..] {
            *name = shift(name.start, delta)..shift(name.end, delta);
        }
        group.fingerprint = digest(&after);
        // An empty preedit has already closed Base's IME transaction; do not synthesize a peer edit.
        self.composing = (!range.is_empty() || delta != 0).then_some(primary);
    }

    /// Keyboard actions mutate Base internally; capture only edits wholly contained in a linked name.
    fn deletion(&mut self, backward: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.queue(NativeInput::Delete(backward), window, cx) {
            return true;
        }
        let state = self.editor.read(cx);
        let source = state.text().to_string();
        let mut range = state.selected_range();
        if range.is_empty() {
            if backward {
                range.start = source[..range.start]
                    .grapheme_indices(true)
                    .next_back()
                    .map_or(0, |(start, _)| start);
            } else if let Some(grapheme) = source[range.end..].graphemes(true).next() {
                range.end += grapheme.len();
            }
        }
        self.edit_bytes(range, "", window, cx)
    }
}

impl Group {
    /// Both methods require safe disjoint names and a caret. Only a negotiated semantic
    /// method may associate different initial texts; standard LSP keeps its equality invariant.
    fn from_response(
        source: &str,
        cursor: usize,
        proposal: LinkedEditingProposal,
    ) -> anyhow::Result<Self> {
        let response = proposal.response;
        anyhow::ensure!(
            (2..=32).contains(&response.ranges.len()),
            "Invalid linked range count"
        );
        let mut ranges = response
            .ranges
            .iter()
            .map(|range| {
                Ok(byte_at_position(source, range.start)?..byte_at_position(source, range.end)?)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        ranges.sort_by_key(|range| range.start);
        // Semantic peers can have different lengths. Check every endpoint before decoding
        // its text, rather than relying on equality with the first range to imply its quota.
        let names = ranges
            .iter()
            .map(|range| {
                anyhow::ensure!(
                    !range.is_empty() && range.len() <= 64 * 1024,
                    "Empty or oversized linked name"
                );
                source
                    .get(range.clone())
                    .ok_or_else(|| anyhow::anyhow!("Invalid linked boundary"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        anyhow::ensure!(
            ranges.windows(2).all(|pair| pair[0].end < pair[1].start)
                && ranges
                    .iter()
                    .any(|range| range.start <= cursor && cursor <= range.end),
            "Uncertain linked ranges"
        );
        anyhow::ensure!(
            proposal.authority == LinkedEditingAuthority::Semantic
                || names.iter().all(|name| *name == names[0]),
            "Standard linked ranges have different initial text"
        );
        let pattern = response
            .word_pattern
            .ok_or_else(|| anyhow::anyhow!("Provider omitted name pattern"))?;
        anyhow::ensure!(pattern.len() <= 4096, "Name pattern quota");
        let pattern = regex::Regex::new(&format!("^(?:{pattern})$"))?;
        anyhow::ensure!(
            names.iter().all(|name| pattern.is_match(name)),
            "Name does not match provider pattern"
        );
        Ok(Self {
            ranges,
            pattern,
            fingerprint: digest(source),
        })
    }
}

fn digest(source: &str) -> [u8; 32] {
    Sha256::digest(source.as_bytes()).into()
}
fn shift(value: usize, delta: isize) -> usize {
    value
        .checked_add_signed(delta)
        .expect("validated range shift")
}
fn utf16_byte(source: &str, offset: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, character) in source.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += character.len_utf16();
    }
    (units == offset).then_some(source.len())
}
