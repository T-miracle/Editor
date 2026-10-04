//! Bounded line decoding is shared by WASI output and language-service stderr.
//! It retains immutable source attribution after the originating reader loses its UI owner.

use crate::{LogLevel, logs::MESSAGE_CHAR_LIMIT};
use std::io::Read;

/// Shared physical-line framing preserves split UTF-8 while bounding every pending message.
#[derive(Default)]
pub(crate) struct LineBuffer {
    pending: Vec<u8>,
}

impl LineBuffer {
    /// Decode complete lines only; four bytes per scalar bound storage independently of input length.
    pub(crate) fn push(&mut self, bytes: &[u8], mut publish: impl FnMut(String)) {
        for byte in bytes {
            if *byte == b'\n' {
                self.finish(&mut publish);
            } else if self.pending.len() < MESSAGE_CHAR_LIMIT * 4 {
                self.pending.push(*byte);
            }
        }
    }

    /// EOF retains a final unterminated line; CRLF and blank physical lines do not create noise.
    pub(crate) fn finish(&mut self, mut publish: impl FnMut(String)) {
        if self.pending.last() == Some(&b'\r') {
            self.pending.pop();
        }
        if !self.pending.is_empty() {
            publish(String::from_utf8_lossy(&self.pending).into_owned());
        }
        self.pending.clear();
    }

    /// Missing output breaks framing; do not join a prior partial line/UTF-8 prefix with later bytes.
    pub(crate) fn discard_pending(&mut self) {
        self.pending.clear();
    }
}

/// Consume arbitrarily long output with a bounded pending line and lossy decoding of invalid UTF-8.
/// Holding bytes until a line boundary preserves valid multibyte characters split across OS reads.
pub(crate) fn drain(
    mut reader: impl Read,
    mut publish: impl FnMut(LogLevel, &str, String),
    level: LogLevel,
    source: &str,
) {
    let mut pending = LineBuffer::default();
    let mut buffer = [0; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                pending.push(&buffer[..count], |message| publish(level, source, message));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                publish(
                    LogLevel::Error,
                    "host/log-read",
                    format!("{source}: {error}"),
                );
                break;
            }
        }
    }
    // A panic, failed preparation or normal Store retirement can end output without a newline.
    pending.finish(|message| publish(level, source, message));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeLogs;

    /// Output framing survives CRLF, split UTF-8, invalid bytes and an unterminated final line.
    #[test]
    fn split_utf8_and_final_output_retain_source_and_severity() {
        struct ByteReader(std::io::Cursor<Vec<u8>>);
        impl Read for ByteReader {
            fn read(&mut self, destination: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(&mut destination[..1])
            }
        }
        let logs = RuntimeLogs::default();
        let bytes = ["你好\r\n\n尾行".as_bytes(), &[0xff]].concat();
        drain(
            ByteReader(std::io::Cursor::new(bytes)),
            |level, source, message| {
                logs.append("guest", level, source, message);
            },
            LogLevel::Warning,
            "wasi/stderr",
        );
        let records = logs.records("guest");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].message, "你好");
        assert_eq!(records[1].message, "尾行�");
        assert_eq!(records[1].source, "wasi/stderr");
        assert_eq!(logs.unread_severity("guest"), Some(LogLevel::Warning));
    }

    /// A large physical line is bounded while later lines still flow into the same retained history.
    #[test]
    fn oversized_lines_are_drained_without_losing_following_records() {
        let logs = RuntimeLogs::default();
        let bytes = format!("{}\nnext\n", "中".repeat(MESSAGE_CHAR_LIMIT * 8));
        drain(
            bytes.as_bytes(),
            |level, source, message| {
                logs.append("guest", level, source, message);
            },
            LogLevel::Info,
            "wasi/stdout",
        );
        let records = logs.records("guest");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].message, "中".repeat(MESSAGE_CHAR_LIMIT));
        assert_eq!(records[1].message, "next");
        assert!(logs.pending_reminders().is_empty());
    }
}
