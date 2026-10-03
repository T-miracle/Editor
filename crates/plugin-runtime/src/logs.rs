//! Bounded, process-local plugin logs share record reads and independently acknowledged reminders.
//! Clones retain the same sink across manager windows, background preparation and retired instances.
//! Each plugin keeps 512 history records plus at most one evicted, unacknowledged anomaly per severity.
//! These two reminder representatives do not restore evicted records or their log-tab unread markers.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
    time::SystemTime,
};

mod streams;
pub(crate) use streams::{LineBuffer, drain};

/// Per-plugin history is bounded independently, so a noisy provider cannot evict a peer's records.
pub const RECORDS_PER_PLUGIN: usize = 512;
/// Truncate messages at Unicode scalar boundaries; no retained message exceeds this character count.
pub const MESSAGE_CHAR_LIMIT: usize = 8192;

/// Severity ordering is also the unread indicator precedence; ordinary records never raise reminders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

/// Immutable presentation data; record identity survives independent views of the same log sink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    /// Monotonically increasing identity within this editor run, shared across plugin owners.
    pub id: u64,
    /// Manifest identity captured at the originating operation, never the current UI selection.
    pub plugin: String,
    /// Host receipt time; the ID breaks ties when multiple sources arrive simultaneously.
    pub time: SystemTime,
    /// Ordinary output, warning or error; reading a record never changes its recorded severity.
    pub level: LogLevel,
    /// Host-defined origin such as WASI stdout, a language service or an execution fault.
    pub source: String,
    /// Readable message, bounded by [`MESSAGE_CHAR_LIMIT`].
    pub message: String,
}

#[derive(Debug)]
struct StoredRecord {
    record: LogRecord,
    read: bool,
}

#[derive(Debug, Default)]
struct PluginLogs {
    records: VecDeque<StoredRecord>,
    /// Only Warning/Error enter this map: latest evicted receipt per level, still unread and unconfirmed.
    evicted_anomalies: BTreeMap<LogLevel, LogRecord>,
    /// Opening a summary dismisses that reminder generation without reading undisplayed records.
    reminders_confirmed_through: u64,
}

impl PluginLogs {
    /// Preserve an actionable cause without allowing normal output to erase a pending bottom reminder.
    fn evict_oldest(&mut self) {
        let Some(entry) = self.records.pop_front() else {
            return;
        };
        if !entry.read
            && entry.record.level > LogLevel::Info
            && entry.record.id > self.reminders_confirmed_through
        {
            // FIFO eviction follows receipt IDs. Replacing the same level coalesces older causes,
            // bounding the extra storage to two records regardless of the producer's output volume.
            self.evicted_anomalies
                .insert(entry.record.level, entry.record);
        }
    }

    /// Retained history remains reviewable after acknowledgement; evicted representatives are pending only.
    fn latest_anomaly(&self) -> Option<&LogRecord> {
        self.records
            .iter()
            .map(|entry| &entry.record)
            .filter(|record| record.level > LogLevel::Info)
            .chain(self.evicted_anomalies.values())
            .max_by_key(|record| (record.time, record.id))
    }

    /// Clamp an acknowledgement to receipts already present and release representatives through that boundary.
    fn confirm_through(&mut self, through: u64) -> bool {
        let through = through.min(self.records.back().map_or(0, |entry| entry.record.id));
        let changed = through > self.reminders_confirmed_through;
        self.reminders_confirmed_through = self.reminders_confirmed_through.max(through);
        let previous = self.evicted_anomalies.len();
        self.evicted_anomalies
            .retain(|_, record| record.id > self.reminders_confirmed_through);
        changed || previous != self.evicted_anomalies.len()
    }
}

#[derive(Debug, Default)]
struct State {
    next_id: u64,
    generation: u64,
    plugins: BTreeMap<String, PluginLogs>,
}

/// Cloneable in-memory sink. Dropping every owner clears history; no data is written to disk.
#[derive(Clone, Debug, Default)]
pub struct RuntimeLogs(Arc<Mutex<State>>);

impl RuntimeLogs {
    /// Append a host-attributed message and return its stable ID; no plugin permissions are granted.
    pub fn append(
        &self,
        plugin: &str,
        level: LogLevel,
        source: &str,
        message: impl Into<String>,
    ) -> u64 {
        let message = message.into().chars().take(MESSAGE_CHAR_LIMIT).collect();
        let mut state = self.0.lock().unwrap();
        state.next_id = state
            .next_id
            .checked_add(1)
            .expect("Runtime log ID exhausted");
        let id = state.next_id;
        let history = state.plugins.entry(plugin.into()).or_default();
        history.records.push_back(StoredRecord {
            record: LogRecord {
                id,
                plugin: plugin.into(),
                time: SystemTime::now(),
                level,
                source: source.into(),
                message,
            },
            read: false,
        });
        // History reads retire with their records. Only an unviewed anomaly may survive as one of
        // the two bounded bottom-reminder representatives, never as a restored log-tab entry.
        while history.records.len() > RECORDS_PER_PLUGIN {
            history.evict_oldest();
        }
        state.generation += 1;
        id
    }

