//! WASI output enters a bounded memory queue without waiting on OS pipes or the shared log lock.
//! A reader retains source attribution and flushes the final line after every output handle retires.

use crate::{LogLevel, RuntimeLogs, logs::LineBuffer};
use bytes::Bytes;
use std::{
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError, sync_channel},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::io::AsyncWrite;
use wasmtime_wasi::{
    cli::{IsTerminal, StdoutStream},
    p2::{OutputStream, Pollable, StreamError, StreamResult},
};

/// A single WASI host write has a finite byte budget, independently of guest fuel or epoch checks.
const WRITE_LIMIT: usize = 4096;
/// Each stream retains at most 128 KiB of queued bytes, plus the bounded line decoder's pending tail.
const QUEUED_CHUNKS: usize = 32;
/// Coalesce overload notices rather than converting every discarded write into another warning.
const NOTICE_INTERVAL: Duration = Duration::from_secs(1);

enum Chunk {
    Bytes {
        bytes: Bytes,
        /// A new value marks missing bytes between successfully admitted chunks.
        dropped_before: u64,
    },
    /// Wake an idle reader when an illegal oversized write closes a previously empty stream.
    RejectedPermit,
}

struct OutputState {
    sender: SyncSender<Chunk>,
    /// Only illegal permits or a disconnected reader close admission; normal congestion never does.
    closed: Arc<AtomicBool>,
    /// Constant-size bookkeeping works even when the queue has no slot for a loss notification.
    dropped: Arc<AtomicU64>,
}

/// All WASI handles share bounded admission; lost logging data never fails a legal guest write.
#[derive(Clone)]
pub(super) struct CapturedOutput(Arc<OutputState>);

impl CapturedOutput {
    /// Admission never waits for a reader or takes the RuntimeLogs mutex; publication belongs to the worker.
    fn enqueue(&self, bytes: Bytes) -> StreamResult<()> {
        if bytes.len() > WRITE_LIMIT {
            self.reject_permit();
            return Err(StreamError::trap(
                "WASI output write exceeds its advertised 4096-byte permit",
            ));
        }
        if self.0.closed.load(Ordering::Acquire) {
            return Err(StreamError::Closed);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        // A small Bytes slice can own a much larger allocation; retain only this admitted chunk.
        match self.0.sender.try_send(Chunk::Bytes {
            bytes: Bytes::copy_from_slice(&bytes),
            dropped_before: self.0.dropped.load(Ordering::Acquire),
        }) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                // Rust println/eprintln unwrap stream errors: diagnostic backpressure must not panic guests.
                self.0.dropped.fetch_add(1, Ordering::AcqRel);
                Ok(())
            }
            Err(TrySendError::Disconnected(_)) => {
                self.0.closed.store(true, Ordering::Release);
                Err(StreamError::Closed)
            }
        }
    }

    /// An illegal permit can close this stream; this state is independent of ordinary discarded chunks.
    fn reject_permit(&self) {
        if !self.0.closed.swap(true, Ordering::AcqRel) {
            // A full queue already wakes the reader; this event handles an otherwise idle queue.
            let _ = self.0.sender.try_send(Chunk::RejectedPermit);
        }
    }

    fn writable(&self) -> StreamResult<usize> {
        if self.0.closed.load(Ordering::Acquire) {
            Err(StreamError::Closed)
        } else {
            Ok(WRITE_LIMIT)
        }
    }
}

impl IsTerminal for CapturedOutput {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for CapturedOutput {
    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(self.clone())
    }

    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(self.clone())
    }
}

#[wasmtime_wasi::async_trait]
impl Pollable for CapturedOutput {
    // Readiness is immediate; legal overload is discarded, and no WASI poll waits on log publication.
    async fn ready(&mut self) {}
}

#[wasmtime_wasi::async_trait]
impl OutputStream for CapturedOutput {
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        self.enqueue(bytes)
    }

    fn flush(&mut self) -> StreamResult<()> {
        // Accepted chunks already have independent ownership; partial physical lines wait for EOF.
        self.writable().map(|_| ())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        self.writable()
    }

    /// Override the convenience loop so even a direct blocking-write call cannot spend unbounded CPU.
    async fn blocking_write_and_flush(&mut self, bytes: Bytes) -> StreamResult<()> {
        self.enqueue(bytes)?;
        self.writable().map(|_| ())
    }
}

