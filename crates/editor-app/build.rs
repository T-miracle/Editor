//! Embeds Windows product resources and reserves the native dock layout's stack.

fn main() {
    println!("cargo:rerun-if-changed=assets/branding/nanobug.rc");
    println!("cargo:rerun-if-changed=assets/branding/nanobug.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Windows file properties use the same numeric version as the Cargo package.
        let version_definitions = ["MAJOR", "MINOR", "PATCH"].map(|part| {
            let version = std::env::var(format!("CARGO_PKG_VERSION_{part}"))
                .expect("Cargo package version must be available to the resource compiler");
            format!("APP_VERSION_{part}={version}")
        });
        // GPUI loads resource ID 1 for its native window and taskbar icon.
        // Fail a Windows build if resources cannot be compiled instead of shipping an unbranded EXE.
        embed_resource::compile_for(
            "assets/branding/nanobug.rc",
            ["editor-app"],
            version_definitions,
        )
        .manifest_required()
        .expect("failed to embed Nanobug Windows resources");

        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            // The nested dock tree exceeds Windows' default main-thread stack during layout.
            // Reserve address space; Windows commits pages only as the application uses them.
            println!("cargo:rustc-link-arg-bin=editor-app=/STACK:8388608");
        }
    }
}
