//! Continuously reads framed LSP messages, including unsolicited diagnostic notifications.

use anyhow::{Context as _, ensure};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::ChildStdout,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

/// Bound queued document notifications without letting an idle document consumer block log receipt.
pub(super) const MESSAGE_CAPACITY: usize = 256;

/// The pipe reader owns a connection lifetime distinct from the reusable service plan.
pub(super) struct Messages {
    receiver: mpsc::Receiver<anyhow::Result<Value>>,
    stopped: Arc<AtomicBool>,
    /// Readiness is connection control state, independent of the lossy ordinary notification queue.
    ready: Arc<AtomicBool>,
}
impl Messages {
    pub(super) fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<anyhow::Result<Value>, mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
    pub(super) fn try_recv(&self) -> Result<anyhow::Result<Value>, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
    /// The reader observes the plugin's declared condition even when the document queue is full.
    pub(super) fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
    /// Intentional teardown revokes logging before closing the native pipe.
    pub(super) fn stop_logging(&self) {
        self.stopped.store(true, Ordering::Release);
    }
}
impl Drop for Messages {
    fn drop(&mut self) {
        self.stop_logging();
    }
}

/// Native writes can block on an unresponsive child, so a bounded writer owns the pipe off the request thread.
pub(super) struct Writer(mpsc::SyncSender<(Vec<u8>, mpsc::SyncSender<std::io::Result<()>>)>);
impl Writer {
    pub(super) fn new(mut input: std::process::ChildStdin) -> Self {
        let (sender, receiver) =
            mpsc::sync_channel::<(Vec<u8>, mpsc::SyncSender<std::io::Result<()>>)>(1);
        std::thread::spawn(move || {
            while let Ok((bytes, reply)) = receiver.recv() {
                let result = input.write_all(&bytes).and_then(|_| input.flush());
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    break;
                }
            }
        });
        Self(sender)
    }
    pub(super) fn send(&self, message: Value, timeout: std::time::Duration) -> anyhow::Result<()> {
        self.enqueue(message)?
            .recv_timeout(timeout)
            .context("wait for LSP pipe write")??;
        Ok(())
    }
    /// Cancellation never extends an expired request deadline while a native pipe is blocked.
    pub(super) fn enqueue(
        &self,
        message: Value,
    ) -> anyhow::Result<mpsc::Receiver<std::io::Result<()>>> {
        let payload = serde_json::to_vec(&message)?;
        ensure!(
            payload.len() <= 32 * 1024 * 1024,
            "LSP output frame exceeds quota"
        );
        let mut frame = format!("Content-Length: {}\r\n\r\n", payload.len()).into_bytes();
        frame.extend(payload);
        let (reply, completion) = mpsc::sync_channel(1);
        self.0
            .try_send((frame, reply))
            .map_err(|error| anyhow::anyhow!("LSP writer unavailable: {error}"))?;
        Ok(completion)
    }
}

/// One bounded reader queue keeps server output flowing while the editor is idle.
pub(super) fn reader(
    stdout: ChildStdout,
    service: Arc<plugin_runtime::LanguageService>,
    retired: Arc<AtomicBool>,
) -> anyhow::Result<Messages> {
    let (sender, receiver) = mpsc::sync_channel(MESSAGE_CAPACITY);
    let stopped = Arc::new(AtomicBool::new(false));
    let reader_stopped = stopped.clone();
    let ready = Arc::new(AtomicBool::new(false));
    let reader_ready = ready.clone();
    std::thread::Builder::new()
        .name("language-server-reader".into())
        .spawn(move || {
            let mut output = BufReader::new(stdout);
            // One warning per connection reports discarded ordinary notifications without flooding logs.
            let mut overflow_reported = false;
            loop {
                let message = read_frame(&mut output);
                let failed = message.is_err();
                if let Ok(message) = &message
                    && let Some(readiness) = &service.provider.readiness
                    && message.get("method").and_then(Value::as_str)
                        == Some(readiness.notification.as_str())
                {
                    // Match arbitrary declared control notifications before log filtering or queue admission.
                    reader_ready.store(
                        message["params"].pointer(&readiness.pointer) == Some(&readiness.expected),
                        Ordering::Release,
                    );
                }
                // The originating adapter and connection must both still own this callback.
                if !reader_stopped.load(Ordering::Acquire) {
                    match &message {
                        Ok(message) => {
                            if log_notification(&service, &retired, message)
                                && message.get("id").is_none()
                            {
                                // Log-only pushes do not fill the document queue while the editor is idle.
                                continue;
                            }
                        }
                        Err(error) => {
                            service.append_runtime_log(
                                &retired,
                                plugin_runtime::LogLevel::Error,
                                &format!("lsp/{}", service.provider.id),
                                format!(
                                    "{}: {error:#}",
                                    rust_i18n::t!("plugins.logs.lsp_output_failed")
                                ),
                            );
                        }
                    }
                }
                if message.as_ref().is_ok_and(|message| {
                    message.get("id").is_none()
                        && message.get("method").and_then(Value::as_str).is_some()
                }) {
                    // Only ordinary notifications may be discarded. Replies and server requests retain
                    // their reliable route below, and readiness was already observed outside this queue.
                    match sender.try_send(message) {
                        Ok(()) => {}
                        Err(mpsc::TrySendError::Full(_)) => {
                            if !overflow_reported && !reader_stopped.load(Ordering::Acquire) {
                                overflow_reported = true;
                                service.append_runtime_log(
                                    &retired,
                                    plugin_runtime::LogLevel::Warning,
                                    &format!("lsp/{}", service.provider.id),
                                    rust_i18n::t!("plugins.logs.notification_overflow").to_string(),
                                );
                            }
                        }
                        Err(mpsc::TrySendError::Disconnected(_)) => break,
                    }
                    continue;
                }
                if sender.send(message).is_err() || failed {
                    break;
                }
            }
        })
        .context("start language server output reader")?;
    Ok(Messages {
        receiver,
        stopped,
        ready,
    })
}

