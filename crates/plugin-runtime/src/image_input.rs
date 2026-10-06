//! Native image bytes stay in scoped host resources; guests receive bounded metadata only.
use plugin_protocol::api;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// These quotas apply before native image bytes are delivered to a WASM instance.
pub const IMAGE_INPUT_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const IMAGE_INPUT_MAX_BATCH: usize = 8;
pub const IMAGE_INPUT_MAX_BATCH_BYTES: usize = 32 * 1024 * 1024;
pub const IMAGE_INPUT_MAX_RESIDENT_BYTES: usize = 64 * 1024 * 1024;
pub const IMAGE_INPUT_TIMEOUT_MS: u32 = 30_000;

/// The native caller validates encoded content and supplies its actual format, never a guest-provided path.
#[derive(Clone)]
pub struct HostImageInput {
    pub format: api::ImageFormat,
    pub bytes: Arc<Vec<u8>>,
}

/// Clipboard capture needs its separate permission; Drop authorizes only the files explicitly offered by the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostImageOrigin {
    Clipboard,
    Drop,
}

/// Immutable request payload pins its original target and bytes, including after the preview advances.
/// Only a validated native offer can create this resource; the guest cannot alter its pixels or target directory.
pub struct ImageInputResource {
    pub input: api::ImageInput,
    pub document: api::DocumentVersion,
    pub selection: api::TextRange,
    pub bytes: Arc<Vec<u8>>,
    _reservation: Reservation,
}

/// Reservations cover queued resources and accepted writer payloads until their final owner drops.
struct Reservation {
    budget: Arc<AtomicUsize>,
    bytes: usize,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

impl ImageInputResource {
    /// Reserving before insertion prevents concurrent offers from exceeding one manager's byte quota.
    pub(crate) fn new(
        input: api::ImageInput,
        document: api::DocumentVersion,
        selection: api::TextRange,
        bytes: Arc<Vec<u8>>,
        budget: Arc<AtomicUsize>,
    ) -> Result<Self, api::Failure> {
        let length = bytes.len();
        budget
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(length)
                    .filter(|total| *total <= IMAGE_INPUT_MAX_RESIDENT_BYTES)
            })
            .map_err(|_| {
                api::Failure::new(
                    api::ErrorCode::LimitExceeded,
                    "Pending image input quota exceeded",
                )
            })?;
        Ok(Self {
            input,
            document,
            selection,
            bytes,
            _reservation: Reservation {
                budget,
                bytes: length,
            },
        })
    }
}

/// Batches are finite even when no guest consumes them; native captures must apply the same limits before enqueue.
pub(crate) fn validate_batch(images: &[HostImageInput]) -> Result<(), api::Failure> {
    if images.is_empty() {
        return Err(api::Failure::new(
            api::ErrorCode::InvalidRequest,
            "Image input batch is empty",
        ));
    }
    if images.len() > IMAGE_INPUT_MAX_BATCH
        || images
            .iter()
            .any(|image| image.bytes.is_empty() || image.bytes.len() > IMAGE_INPUT_MAX_BYTES)
        || images.iter().map(|image| image.bytes.len()).sum::<usize>() > IMAGE_INPUT_MAX_BATCH_BYTES
    {
        return Err(api::Failure::new(
            api::ErrorCode::LimitExceeded,
            "Image input batch quota exceeded",
        ));
    }
    Ok(())
}

/// A single safe basename and canonical suffix keep this API confined to document-sibling attachments.
pub(crate) fn validate_name(name: &str, format: api::ImageFormat) -> Result<(), api::Failure> {
    let base = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    let device = matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) || base
        .strip_prefix("COM")
        .or_else(|| base.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        });
    if name.is_empty()
        || name.len() > 255
        || name.chars().any(char::is_control)
        || name.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        || name.ends_with([' ', '.'])
        || device
        || !name.rsplit_once('.').is_some_and(|(stem, extension)| {
            !stem.is_empty() && extension.eq_ignore_ascii_case(format.extension())
        })
    {
        return Err(api::Failure::new(
            api::ErrorCode::InvalidPath,
            "Image name must be a safe basename with its actual format suffix",
        ));
    }
    Ok(())
}
