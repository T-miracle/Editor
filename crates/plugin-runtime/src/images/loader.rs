//! Permission-aware URI resolution and bounded IO run only on image producer threads.
use super::*;
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

#[derive(Clone)]
pub(super) enum Uri {
    Local(String),
    Http(String),
}

/// Authorize before any filesystem or network access; invalid sources become per-node failures.
pub(super) fn authorize(identity: &Identity) -> Result<Uri, api::Failure> {
    let source = &identity.uri;
    if let Some((scheme, tail)) = source.split_once("://") {
        if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
            return Err(failure(
                api::ErrorCode::InvalidPath,
                "Unsupported image URI scheme",
            ));
        }
        if !identity.network {
            return Err(failure(
                api::ErrorCode::PermissionDenied,
                "network.images grant required",
            ));
        }
        // Scheme names are ASCII-insensitive; keep the original URI in the published identity.
        let url = format!("{}://{tail}", scheme.to_ascii_lowercase());
        let uri: ureq::http::Uri = url
            .parse()
            .map_err(|_| failure(api::ErrorCode::InvalidPath, "Invalid image URL"))?;
        let authority = uri
            .authority()
            .ok_or_else(|| failure(api::ErrorCode::InvalidPath, "Image URL requires a host"))?;
        if authority.as_str().contains('@') || source.contains('#') || uri.host().is_none() {
            return Err(failure(
                api::ErrorCode::InvalidPath,
                "Image URLs cannot contain credentials or fragments",
            ));
        }
        return Ok(Uri::Http(url));
    }
    if !identity.local {
        return Err(failure(
            api::ErrorCode::PermissionDenied,
            "workspace.read grant required",
        ));
    }
    let decoded = decode_path(source)?;
    if decoded.is_empty()
        || decoded.starts_with('/')
        || decoded.contains(['\\', ':'])
        || decoded.chars().any(char::is_control)
        || source.contains(['?', '#'])
        || Path::new(&decoded).is_absolute()
        || decoded.split('/').any(device_name)
    {
        return Err(failure(
            api::ErrorCode::InvalidPath,
            "Image path must be a document-relative workspace path",
        ));
    }
    Ok(Uri::Local(decoded))
}

/// Decode percent escapes once before checking Windows device names, separators and alternate streams.
fn decode_path(source: &str) -> Result<String, api::Failure> {
    let bytes = source.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let digit = |byte: u8| (byte as char).to_digit(16).map(|value| value as u8);
            let pair = bytes
                .get(index + 1..index + 3)
                .ok_or_else(|| failure(api::ErrorCode::InvalidPath, "Invalid image path escape"))?;
            let high = digit(pair[0])
                .ok_or_else(|| failure(api::ErrorCode::InvalidPath, "Invalid image path escape"))?;
            let low = digit(pair[1])
                .ok_or_else(|| failure(api::ErrorCode::InvalidPath, "Invalid image path escape"))?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded)
        .map_err(|_| failure(api::ErrorCode::InvalidPath, "Image path is not UTF-8"))
}

