//! Continuously reads framed LSP messages, including unsolicited diagnostic notifications.

use anyhow::{Context as _, ensure};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    process::ChildStdout,
    sync::mpsc,
};

pub(super) type Messages = mpsc::Receiver<anyhow::Result<Value>>;

/// One bounded reader queue keeps server output flowing while the editor is idle.
pub(super) fn reader(stdout: ChildStdout) -> anyhow::Result<Messages> {
    let (sender, receiver) = mpsc::sync_channel(256);
    std::thread::Builder::new()
        .name("language-server-reader".into())
        .spawn(move || {
            let mut output = BufReader::new(stdout);
            loop {
                let message = read_frame(&mut output);
                let failed = message.is_err();
                if sender.send(message).is_err() || failed {
                    break;
                }
            }
        })
        .context("start language server output reader")?;
    Ok(receiver)
}

/// Decode the same bounded framing for requests, responses, and diagnostic pushes.
fn read_frame(output: &mut impl BufRead) -> anyhow::Result<Value> {
    let mut content_length = None;
    let mut header_bytes = 0;
    loop {
        let mut header = String::new();
        let read = output.read_line(&mut header).context("read LSP header")?;
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
