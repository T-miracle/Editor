//! Bounded observation and stop policy for a host preparation's delegated native resources.
//! The root is authenticated by the broker context; neither guests nor configuration strings can
//! subscribe to another invocation's processes. This registry interprets no language/tool output.
use plugin_protocol::{
    api::ResourceHandle,
    process::{ExitMode, Update},
};
#[cfg(test)]
#[path = "native_work_tests.rs"]
mod tests;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, Weak, atomic::AtomicBool},
    time::{Duration, Instant},
};

/// The host may render one preparation's raw output and its actual native lifecycle independently.
#[derive(Clone, Debug)]
pub struct PreparationSnapshot {
    pub output: String,
    pub state: crate::ExecutionState,
    /// Immutable actual provider identity, filled by TargetRequest rather than a configuration.
    pub provider: String,
}
#[derive(Debug)]
struct Stop {
    mode: ExitMode,
    since: Instant,
    sent: BTreeMap<(String, String, u64), ExitMode>,
}
/// A single invocation owns this history; output is bounded even if a compiler is noisy.
#[derive(Debug, Default)]
pub(crate) struct NativeWork {
    output: String,
    /// Each owned process/stream retains at most three incomplete UTF-8 bytes.
    utf8: BTreeMap<((String, String, u64), u8), Vec<u8>>,
    active: BTreeSet<(String, String, u64)>,
    stop: Option<Stop>,
    /// A graceful debugger disconnect seals allocations without sending stdin/PTY exit controls.
    sealed: bool,
    /// Failed native observation is terminal failure, never an invented successful cleanup.
    failure: Option<String>,
}
impl NativeWork {
    pub(crate) fn snapshot(&self) -> PreparationSnapshot {
        PreparationSnapshot {
            provider: String::new(),
            output: self.output.clone(),
            state: if self.failure.is_some() {
                crate::ExecutionState::Failed
            } else {
                match &self.stop {
                    Some(_) if self.active.is_empty() => crate::ExecutionState::Exited,
                    Some(stop) => {
                        if stop.mode == ExitMode::Force {
                            crate::ExecutionState::Terminating
                        } else {
                            crate::ExecutionState::Stopping
                        }
                    }
                    _ => {
                        if self.active.is_empty() {
                            crate::ExecutionState::Starting
                        } else {
                            crate::ExecutionState::Running
                        }
                    }
                }
            },
        }
    }
    /// Kept separately from active ownership: failed cleanup does not claim the tree exited.
    pub(crate) fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }
    pub(crate) fn stop(&mut self, mode: ExitMode) {
        if self.stop.is_none() || mode == ExitMode::Force {
            self.stop = Some(Stop {
                mode,
                since: Instant::now(),
                sent: BTreeMap::new(),
            });
        }
    }
    pub(crate) fn stopped(&self) -> bool {
        self.stop.is_some()
    }
    pub(crate) fn seal(&mut self) {
        self.sealed = true;
    }
    pub(crate) fn drained(&self) -> bool {
        self.active.is_empty()
    }
    fn control(&mut self, handle: &ResourceHandle) -> Option<ExitMode> {
        let stop = self.stop.as_mut()?;
        if stop.mode == ExitMode::Graceful
            && stop.since.elapsed() >= Duration::from_millis(crate::DEFAULT_STOP_GRACE_MS.into())
        {
            stop.mode = ExitMode::Force;
        }
        if stop.sent.get(&key(handle)) == Some(&stop.mode) {
            return None;
        }
        stop.sent.insert(key(handle), stop.mode);
        Some(stop.mode)
    }
    fn update(&mut self, handle: &ResourceHandle, update: &Update) {
        match update {
            Update::Output { stream, bytes } => {
                let stream = match stream {
                    plugin_protocol::process::Stream::Stdout => 0,
                    plugin_protocol::process::Stream::Stderr => 1,
                    plugin_protocol::process::Stream::Pty => 2,
                };
                let mut input = self.utf8.remove(&(key(handle), stream)).unwrap_or_default();
                input.extend(bytes);
                let mut remaining = input.as_slice();
                while !remaining.is_empty() {
                    match std::str::from_utf8(remaining) {
                        Ok(text) => {
                            self.output.push_str(text);
                            break;
                        }
                        Err(error) => {
                            self.output.push_str(
                                std::str::from_utf8(&remaining[..error.valid_up_to()]).unwrap(),
                            );
                            remaining = &remaining[error.valid_up_to()..];
                            if let Some(length) = error.error_len() {
                                self.output.push('\u{fffd}');
                                remaining = &remaining[length..];
                            } else {
                                self.utf8.insert((key(handle), stream), remaining.to_vec());
                                break;
                            }
                        }
                    }
                }
            }
            Update::Exited { .. } | Update::Terminated => {
                for stream in 0..3 {
                    if let Some(bytes) = self.utf8.remove(&(key(handle), stream)) {
                        self.output.push_str(&String::from_utf8_lossy(&bytes));
                    }
                }
                self.active.remove(&key(handle));
            }
        }
        if self.output.len() > 65536 {
            let mut cut = self.output.len() - 65536;
            while !self.output.is_char_boundary(cut) {
                cut += 1;
            }
            self.output.drain(..cut);
        }
    }
}
/// Immutable resource ownership supplies a collision-free key without changing the guest handle type.
fn key(handle: &ResourceHandle) -> (String, String, u64) {
    (
        handle.instance.clone(),
        handle.scope.clone(),
        handle.resource,
    )
}
/// Shared by live/prepared instances. Weak entries cannot keep an abandoned caller or process alive.
#[derive(Clone, Debug, Default)]
pub struct PreparationRegistry(Arc<Mutex<BTreeMap<usize, Weak<Mutex<NativeWork>>>>>);
impl PreparationRegistry {
    pub(crate) fn register(&self, root: &Arc<AtomicBool>, work: &Arc<Mutex<NativeWork>>) {
        let mut entries = self.0.lock().unwrap();
        entries.retain(|_, entry| entry.strong_count() > 0);
        entries.insert(Arc::as_ptr(root) as usize, Arc::downgrade(work));
    }
    fn find(&self, lifetimes: &[Arc<AtomicBool>]) -> Option<Arc<Mutex<NativeWork>>> {
        let entries = self.0.lock().unwrap();
        lifetimes.iter().find_map(|root| {
            entries
                .get(&(Arc::as_ptr(root) as usize))
                .and_then(Weak::upgrade)
        })
    }
    /// A reaper retains the actual observer after delegated roots/references have been revoked.
    pub(crate) fn closing(
        &self,
        lifetimes: &[Arc<AtomicBool>],
        handle: &ResourceHandle,
    ) -> Box<dyn FnMut(Result<Update, String>) + Send> {
        let work = self.find(lifetimes);
        let handle = handle.clone();
        Box::new(move |result| {
            if let Some(work) = &work {
                let mut work = work.lock().unwrap();
                match result {
                    Ok(update) => work.update(&handle, &update),
                    Err(message) => {
                        work.output
                            .push_str(&format!("\nNative cleanup failed: {message}\n"));
                        work.failure = Some(message);
                    }
                }
            }
        })
    }
    /// The exact allocated resource is observed only after permission and ownership validation.
    pub(crate) fn opened(&self, lifetimes: &[Arc<AtomicBool>], handle: &ResourceHandle) {
        if let Some(work) = self.find(lifetimes) {
            work.lock().unwrap().active.insert(key(handle));
        }
    }
    pub(crate) fn update(
        &self,
        lifetimes: &[Arc<AtomicBool>],
        handle: &ResourceHandle,
        update: &Update,
    ) {
        if let Some(work) = self.find(lifetimes) {
            work.lock().unwrap().update(handle, update);
        }
    }
    pub(crate) fn sealed(&self, lifetimes: &[Arc<AtomicBool>]) -> bool {
        self.find(lifetimes).is_some_and(|work| {
            let work = work.lock().unwrap();
            work.sealed || work.stopped()
        })
    }
    pub(crate) fn control(
        &self,
        lifetimes: &[Arc<AtomicBool>],
        handle: &ResourceHandle,
    ) -> Option<ExitMode> {
        self.find(lifetimes)?.lock().unwrap().control(handle)
    }
    /// Unsupported normal exit explicitly escalates rather than pretending cleanup was graceful.
    pub(crate) fn escalate(&self, lifetimes: &[Arc<AtomicBool>]) {
        if let Some(work) = self.find(lifetimes) {
            work.lock().unwrap().stop(ExitMode::Force);
        }
    }
}