/// Reject DOS device spellings on every platform so a portable declaration has one meaning.
fn device_name(segment: &str) -> bool {
    let name = segment
        .trim_end_matches([' ', '.'])
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(
        name.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || name
        .strip_prefix("COM")
        .or_else(|| name.strip_prefix("LPT"))
        .is_some_and(|number| {
            matches!(
                number,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

pub(super) fn load(
    identity: &Identity,
    uri: &Uri,
    control: &Control,
    budget: Arc<AtomicUsize>,
) -> Result<Loaded, api::Failure> {
    control.check()?;
    match uri {
        Uri::Local(path) => {
            let root = Path::new(&identity.workspace)
                .canonicalize()
                .map_err(io_failure)?;
            let document = root.join(&identity.source.path);
            let directory = document
                .parent()
                .ok_or_else(|| {
                    failure(api::ErrorCode::InvalidPath, "Image source has no directory")
                })?
                .canonicalize()
                .map_err(io_failure)?;
            if !directory.starts_with(&root) {
                return Err(failure(
                    api::ErrorCode::PermissionDenied,
                    "Image document is outside its workspace",
                ));
            }
            let target = directory.join(path).canonicalize().map_err(io_failure)?;
            if !target.starts_with(&root) {
                return Err(failure(
                    api::ErrorCode::PermissionDenied,
                    "Image path escapes its workspace",
                ));
            }
            let metadata = target.metadata().map_err(io_failure)?;
            if !metadata.is_file() {
                return Err(failure(
                    api::ErrorCode::InvalidPath,
                    "Image resource is not a file",
                ));
            }
            if metadata.len() > MAX_BYTES as u64 {
                return Err(failure(
                    api::ErrorCode::LimitExceeded,
                    "Image exceeds 8 MiB",
                ));
            }
            let mut file = File::open(&target).map_err(io_failure)?;
            read(&mut file, control, budget)
        }
        Uri::Http(url) => {
            // No environment proxy, cookie jar, authentication headers or redirects enter this request.
            let config = ureq::Agent::config_builder()
                .proxy(None)
                .max_redirects(0)
                .http_status_as_error(false)
                .timeout_global(Some(
                    control.deadline.saturating_duration_since(Instant::now()),
                ))
                .timeout_resolve(Some(Duration::from_secs(10)))
                .timeout_connect(Some(Duration::from_secs(10)))
                .build();
            let agent = ureq::Agent::with_parts(
                config,
                ureq::unversioned::transport::DefaultConnector::default(),
                BoundedResolver {
                    cancelled: control.cancelled.clone(),
                    deadline: control.deadline,
                },
            );
            let mut response = agent.get(url).call().map_err(http_failure)?;
            if !response.status().is_success() {
                return Err(failure(
                    api::ErrorCode::OperationFailed,
                    format!("Image HTTP {}", response.status().as_u16()),
                ));
            }
            if response
                .headers()
                .get("content-length")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .is_some_and(|bytes| bytes > MAX_BYTES as u64)
            {
                return Err(failure(
                    api::ErrorCode::LimitExceeded,
                    "Image exceeds 8 MiB",
                ));
            }
            read(&mut response.body_mut().as_reader(), control, budget)
        }
    }
}

/// The default timed resolver creates an uncounted helper thread per lookup. Keep DNS in the
/// already-counted producer: a slow OS lookup may finish later, but cannot create more workers
/// or connect after its consumer has timed out/retired. Only the existing public ureq seam is used.
#[derive(Debug)]
struct BoundedResolver {
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}
impl ureq::unversioned::resolver::Resolver for BoundedResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        use ureq::unversioned::{
            resolver::DefaultResolver,
            transport::{NextTimeout, time::Duration as ResolverDuration},
        };
        self.check()?;
        let started = Instant::now();
        let result = DefaultResolver::default().resolve(
            uri,
            config,
            NextTimeout {
                after: ResolverDuration::NotHappening,
                reason: timeout.reason,
            },
        );
        self.check()?;
        if !timeout.after.is_not_happening() && started.elapsed() >= *timeout.after {
            return Err(ureq::Error::Timeout(timeout.reason));
        }
        result
    }
}
impl BoundedResolver {
    fn check(&self) -> Result<(), ureq::Error> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(ureq::Error::Other(Box::new(failure(
                api::ErrorCode::Cancelled,
                "Image consumer retired",
            ))))
        } else if Instant::now() >= self.deadline {
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        } else {
            Ok(())
        }
    }
}

/// Incremental charging includes queued worker results; oversized streams never allocate beyond the quota.
fn read(
    reader: &mut impl Read,
    control: &Control,
    budget: Arc<AtomicUsize>,
) -> Result<Loaded, api::Failure> {
    let mut reservation = Reservation::new(budget);
    let mut bytes = Vec::new();
    let mut buffer = [0; 32768];
    loop {
        control.check()?;
        let count = reader.read(&mut buffer).map_err(io_failure)?;
        control.check()?;
        if count == 0 {
            return Ok(Loaded {
                bytes: Arc::new(bytes),
                reservation,
            });
        }
        if bytes.len() + count > MAX_BYTES {
            return Err(failure(
                api::ErrorCode::LimitExceeded,
                "Image exceeds 8 MiB",
            ));
        }
        reservation.add(count)?;
        bytes
            .try_reserve_exact(count)
            .map_err(|_| failure(api::ErrorCode::LimitExceeded, "Cannot reserve image bytes"))?;
        bytes.extend_from_slice(&buffer[..count]);
    }
}

fn failure(code: api::ErrorCode, message: impl Into<String>) -> api::Failure {
    api::Failure::new(code, message)
}
fn io_failure(error: io::Error) -> api::Failure {
    let code = match error.kind() {
        io::ErrorKind::NotFound => api::ErrorCode::NotFound,
        io::ErrorKind::PermissionDenied => api::ErrorCode::PermissionDenied,
        io::ErrorKind::TimedOut => api::ErrorCode::TimedOut,
        _ => api::ErrorCode::OperationFailed,
    };
    // Keep diagnostics bounded and avoid exposing arbitrary host paths or authenticated endpoints.
    failure(code, format!("Image IO failed: {}", error.kind()))
}
fn http_failure(error: ureq::Error) -> api::Failure {
    let code = if matches!(error, ureq::Error::Timeout(_)) {
        api::ErrorCode::TimedOut
    } else {
        api::ErrorCode::OperationFailed
    };
    failure(
        code,
        if code == api::ErrorCode::TimedOut {
            "Image loading exceeded 30 seconds"
        } else {
            "Image HTTP request failed"
        },
    )
}
