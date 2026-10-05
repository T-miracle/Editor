//! Portable incremental execution observations, bounded independently of native UI rendering.

/// Schema for one immutable observation, shared by provider pulls and host subscriptions.
pub fn event_schema() -> crate::service::Schema {
    serde_json::from_value(serde_json::json!({"type":"record","fields":{
        "sequence":{"type":"integer","min":1,"max":i64::MAX},"kind":{"type":"string","max_bytes":32},
        "stream":{"type":"string","max_bytes":16},"bytes":{"type":"array","max_items":OUTPUT_FRAGMENT_BYTES,"items":{"type":"integer","min":0,"max":255}},
        "state":{"type":"string","max_bytes":32},"code":{"type":"integer","min":0,"max":u32::MAX}},
        "optional":["stream","bytes","state","code"]})).expect("the fixed execution event schema is valid")
}

/// Execution 2.0's input, presentation and incremental observation signatures.
/// Providers retain their own process and UI models while agreeing on literal bytes and ownership.
pub fn observation_methods() -> std::collections::BTreeMap<String, crate::service::Method> {
    use serde_json::json;
    let session = json!({"type":"string","max_bytes":128});
    let state = json!({"type":"string","max_bytes":32});
    let answer = json!({"type":"record","fields":{"session":session,"state":state}});
    let events = json!({"type":"array","max_items":BATCH_EVENTS,"items":event_schema()});
    serde_json::from_value(json!({
        "input":{"parameters":{"type":"record","fields":{"session":session,"bytes":{"type":"array","max_items":1024,"items":{"type":"integer","min":0,"max":255}}}},"result":answer,"permissions":["process.exec"]},
        "locate":{"parameters":{"type":"record","fields":{"session":session}},"result":answer,"permissions":["ui.panels"]},
        "events":{"parameters":{"type":"record","fields":{"session":session,"after":{"type":"integer","min":0,"max":i64::MAX},"limit":{"type":"integer","min":1,"max":BATCH_EVENTS}}},
            "result":{"type":"record","fields":{"session":session,"events":events,"cursor":{"type":"integer","min":0,"max":i64::MAX},"gap":{"type":"boolean"}}},"permissions":["process.exec"]}
    })).expect("fixed execution observation methods are valid")
}
use crate::{
    api::{ErrorCode, Failure},
    process,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A single output fragment fits the service's 64 KiB encoded result bound in a 16-event batch.
pub const OUTPUT_FRAGMENT_BYTES: usize = 512;
/// Retained bytes and events are bounded; lagging observers receive an explicit gap.
pub const RETAINED_EVENTS: usize = 128;
/// Most events one pull may publish, regardless of the caller's polling frequency.
pub const BATCH_EVENTS: usize = 16;

/// Immutable ordered output or lifecycle observation. Bytes retain exact UTF-8/ANSI boundaries.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub sequence: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<process::Stream>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<u32>,
}

/// One bounded incremental response. A gap asks the consumer to mark missing history explicitly.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub session: String,
    pub events: Vec<Event>,
    pub cursor: u64,
    pub gap: bool,
}

/// Providers share transport-neutral bounds, without sharing a mutable terminal or process model.
#[derive(Default)]
pub struct EventBuffer {
    next: u64,
    events: VecDeque<Event>,
}
impl EventBuffer {
    /// Native output is fragmented by byte length; final states retain every valid u32 exit status.
    pub fn observe(&mut self, update: &process::Update) {
        match update {
            process::Update::Output { stream, bytes } => {
                for bytes in bytes.chunks(OUTPUT_FRAGMENT_BYTES) {
                    self.push(Event {
                        sequence: 0,
                        kind: "output".into(),
                        stream: Some(*stream),
                        bytes: Some(bytes.to_vec()),
                        state: None,
                        code: None,
                    });
                }
            }
            process::Update::Exited { code } => self.state("exited", Some(*code)),
            process::Update::Terminated => self.state("terminated", None),
        }
    }
    /// Control admission is observed separately from final program exit.
    pub fn state(&mut self, state: &str, code: Option<u32>) {
        self.push(Event {
            sequence: 0,
            kind: "state".into(),
            stream: None,
            bytes: None,
            state: Some(state.into()),
            code,
        });
    }
    fn push(&mut self, mut event: Event) {
        // The public schema uses signed JSON integers. Exhaustion is practically unreachable and
        // seals history rather than wrapping a cursor onto previously published observations.
        if self.next >= i64::MAX as u64 {
            return;
        }
        self.next += 1;
        event.sequence = self.next;
        if self.events.len() >= RETAINED_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
    /// Return only observations after this cursor; future cursors are rejected instead of hiding data.
    pub fn read(&self, session: String, after: u64, limit: usize) -> Result<Batch, Failure> {
        if after > self.next || !(1..=BATCH_EVENTS).contains(&limit) {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Invalid execution cursor or batch limit",
            ));
        }
        let gap = self
            .events
            .front()
            .is_some_and(|first| first.sequence > after.saturating_add(1));
        let events = self
            .events
            .iter()
            .filter(|event| event.sequence > after)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let cursor = events.last().map_or(after, |event| event.sequence);
        Ok(Batch {
            session,
            events,
            cursor,
            gap,
        })
    }
}
