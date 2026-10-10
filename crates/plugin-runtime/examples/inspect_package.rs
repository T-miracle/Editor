//! Registry admission validates untrusted ZIPs through Package without instantiating or running WASM.
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Usage: inspect_package <plugin.zip>"))?;
    let package = plugin_runtime::Package::read(Path::new(&path))?;
    // JSON is inert output; no Manager, process, WASM instance or install directory is created.
    println!("{}", serde_json::to_string(&package.manifest)?);
    Ok(())
}
