//! Real public-SDK debug packages reuse verified official bytes locally to keep acceptance offline.
use plugin_runtime::Package;
use serde_json::{Value, json};
use sha2::Digest;
use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

/// Change only acquisition to the exact already verified VSIX; identity, hash and private DAG remain.
pub fn debugger(id: &str) -> Package {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let files = Package::read(&root.join("dist/plugins/rust-debugger.zip"))
        .unwrap()
        .files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!(id);
    let archive = root
        .join("target/debug-adapters/codelldb-1.12.3/codelldb-win32-x64.vsix")
        .canonicalize()
        .expect("download and verify the pinned official CodeLLDB VSIX first");
    manifest["services"]["adapter"]["installation"]["artifacts"][0]["source"] =
        json!({"kind":"local","path":archive.display().to_string()});
    package(files, manifest)
}
/// Repackage literal public metadata; this never injects host callbacks or a private SDK.
pub fn package(mut files: std::collections::BTreeMap<String, Vec<u8>>, manifest: Value) -> Package {
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// A native DAP fault instrument drives the real WASM provider through its installed service.
/// It rejects a legal step and returns oversized variables; it is not evidence of real debugging.
pub fn fault_adapter(id: &str, directory: &Path) -> Package {
    instrument(id, directory, FAULT_ADAPTER)
}
/// The same public package drives a nonresponsive disconnect and a real three-process tree.
pub fn stubborn_adapter(id: &str, directory: &Path) -> Package {
    let source = FAULT_ADAPTER
        .replace(
            "if command == \"next\"",
            "if command == \"disconnect\" { continue; }\n        if command == \"next\"",
        )
        .replace("    let input = std::io::stdin();", TREE_START);
    instrument(id, directory, &source)
}
/// An initialization refusal keeps a real inherited-stdio tree alive until the provider retires it.
pub fn initialization_failure_adapter(id: &str, directory: &Path) -> Package {
    let source=FAULT_ADAPTER.replace("    let input = std::io::stdin();",TREE_START)
        .replace("        if command == \"next\"",r##"        if command == "initialize" {
            let marker=std::path::PathBuf::from(std::env::args().nth(1).unwrap());
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
            // Prove descendants exist before publishing a refused initialization.
            while !marker.with_extension("grandchild").exists() {
                assert!(std::time::Instant::now()<deadline);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            emit(&format!(r#"{{"type":"response","request_seq":{sequence},"success":false,"message":"INITIALIZE_REFUSED_BY_INSTRUMENT"}}"#));
            continue;
        }
        if command == "next""##);
    instrument(id, directory, &source)
}

/// Package a compiled instrument as an ordinary controlled dependency, without host test hooks.
fn instrument(id: &str, directory: &Path, instrument_source: &str) -> Package {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Package::read(&root.join("dist/plugins/rust-debugger.zip"))
        .unwrap()
        .files;
    let source = directory.join("fault_adapter.rs");
    let binary = directory.join("fault_adapter.exe");
    std::fs::write(&source, instrument_source).unwrap();
    let build = std::process::Command::new("rustc")
        .args(["--edition", "2024", "-o"])
        .arg(&binary)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let bytes = std::fs::read(&binary).unwrap();
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!(id);
    let digest = format!("{:x}", sha2::Sha256::digest(&bytes));
    manifest["services"]["adapter"] = json!({"program":"fault-adapter","args":[directory.join("adapter-pids").display().to_string()],"installation":{
        "artifacts":[{"id":"instrument","version":"0.1.0","platform":"windows-x86_64","sha256":digest,
            "source":{"kind":"package","path":"native/fault_adapter.exe"},"format":{"kind":"file","path":"fault_adapter.exe"}}],
        "executable":"instrument/fault_adapter.exe"}});
    files.insert("native/fault_adapter.exe".into(), bytes);
    package(files, manifest)
}

/// Every instrument generation writes real PIDs, with inherited stdio retained by descendants.
const TREE_START: &str = r#"
    let arguments=std::env::args().collect::<Vec<_>>();
    let marker=std::path::PathBuf::from(&arguments[1]);
    if let Some(level)=arguments.get(2) {
        std::fs::write(marker.with_extension(level),std::process::id().to_string()).unwrap();
        if level=="child" {
            let mut child=std::process::Command::new(std::env::current_exe().unwrap()).arg(&marker).arg("grandchild").spawn().unwrap();
            child.wait().unwrap();
        } else {std::thread::sleep(std::time::Duration::from_secs(120));}
        return;
    }
    std::fs::write(marker.with_extension("root"),std::process::id().to_string()).unwrap();
    let _child=std::process::Command::new(std::env::current_exe().unwrap()).arg(&marker).arg("child").spawn().unwrap();
    let input = std::io::stdin();
"#;

/// Literal framed messages let the instrument run without any extra library or compiler download.
const FAULT_ADAPTER: &str = r##"
//! Deterministic DAP fault instrument; only the surrounding test labels its responses as simulated.
use std::io::{BufRead,Read,Write};
/// Emit one complete DAP body with the required framing and flush its observable bytes.
fn emit(body:&str) { print!("Content-Length: {}\r\n\r\n{}",body.len(),body); std::io::stdout().flush().unwrap(); }
fn main() {
    let input = std::io::stdin(); let mut input = input.lock();
    loop {
        let mut header = String::new(); if input.read_line(&mut header).unwrap() == 0 { break; }
        let length:usize = header.strip_prefix("Content-Length:").unwrap().trim().parse().unwrap();
        let mut blank = String::new(); input.read_line(&mut blank).unwrap();
        let mut bytes = vec![0;length]; input.read_exact(&mut bytes).unwrap();
        let body = String::from_utf8(bytes).unwrap();
        let sequence:u32 = body.split("\"seq\":").nth(1).unwrap().split(|c:char| !c.is_ascii_digit()).next().unwrap().parse().unwrap();
        let command = body.split("\"command\":\"").nth(1).unwrap().split('"').next().unwrap();
        if command == "next" { emit(&format!(r#"{{"type":"response","request_seq":{sequence},"success":false,"message":"STEP_REFUSED_BY_INSTRUMENT"}}"#)); continue; }
        let result = match command {
            "stackTrace" => r#"{"stackFrames":[{"id":0,"name":"main","source":{"path":"main.rs"},"line":8}]}"#.into(),
            "scopes" => r#"{"scopes":[{"variablesReference":1,"expensive":false}]}"#.into(),
            "variables" => format!(r#"{{"variables":[{}]}}"#, (0..32).map(|i|format!(r#"{{"name":"local_{i}","value":"{}"}}"#,"x".repeat(4096))).collect::<Vec<_>>().join(",")),
            _ => "{}".into()
        };
        emit(&format!(r#"{{"type":"response","request_seq":{sequence},"success":true,"body":{result}}}"#));
        if command=="launch" {
            let marker=body.split("\"program\":\"").nth(1).unwrap().split('"').next().unwrap();
            emit(&format!(r#"{{"type":"event","event":"output","body":{{"output":"{}:{marker}","category":"stdout"}}}}"#,"x".repeat(7500)));
            eprintln!("{}", "d".repeat(2048));
        }
        if command=="initialize" {
            let marker=std::path::PathBuf::from(std::env::args().nth(1).unwrap());
            std::fs::write(marker.with_extension("root"),std::process::id().to_string()).unwrap();
        }
        if command == "initialize" { emit(r#"{"type":"event","event":"initialized","body":{}}"#); }
        if command == "configurationDone" { emit(r#"{"type":"event","event":"stopped","body":{"threadId":1,"reason":"breakpoint"}}"#); }
        if command == "disconnect" {
            let marker=std::path::PathBuf::from(std::env::args().nth(1).unwrap());
            std::fs::write(marker.with_extension("disconnect"),"received").unwrap();
            break;
        }
    }
}
"##;
/// Compile a real Rust/MSVC executable and PDB with the installed compiler; no toolchain install.
pub fn program(directory: &Path) -> (PathBuf, PathBuf) {
    let source = directory.join("debug_probe.rs");
    std::fs::write(&source,"//! Native Rust debug acceptance target.\n#[inline(never)]\nfn calculate(value:i32)->i32 {\n    let doubled = value * 2;\n    doubled + 1\n}\nfn main() {\n    let result = calculate(5);\n    println!(\"RUST_RESULT:{result}\");\n    for _ in 0..1000 { std::thread::sleep(std::time::Duration::from_millis(10)); }\n}\n").unwrap();
    let binary = directory.join("debug_probe.exe");
    let output = std::process::Command::new("rustc")
        .args(["--edition", "2024", "-g", "-C", "opt-level=0", "-o"])
        .arg(&binary)
        .arg(&source)
        .output()
        .expect("existing Rust compiler required");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (source, binary)
}
