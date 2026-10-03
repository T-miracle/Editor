//! Headless regression harness for the production language-server connection.
//! Run: cargo test -p editor-app --example language_diagnostics local_rust_semantic_diagnostics -- --ignored

// Use production modules so the smoke test remains independent of window setup.
#[allow(dead_code)]
#[path = "../src/language"]
mod language {
    mod navigation;
    mod toolchains;
}
#[allow(dead_code)]
#[path = "../src/sdk_export.rs"]
mod sdk_export;

/// This example is a test entry point; its regression fixtures own temporary workspaces.
fn main() {}
