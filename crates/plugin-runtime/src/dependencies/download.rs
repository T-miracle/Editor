//! A bounded HTTP producer never writes cache files; cancellation drops its consumer immediately.
use super::*;
use std::{
    sync::{atomic::AtomicUsize, mpsc},
    time::Duration,
};
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Interrupted DNS/TLS reads may finish later, but cannot publish files or create unbounded worker threads.
pub(super) fn download(url: &str, file: &mut File, control: &InstallControl) -> anyhow::Result<()> {
    IN_FLIGHT
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < 4).then_some(count + 1)
        })
        .map_err(|_| anyhow::anyhow!("Download workers busy; retry shortly"))?;
    let permit = Permit;
    let url = url.to_owned();
    let producer_control = control.clone();
    let (tx, rx) = mpsc::sync_channel(2);
    std::thread::Builder::new()
        .name("plugin-dependency-download".into())
        .spawn(move || {
            let _permit = permit;
            let result = receive(&url, &tx, &producer_control);
            let _ = tx.send(
                result
                    .map(|_| Vec::new())
                    .map_err(|error| format!("{error:#}")),
            );
        })?;
    let mut total = 0;
    loop {
        control.check()?;
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(bytes)) if bytes.is_empty() => return control.check(),
            Ok(Ok(bytes)) => {
                total += bytes.len();
                anyhow::ensure!(
                    total <= 128 * 1024 * 1024,
                    "Dependency download quota exceeded"
                );
                file.write_all(&bytes)?;
            }
            Ok(Err(error)) => anyhow::bail!("Dependency download failed: {error}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => anyhow::bail!("Dependency download worker stopped"),
        }
    }
}

fn receive(
    url: &str,
    tx: &mpsc::SyncSender<Result<Vec<u8>, String>>,
    control: &InstallControl,
) -> anyhow::Result<()> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .timeout_resolve(Some(Duration::from_secs(10)))
        .timeout_connect(Some(Duration::from_secs(10)))
        // HTTPS cannot downgrade on redirect. Local fixture URLs do not follow redirects at all.
        .https_only(url.starts_with("https://"))
        .max_redirects(if url.starts_with("https://") { 5 } else { 0 })
        .build()
        .new_agent();
    let mut response = agent.get(url).call()?;
    anyhow::ensure!(response.status().is_success(), "HTTP {}", response.status());
    let mut reader = response.body_mut().as_reader();
    let mut buffer = [0; 32768];
    loop {
        control.check()?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        tx.send(Ok(buffer[..count].to_vec()))
            .map_err(|_| anyhow::anyhow!("Download consumer closed"))?;
    }
}
