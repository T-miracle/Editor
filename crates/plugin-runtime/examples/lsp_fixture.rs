//! Test-only stdio server records the real wire and echoes unsaved text through diagnostics.
use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};

/// Flush each frame so tests observe protocol order without simulated transport.
fn send(value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    let mut output = std::io::stdout().lock();
    write!(output, "Content-Length: {}\r\n\r\n", bytes.len()).unwrap();
    output.write_all(&bytes).unwrap();
    output.flush().unwrap();
}
fn main() {
    let log = std::env::args().nth(1).expect("wire log path");
    // Startup arguments are observable independently of initialization JSON.
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .unwrap();
    writeln!(
        file,
        "{}",
        json!({"startupArgs":std::env::args().collect::<Vec<_>>()})
    )
    .unwrap();
    let mut input = std::io::BufReader::new(std::io::stdin().lock());
    loop {
        let mut length = 0;
        loop {
            let mut line = String::new();
            if input.read_line(&mut line).unwrap() == 0 {
                return;
            }
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = value.trim().parse::<usize>().unwrap();
            }
        }
        assert!(length <= 32 * 1024 * 1024);
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes).unwrap();
        let message: Value = serde_json::from_slice(&bytes).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .unwrap();
        writeln!(file, "{message}").unwrap();
        let method = message["method"].as_str().unwrap_or("");
        let id = &message["id"];
        let params = &message["params"];
        // Recreating the marker after a healthy handshake also exercises a short-lived reconnect.
        let args = std::env::args().collect::<Vec<_>>();
        if !method.is_empty()
            && args
                .windows(2)
                .any(|pair| pair[0] == "--crash-marker" && std::path::Path::new(&pair[1]).exists())
        {
            // A real startup crash can leave its diagnostic tail without a line terminator.
            if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--crash-stderr") {
                let mut errors = std::io::stderr().lock();
                errors.write_all(pair[1].as_bytes()).unwrap();
                errors.flush().unwrap();
            }
            std::process::exit(19);
        }
        let result = match method {
            "initialize" => {
                json!({"capabilities":{"completionProvider":{},"definitionProvider":true,"textDocumentSync":{"openClose":true,"change":1,"save":true}}})
            }
            "initialized" => {
                send(
                    json!({"jsonrpc":"2.0","id":"server-config","method":"workspace/configuration","params":{"items":[{"section":"fixture.analysis"}]}}),
                );
                send(json!({"jsonrpc":"2.0","method":"fixture/status","params":{"state":"ready"}}));
                // Native UI acceptance uses the same real transport without a host-only injection API.
                if let Some(pair) = args
                    .windows(2)
                    .find(|pair| pair[0] == "--initial-notifications")
                {
                    let notifications: Vec<Value> =
                        serde_json::from_slice(&std::fs::read(&pair[1]).unwrap()).unwrap();
                    for notification in notifications {
                        send(notification);
                    }
                }
                continue;
            }
            "textDocument/didOpen" | "textDocument/didChange" => {
                let document = &params["textDocument"];
                let source = document["text"]
                    .as_str()
                    .or_else(|| params["contentChanges"][0]["text"].as_str())
                    .unwrap();
                send(
                    json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{
                    "uri":document["uri"],"version":document["version"],"diagnostics":[{
                        "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},
                        "severity":2,"message":format!("unsaved:{source}")}]}}),
                );
                continue;
            }
            "textDocument/completion" => json!([{"label":"fixture-completion"}]),
            "textDocument/definition" => {
                json!({"uri":params["textDocument"]["uri"],"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}}})
            }
            "fixture/notifications" => {
                // Exercise real server notifications without adding any host-side test-only operation.
                if let Some(notifications) = params["notifications"].as_array() {
                    for notification in notifications {
                        send(notification.clone());
                    }
                }
                Value::Null
            }
            "fixture/fail-with-stderr" => {
                // Keep stderr open after a malformed frame, so only the host's fault teardown
                // releases this unterminated diagnostic. A controlled stop must still suppress it.
                let mut errors = std::io::stderr().lock();
                errors
                    .write_all(params["message"].as_str().unwrap().as_bytes())
                    .unwrap();
                errors.flush().unwrap();
                let mut output = std::io::stdout().lock();
                output
                    .write_all(b"Content-Length: 8\r\n\r\nnot-json")
                    .unwrap();
                output.flush().unwrap();
                continue;
            }
            "fixture/stderr" => {
                // The normal reply is a wire barrier confirming that the pending line was written.
                let mut errors = std::io::stderr().lock();
                errors
                    .write_all(params["message"].as_str().unwrap().as_bytes())
                    .unwrap();
                errors.flush().unwrap();
                Value::Null
            }
            "fixture/pending" => continue,
            "fixture/block-with-ready" => {
                // A reply much larger than the native pipe buffer tests the caller's existing deadline.
                // Stop reading stdin only after publishing the request, readiness, and a receipt barrier.
                send(
                    json!({"jsonrpc":"2.0", "id":"blocked-config", "method":"workspace/configuration",
                    "params":{"items":vec![json!({"section":"missing"}); 65_536]}}),
                );
                send(json!({"jsonrpc":"2.0","method":"fixture/status","params":{"state":"ready"}}));
                send(json!({"jsonrpc":"2.0","method":"window/logMessage",
                    "params":{"type":3,"message":"fixture stdin blocked"}}));
                // The host must terminate the process when the bounded readiness operation fails.
                std::thread::sleep(std::time::Duration::from_secs(60));
                continue;
            }
            _ => Value::Null,
        };
        if !id.is_null() && !method.is_empty() {
            send(json!({"jsonrpc":"2.0","id":id,"result":result}));
        }
    }
}
