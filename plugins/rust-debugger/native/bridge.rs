//! A controlled service bridges byte-exact DAP between host pipes and CodeLLDB's loopback socket.
//! It launches one fixed adapter from verified dependency paths; the host job owns all descendants.
use std::{
    io::{self, Read, Write},
    net::{Shutdown, TcpListener},
    process::{Child, Command, Stdio},
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
};

/// Even launch/connection failures close the adapter. The enclosing host job closes its target tree.
struct Adapter(Child);
impl Drop for Adapter {
    fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
}

/// Stdout contains only DAP bytes; diagnostics use the separately controlled stderr stream.
fn main() {
    if let Err(error) = run() { eprintln!("Debug transport failed: {error}"); std::process::exit(1); }
}

/// Connect within a bounded deadline, then forward under pipe/socket backpressure with fixed buffers.
fn run() -> io::Result<()> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 2 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "Expected adapter and liblldb paths")); }
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let mut command = Command::new(&arguments[0]);
    command.arg("--connect").arg(listener.local_addr()?.port().to_string())
        .arg("--liblldb").arg(&arguments[1])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit());
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        // Inherit the host's job; creating a separate console must not escape tree cleanup.
        command.creation_flags(0x08000000);
    }
    let mut adapter = Adapter(command.spawn()?);
    let deadline = Instant::now() + Duration::from_secs(15);
    let (mut socket, _) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if adapter.0.try_wait()?.is_some() { return Err(io::Error::other("Adapter exited before connecting")); }
                if Instant::now() >= deadline { return Err(io::Error::new(io::ErrorKind::TimedOut, "Adapter connection timed out")); }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    };
    socket.set_nodelay(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(200)))?;
    let mut incoming = socket.try_clone()?;
    let ended = Arc::new(AtomicBool::new(false));
    let input_ended = ended.clone();
    std::thread::spawn(move || {
        let _ = io::copy(&mut io::stdin().lock(), &mut incoming);
        input_ended.store(true, Ordering::Release);
        let _ = incoming.shutdown(Shutdown::Both);
    });
    let mut output = io::stdout().lock();
    let mut bytes = [0u8; 8192];
    loop {
        match socket.read(&mut bytes) {
            Ok(0) => break,
            Ok(count) => { output.write_all(&bytes[..count])?; output.flush()?; }
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                if ended.load(Ordering::Acquire) || adapter.0.try_wait()?.is_some() { break; }
            }
            Err(error) => return Err(error),
        }
    }
    let _ = socket.shutdown(Shutdown::Both);
    Ok(())
}