    /// Return retained messages in recording order for one plugin, without modifying reads or reminders.
    pub fn records(&self, plugin: &str) -> Vec<LogRecord> {
        self.0
            .lock()
            .unwrap()
            .plugins
            .get(plugin)
            .map(|history| {
                history
                    .records
                    .iter()
                    .map(|entry| entry.record.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Change counter includes incoming records, reads and reminder acknowledgments for native repaint.
    pub fn generation(&self) -> u64 {
        self.0.lock().unwrap().generation
    }

    /// Highest retained unread anomaly for this plugin; ordinary messages return no indicator.
    pub fn unread_severity(&self, plugin: &str) -> Option<LogLevel> {
        self.0
            .lock()
            .unwrap()
            .plugins
            .get(plugin)
            .and_then(|history| {
                anomaly_severity(history.records.iter().filter(|entry| !entry.read))
            })
    }

    /// Read only the listed IDs for this plugin and release a matching evicted reminder representative.
    /// A stale visible-summary callback cannot read a newer representative of the same severity.
    /// Return whether history reads or bottom-reminder state changed.
    pub fn mark_read(&self, plugin: &str, ids: &[u64]) -> bool {
        let mut state = self.0.lock().unwrap();
        let Some(history) = state.plugins.get_mut(plugin) else {
            return false;
        };
        let mut changed = false;
        for entry in &mut history.records {
            if !entry.read && ids.contains(&entry.record.id) {
                entry.read = true;
                changed = true;
            }
        }
        let previous = history.evicted_anomalies.len();
        history
            .evicted_anomalies
            .retain(|_, record| !ids.contains(&record.id));
        changed |= previous != history.evicted_anomalies.len();
        if changed {
            state.generation += 1;
        }
        changed
    }

    /// Viewing a captured full-page boundary reads records through that ID and confirms its reminders.
    /// Concurrent arrivals with later IDs remain unread; another plugin's history is untouched.
    pub fn view_through(&self, plugin: &str, through: u64) -> bool {
        let mut state = self.0.lock().unwrap();
        let Some(history) = state.plugins.get_mut(plugin) else {
            return false;
        };
        let through = through.min(history.records.back().map_or(0, |entry| entry.record.id));
        let mut changed = false;
        for entry in &mut history.records {
            if !entry.read && entry.record.id <= through {
                entry.read = true;
                changed = true;
            }
        }
        changed |= history.confirm_through(through);
        if changed {
            state.generation += 1;
        }
        changed
    }

    /// Latest retained warning/error or pending evicted representative per plugin, newest-first.
    /// Retained records remain reviewable after reads; viewed evicted representatives are released.
    /// Receipt time and ID select the summary independently of highest pending reminder severity.
    pub fn latest_anomalies(&self) -> Vec<LogRecord> {
        let state = self.0.lock().unwrap();
        latest_summaries(&state)
    }

    /// Atomically capture per-plugin receipt checkpoints and latest anomaly summaries for one popup.
    /// The returned record clones survive later eviction or representative replacement. Confirm only
    /// these checkpoints after opening; arrivals beyond them remain eligible for the next reminder.
    pub fn reminder_snapshot(&self) -> (Vec<(String, u64)>, Vec<LogRecord>) {
        let state = self.0.lock().unwrap();
        (reminder_checkpoints(&state), latest_summaries(&state))
    }

    /// Capture current per-plugin ID boundaries before a summary is displayed.
    pub fn reminder_checkpoint(&self) -> Vec<(String, u64)> {
        reminder_checkpoints(&self.0.lock().unwrap())
    }

    /// Confirm only captured reminder generations. Undisplayed records retain their unread state.
    /// A delayed or fabricated future checkpoint cannot suppress an anomaly that has yet to arrive.
    pub fn confirm_reminders(&self, checkpoints: &[(String, u64)]) -> bool {
        let mut state = self.0.lock().unwrap();
        let mut changed = false;
        for (plugin, through) in checkpoints {
            let Some(history) = state.plugins.get_mut(plugin) else {
                continue;
            };
            changed |= history.confirm_through(*through);
        }
        if changed {
            state.generation += 1;
        }
        changed
    }

    /// Highest unread, unconfirmed severity per plugin, including the two bounded evicted representatives.
    /// History eviction can remove a log-tab marker but cannot silently dismiss a bottom reminder.
    pub fn pending_reminders(&self) -> Vec<(String, LogLevel)> {
        self.0
            .lock()
            .unwrap()
            .plugins
            .iter()
            .filter_map(|(plugin, history)| {
                anomaly_severity(history.records.iter().filter(|entry| {
                    !entry.read && entry.record.id > history.reminders_confirmed_through
                }))
                .into_iter()
                .chain(history.evicted_anomalies.keys().copied())
                .max()
                .map(|level| (plugin.clone(), level))
            })
            .collect()
    }
}

/// Capture boundaries while the caller holds the same lock used to clone popup summaries.
fn reminder_checkpoints(state: &State) -> Vec<(String, u64)> {
    state
        .plugins
        .iter()
        .filter_map(|(plugin, history)| {
            history
                .records
                .back()
                .map(|entry| (plugin.clone(), entry.record.id))
        })
        .collect()
}

/// Shared selection keeps standalone queries and atomic popup snapshots consistent.
fn latest_summaries(state: &State) -> Vec<LogRecord> {
    let mut summaries: Vec<_> = state
        .plugins
        .values()
        .filter_map(|history| history.latest_anomaly().cloned())
        .collect();
    summaries.sort_by_key(|record| std::cmp::Reverse((record.time, record.id)));
    summaries
}

/// Normal records affect history only; reminder precedence follows the ordered severity enum.
fn anomaly_severity<'a>(entries: impl Iterator<Item = &'a StoredRecord>) -> Option<LogLevel> {
    entries
        .map(|entry| entry.record.level)
        .filter(|level| *level > LogLevel::Info)
        .max()
}

#[cfg(test)]
mod tests;
