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
        let result = match method {
            "initialize" => {
                json!({"capabilities":{"completionProvider":{},"definitionProvider":true,"textDocumentSync":{"openClose":true,"change":1,"save":true}}})
            }
            "initialized" => {
                send(
                    json!({"jsonrpc":"2.0","id":"server-config","method":"workspace/configuration","params":{"items":[{"section":"fixture.analysis"}]}}),
                );
                send(json!({"jsonrpc":"2.0","method":"fixture/status","params":{"state":"ready"}}));
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
            "fixture/pending" => continue,
            _ => Value::Null,
        };
        if !id.is_null() && !method.is_empty() {
            send(json!({"jsonrpc":"2.0","id":id,"result":result}));
        }
    }
}
