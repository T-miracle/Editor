//! Harmless installer fixture: records execution and copies itself only to an explicit test directory.
fn main() -> std::io::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let target = std::path::Path::new(&args[0]);
    if args[2] == "hold" {
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let _held = options.open(target.join("held.lock"))?;
        std::fs::write(
            std::path::Path::new(&args[1]).with_extension("child"),
            b"ready",
        )?;
        std::thread::sleep(std::time::Duration::from_secs(30));
        return Ok(());
    }
    let previous = std::path::Path::new(&args[1]).exists();
    std::fs::create_dir_all(target)?;
    std::fs::write(&args[1], target.display().to_string())?;
    let payload = if args[2] == "install-lsp" {
        std::path::Path::new(&args[3]).join("server.exe")
    } else {
        std::env::current_exe()?
    };
    std::fs::copy(payload, target.join("tool.exe"))?;
    // A descendant deliberately holds a staged file after its root exits to test the host's job barrier.
    if !previous && args[2].starts_with("child-") {
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command.args([&args[0], &args[1], "hold"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let _child = command.spawn()?;
        let ready = std::path::Path::new(&args[1]).with_extension("child");
        while !ready.exists() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if args[2] == "child-cancel" {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }
    // Failure intentionally leaves partial managed output for the host's cleanup contract.
    if args[2] == "fail" {
        std::process::exit(7);
    }
    if args[2] == "wait" {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
    if !previous && args[2] == "fail-once" {
        std::process::exit(7);
    }
    if !previous && args[2] == "wait-once" {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
    Ok(())
}
