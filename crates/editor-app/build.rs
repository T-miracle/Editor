// The nested GPUI dock tree exceeds Windows' one-megabyte main-thread stack
// during layout and paint. Reserve stack address space; Windows commits pages
// only as the application uses them.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rustc-link-arg-bin=editor-app=/STACK:8388608");
    }
}
