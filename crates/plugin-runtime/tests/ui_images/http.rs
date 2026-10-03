//! Loopback HTTP fixtures expose requests and deliberately delayed bodies without external services.
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub(super) struct HttpServer {
    address: String,
    requests: Arc<AtomicUsize>,
    ambient: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl HttpServer {
    pub fn new() -> Self {
        Self::start(|mut stream, path, _| {
            let response = match path {
                "/missing" => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
                "/redirect" => "HTTP/1.1 302 Found\r\nLocation: /target\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
                "/large" => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", 8 * 1024 * 1024 + 1),
                _ => "HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\nfixture-image".to_owned(),
            };
            let _ = stream.write_all(response.as_bytes());
        })
    }
    /// All accepts and test response threads are joined; only the first delayed body needs a release.
    fn start(handler: impl Fn(TcpStream, &str, usize) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let ambient = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (count, seen, stopped) = (requests.clone(), ambient.clone(), stop.clone());
        let handler = Arc::new(handler);
        let thread = thread::spawn(move || {
            let mut clients = Vec::new();
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let index = count.fetch_add(1, Ordering::AcqRel);
                        let handler = handler.clone();
                        let seen = seen.clone();
                        clients.push(thread::spawn(move || {
                            stream
                                .set_read_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            stream
                                .set_write_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            let mut header = Vec::new();
                            let mut byte = [0; 1];
                            while header.len() < 8192 && !header.ends_with(b"\r\n\r\n") {
                                if stream.read(&mut byte).unwrap_or(0) == 0 {
                                    return;
                                }
                                header.push(byte[0]);
                            }
                            let header = String::from_utf8_lossy(&header);
                            let lowered = header.to_ascii_lowercase();
                            if lowered.contains("\r\nauthorization:")
                                || lowered.contains("\r\ncookie:")
                            {
                                seen.store(true, Ordering::Release);
                            }
                            let path = header.split_whitespace().nth(1).unwrap_or("/");
                            handler(stream, path, index);
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                }
            }
            for client in clients {
                client.join().unwrap();
            }
        });
        Self {
            address,
            requests,
            ambient,
            stop,
            thread: Some(thread),
        }
    }
    pub fn url(&self, path: &str) -> String {
        format!("http://{}/{path}", self.address)
    }
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::Acquire)
    }
    pub fn ambient_headers(&self) -> bool {
        self.ambient.load(Ordering::Acquire)
    }
}
impl Drop for HttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

pub(super) struct PausedServer {
    server: HttpServer,
    accepted: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    paused: usize,
}
impl PausedServer {
    /// First request stalls after partial body; later requests immediately return distinguishable bytes.
    pub fn new() -> Self {
        Self::with_paused(1)
    }
    /// Multiple paused requests separately exercise document closure and instance retirement.
    pub fn with_paused(paused: usize) -> Self {
        let (accepted_tx, accepted) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let server = HttpServer::start(move |mut stream, _, index| {
            if index < paused {
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nold",
                );
                let _ = accepted_tx.send(());
                let _ = release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(40));
                let _ = stream.write_all(b"-image");
            } else {
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nfresh-image");
            }
        });
        Self {
            server,
            accepted,
            release,
            paused,
        }
    }
    pub fn url(&self) -> String {
        self.server.url("delayed")
    }
    /// Drive the same public heartbeat as the host while a request waits for a global worker permit.
    pub fn accepted(&self, manager: &mut plugin_runtime::Manager) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            manager.poll();
            match self.accepted.try_recv() {
                Ok(()) => return,
                Err(mpsc::TryRecvError::Empty) => {}
                Err(error) => panic!("fixture accept channel closed: {error}"),
            }
            assert!(
                std::time::Instant::now() < deadline,
                "image worker did not reach the controlled HTTP fixture"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    pub fn release(&self) {
        let _ = self.release.send(());
    }
}
impl Drop for PausedServer {
    fn drop(&mut self) {
        for _ in 0..self.paused {
            self.release();
        }
    }
}

pub(super) struct GatedServer {
    server: HttpServer,
    gate: Arc<(Mutex<bool>, Condvar)>,
}
impl GatedServer {
    /// All accepted bodies stay pending so the publicly observable connection count exposes the pool bound.
    pub fn new() -> Self {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let producer_gate = gate.clone();
        let server = HttpServer::start(move |mut stream, _, _| {
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n");
            let (lock, wake) = &*producer_gate;
            let released = lock.lock().unwrap();
            drop(
                wake.wait_timeout_while(released, Duration::from_secs(10), |released| !*released)
                    .unwrap(),
            );
            let _ = stream.write_all(b"fixture-image");
        });
        Self { server, gate }
    }
    pub fn url(&self) -> String {
        self.server.url("gated")
    }
    pub fn requests(&self) -> usize {
        self.server.requests()
    }
    pub fn release(&self) {
        let (lock, wake) = &*self.gate;
        *lock.lock().unwrap() = true;
        wake.notify_all();
    }
}
impl Drop for GatedServer {
    fn drop(&mut self) {
        self.release();
    }
}