/// LSP message levels describe server runtime events, independently of document diagnostics.
/// Returns whether this is a log/show notification; other protocol messages retain their normal route.
fn log_notification(
    service: &plugin_runtime::LanguageService,
    retired: &AtomicBool,
    message: &Value,
) -> bool {
    let Some(method @ ("window/logMessage" | "window/showMessage")) =
        message.get("method").and_then(Value::as_str)
    else {
        return false;
    };
    let (level, text) =
        match serde_json::from_value::<lsp_types::LogMessageParams>(message["params"].clone()) {
            Ok(params) => {
                let level = if params.typ == lsp_types::MessageType::ERROR {
                    plugin_runtime::LogLevel::Error
                } else if params.typ == lsp_types::MessageType::WARNING {
                    plugin_runtime::LogLevel::Warning
                } else {
                    plugin_runtime::LogLevel::Info
                };
                (level, params.message)
            }
            Err(error) => (
                plugin_runtime::LogLevel::Warning,
                format!(
                    "{} ({method}): {error}",
                    rust_i18n::t!("plugins.logs.lsp_invalid_message")
                ),
            ),
        };
    service.append_runtime_log(
        retired,
        level,
        &format!("lsp/{}", service.provider.id),
        text,
    );
    true
}

/// Decode the same bounded framing for requests, responses, and diagnostic pushes.
fn read_frame(output: &mut impl BufRead) -> anyhow::Result<Value> {
    let mut content_length = None;
    let mut header_bytes = 0;
    loop {
        let mut header = String::new();
        let read = output
            .by_ref()
            .take((8193 - header_bytes) as u64)
            .read_line(&mut header)
            .context("read LSP header")?;
        ensure!(read > 0, "language server closed its output");
        header_bytes += read;
        ensure!(
            header_bytes <= 8192,
            "language server header exceeded 8 KiB"
        );
        if header == "\r\n" || header == "\n" {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = Some(value.trim().parse::<usize>().context("parse LSP length")?);
        }
    }
    let length = content_length.context("language server response omitted Content-Length")?;
    ensure!(
        length <= 32 * 1024 * 1024,
        "language server response exceeded 32 MiB"
    );
    let mut payload = vec![0; length];
    output
        .read_exact(&mut payload)
        .context("read LSP message body")?;
    serde_json::from_slice(&payload).context("parse language server JSON-RPC response")
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A server cannot allocate an unbounded String by withholding a header newline.
    #[test]
    fn oversized_unterminated_header_is_bounded() {
        let mut input = std::io::Cursor::new(vec![b'x'; 50_000]);
        assert!(read_frame(&mut input).is_err());
        assert!(input.position() <= 8193);
    }

    /// Diagnostic pushes and responses can share a stream without consuming each other's bytes.
    #[test]
    fn consecutive_frames_preserve_notifications() {
        let notification =
            r#"{"method":"textDocument/publishDiagnostics","params":{"diagnostics":[]}}"#;
        let response = r#"{"id":1,"result":null}"#;
        let wire = format!(
            "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
            notification.len(),
            notification,
            response.len(),
            response
        );
        let mut input = std::io::Cursor::new(wire);
        assert_eq!(
            read_frame(&mut input).unwrap()["method"],
            "textDocument/publishDiagnostics"
        );
        assert_eq!(read_frame(&mut input).unwrap()["id"], 1);
    }
}
