//! Independently compiled native fixture: echoes lines and a large final burst before exit.
use std::io::{BufRead, Write};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "short") {
        std::io::stdout().write_all(&vec![b's'; 16384]).unwrap();
        std::fs::write(&args[1], b"done").unwrap();
        return;
    }
    if args.first().is_some_and(|arg| arg == "marker") {
        std::fs::write(&args[1], b"executed").unwrap();
        return;
    }
    if args.first().is_some_and(|arg| arg == "tree") {
        // Spawn before doing anything else: the owning job must already cover this descendant.
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("wait")
            .spawn()
            .unwrap();
        std::fs::write(&args[1], child.id().to_string()).unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    if args.first().is_some_and(|arg| arg == "wait") {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    if args.first().is_some_and(|arg| arg == "interactive") {
        // Display parsed argv so the PTY test also covers spaces, literal quotes, and trailing slashes.
        println!("argument:{}", args.get(1).unwrap());
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        println!("received:{}", line.trim_end());
        return;
    }
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        if line == "exit" {
            break;
        }
        println!("{line}");
    }
    std::io::stdout().write_all(&vec![b'x'; 131072]).unwrap();
    eprintln!("separate stderr");
}