impl AsyncWrite for CapturedOutput {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let count = bytes.len().min(WRITE_LIMIT);
        Poll::Ready(
            self.enqueue(Bytes::copy_from_slice(&bytes[..count]))
                .map(|_| count)
                .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error.to_string())),
        )
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(
            self.writable()
                .map(|_| ())
                .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error.to_string())),
        )
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Shutting down one borrowed handle must not close its peers or discard their pending tail.
        self.poll_flush(cx)
    }
}

/// Only the background reader may block; it never owns a sender that could prevent EOF on retirement.
struct OutputReader {
    receiver: Receiver<Chunk>,
    pending: LineBuffer,
    closed: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    received_generation: u64,
    reported_drops: u64,
    next_notice: Instant,
}

impl OutputReader {
    /// Reader-side framing keeps guest calls constant-cost and prevents missing chunks from joining lines.
    fn run(mut self, logs: &RuntimeLogs, plugin: &str, level: LogLevel, source: &str) {
        loop {
            // Illegal permits end this reader after admitted data drains; ordinary Full never creates EOF.
            let chunk = if self.closed.load(Ordering::Acquire) {
                match self.receiver.try_recv() {
                    Ok(chunk) => chunk,
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            } else {
                match self.receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(chunk) => chunk,
                    Err(RecvTimeoutError::Timeout) => {
                        // Full queues have no event slot; an idle retained handle still reports accumulated loss.
                        self.publish_drops(logs, plugin, source, false);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            };
            match chunk {
                Chunk::Bytes {
                    bytes,
                    dropped_before,
                } => {
                    if dropped_before != self.received_generation {
                        self.pending.discard_pending();
                        self.received_generation = dropped_before;
                    }
                    self.pending.push(&bytes, |message| {
                        logs.append(plugin, level, source, message);
                    });
                    self.publish_drops(logs, plugin, source, false);
                }
                Chunk::RejectedPermit => {}
            }
        }
        if self.received_generation != self.dropped.load(Ordering::Acquire) {
            // Missing bytes after the final admitted chunk make its unfinished line unreliable.
            self.pending.discard_pending();
        }
        self.pending.finish(|message| {
            logs.append(plugin, level, source, message);
        });
        // EOF bypasses the time gate so a short-lived process never loses its last overload notice.
        self.publish_drops(logs, plugin, source, true);
        if self.closed.load(Ordering::Acquire) {
            logs.append(
                plugin,
                LogLevel::Warning,
                source,
                "WASI output exceeded its advertised write permit; remaining output was truncated.",
            );
        }
    }

    /// Live readers emit at most one overload warning per second; EOF reports pending losses once.
    fn publish_drops(&mut self, logs: &RuntimeLogs, plugin: &str, source: &str, eof: bool) {
        let total = self.dropped.load(Ordering::Acquire);
        if total == self.reported_drops || (!eof && Instant::now() < self.next_notice) {
            return;
        }
        let count = total.wrapping_sub(self.reported_drops);
        logs.append(
            plugin,
            LogLevel::Warning,
            source,
            format!(
                "Dropped {count} WASI output chunk(s) because the bounded log queue was full. Log collection continues."
            ),
        );
        self.reported_drops = total;
        self.next_notice = Instant::now() + NOTICE_INTERVAL;
    }
}

/// Keep the output sender solely in handles, so the reader cannot keep its own EOF boundary alive.
fn output_channel() -> (CapturedOutput, OutputReader) {
    let (sender, receiver) = sync_channel(QUEUED_CHUNKS);
    let closed = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicU64::new(0));
    let output = CapturedOutput(Arc::new(OutputState {
        sender,
        closed: closed.clone(),
        dropped: dropped.clone(),
    }));
    let reader = OutputReader {
        receiver,
        pending: LineBuffer::default(),
        closed,
        dropped,
        received_generation: 0,
        reported_drops: 0,
        next_notice: Instant::now() + NOTICE_INTERVAL,
    };
    (output, reader)
}

/// Build a nonterminal WASI stdout/stderr adapter with bounded admission and immutable attribution.
pub(super) fn capture(
    logs: RuntimeLogs,
    plugin: &str,
    level: LogLevel,
    source: &str,
) -> anyhow::Result<CapturedOutput> {
    let (output, reader) = output_channel();
    let plugin = plugin.to_owned();
    let source = source.to_owned();
    std::thread::Builder::new()
        .name(format!("plugin-log-{source}"))
        .spawn(move || {
            reader.run(&logs, &plugin, level, &source);
        })?;
    Ok(output)
}

#[cfg(test)]
mod tests;
