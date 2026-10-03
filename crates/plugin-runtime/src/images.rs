//! Source-bound image consumers publish immutable bytes without lending ambient access to WASM.
use plugin_protocol::api;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

mod loader;

pub(crate) const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_RESIDENT: usize = 64 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);
static WORKERS: AtomicUsize = AtomicUsize::new(0);

/// Immutable result for one declared node in its current source version.
/// Its manager key is `plugin/panel/image/node_id`; this does not grant guest resource handles.
#[derive(Clone, Debug)]
pub struct ImageResource {
    /// Exact open-document identity, path and revision echoed by the owning preview.
    pub source: api::DocumentVersion,
    /// Original declared URI; this is metadata, never an ambient file or URL rendering source.
    pub uri: String,
    /// Immutable progress or terminal result for this incarnation/source/node/URI identity.
    pub state: ImageState,
}

/// Loading and failure are local to one image; native decoding uses only the returned bounded bytes.
#[derive(Clone, Debug)]
pub enum ImageState {
    /// Queued or running within the shared finite worker pool and 30-second lifetime.
    Loading,
    /// At most 8 MiB of encoded bytes; the host still performs bounded, safe native decoding.
    Ready(Arc<Vec<u8>>),
    /// Permission, path, quota, timeout or IO failure local to this image.
    Failed(api::Failure),
}

/// Changing any authority or source identity retires the previous consumer before accepting a result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub instance: String,
    pub source: api::DocumentVersion,
    pub uri: String,
    pub workspace: String,
    pub local: bool,
    pub network: bool,
}

pub(crate) struct Entry {
    pub identity: Identity,
    pub resource: Arc<ImageResource>,
    pub task: Option<Task>,
    /// The reservation outlives the worker and is released when this published resource is retired.
    pub bytes: Option<Reservation>,
}

impl Entry {
    pub fn new(identity: Identity) -> Self {
        let uri = loader::authorize(&identity);
        let state = match &uri {
            Ok(_) => ImageState::Loading,
            Err(error) => ImageState::Failed(error.clone()),
        };
        Self {
            resource: Arc::new(ImageResource {
                source: identity.source.clone(),
                uri: identity.uri.clone(),
                state,
            }),
            task: uri.ok().map(|uri| Task::new(identity.clone(), uri)),
            identity,
            bytes: None,
        }
    }

    /// Only the still-owned receiver can publish. A dropped consumer never joins an IO thread.
    pub fn poll(&mut self, budget: &Arc<AtomicUsize>) {
        let Some(task) = self.task.as_mut() else {
            return;
        };
        let result = task.poll().or_else(|| task.start(budget).err().map(Err));
        if let Some(result) = result {
            let state = match result {
                Ok(loaded) => {
                    self.bytes = Some(loaded.reservation);
                    ImageState::Ready(loaded.bytes)
                }
                Err(error) => ImageState::Failed(error),
            };
            self.task = None;
            self.resource = Arc::new(ImageResource {
                source: self.identity.source.clone(),
                uri: self.identity.uri.clone(),
                state,
            });
        }
    }
}

pub(crate) struct Loaded {
    bytes: Arc<Vec<u8>>,
    reservation: Reservation,
}

/// Charge actual encoded bytes from all active producers and retained results to one manager.
pub(crate) struct Reservation {
    budget: Arc<AtomicUsize>,
    bytes: usize,
}
impl Reservation {
    fn new(budget: Arc<AtomicUsize>) -> Self {
        Self { budget, bytes: 0 }
    }
    fn add(&mut self, bytes: usize) -> Result<(), api::Failure> {
        self.budget
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |resident| {
                resident
                    .checked_add(bytes)
                    .filter(|next| *next <= MAX_RESIDENT)
            })
            .map_err(|_| {
                api::Failure::new(
                    api::ErrorCode::LimitExceeded,
                    "Image resident byte quota exceeded",
                )
            })?;
        self.bytes += bytes;
        Ok(())
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

struct WorkerPermit;
impl WorkerPermit {
    fn acquire() -> Option<Self> {
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 8).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
impl Drop for WorkerPermit {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) struct Control {
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}
impl Control {
    fn check(&self) -> Result<(), api::Failure> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(api::Failure::new(
                api::ErrorCode::Cancelled,
                "Image consumer retired",
            ))
        } else if Instant::now() >= self.deadline {
            Err(api::Failure::new(
                api::ErrorCode::TimedOut,
                "Image loading exceeded 30 seconds",
            ))
        } else {
            Ok(())
        }
    }
}

pub(crate) struct Task {
    identity: Identity,
    uri: loader::Uri,
    control: Control,
    receiver: Option<mpsc::Receiver<Result<Loaded, api::Failure>>>,
}
impl Task {
    fn new(identity: Identity, uri: loader::Uri) -> Self {
        Self {
            identity,
            uri,
            control: Control {
                cancelled: Arc::new(AtomicBool::new(false)),
                deadline: Instant::now() + TIMEOUT,
            },
            receiver: None,
        }
    }
    fn poll(&mut self) -> Option<Result<Loaded, api::Failure>> {
        if let Err(error) = self.control.check() {
            return Some(Err(error));
        }
        let receiver = self.receiver.as_ref()?;
        match receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(api::Failure::new(
                api::ErrorCode::OperationFailed,
                "Image loading worker stopped",
            ))),
        }
    }
    /// Busy producers leave the node queued; the process-wide permit also bounds retired managers.
    fn start(&mut self, budget: &Arc<AtomicUsize>) -> Result<(), api::Failure> {
        if self.receiver.is_some() {
            return Ok(());
        }
        let Some(permit) = WorkerPermit::acquire() else {
            return Ok(());
        };
        let (tx, rx) = mpsc::sync_channel(1);
        let identity = self.identity.clone();
        let uri = self.uri.clone();
        let control = Control {
            cancelled: self.control.cancelled.clone(),
            deadline: self.control.deadline,
        };
        let budget = budget.clone();
        std::thread::Builder::new()
            .name("plugin-image-loader".into())
            .spawn(move || {
                let _permit = permit;
                let result = loader::load(&identity, &uri, &control, budget);
                // Dropping a cancelled receiver also drops any result and its byte reservation.
                let _ = tx.send(result);
            })
            .map_err(|_| {
                api::Failure::new(api::ErrorCode::OperationFailed, "Cannot start image worker")
            })?;
        self.receiver = Some(rx);
        Ok(())
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        self.control.cancelled.store(true, Ordering::Release);
    }
}
