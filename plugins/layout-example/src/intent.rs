//! Display intent belongs to the guest; host CAS records and watches provide scoped convergence.
use plugin_protocol::api;

#[derive(Default)]
pub(super) struct Intent {
    key: Option<api::PreferenceKey>,
    watch: Option<api::ResourceHandle>,
    value: api::PreferenceValue,
}

impl Intent {
    /// File identity changes retain the same file-type preference; leaving the scope releases its watch.
    pub(super) fn bind(
        &mut self,
        file: Option<&api::FileContext>,
    ) -> Result<Option<u8>, api::Failure> {
        let key = file.map(|file| api::PreferenceKey {
            file_type: file.file_type.clone(),
            name: "display".into(),
        });
        if key == self.key {
            return Ok(None);
        }
        self.close()?;
        let Some(key) = key else {
            return Ok(None);
        };
        let read = api::guest::read_preference(key.clone(), true)?;
        let mode = mode(&read.value)?;
        self.key = Some(key);
        self.watch = read.subscription;
        self.value = read.value;
        Ok(Some(mode))
    }

    /// A concurrent winner is applied instead of blindly retrying and overwriting its newer intent.
    pub(super) fn select(&mut self, requested: u8) -> Result<u8, api::Failure> {
        let Some(key) = self.key.clone() else {
            return Ok(requested);
        };
        self.value = match api::guest::write_preference(
            key.clone(),
            self.value.revision,
            serde_json::json!(requested),
        ) {
            Ok(value) => value,
            Err(error) if error.code == api::ErrorCode::Conflict => {
                api::guest::read_preference(key, false)?.value
            }
            Err(error) => return Err(error),
        };
        mode(&self.value)
    }

    /// Only this live watch can update the choice; stale subscription notifications add no authority.
    pub(super) fn changed(
        &mut self,
        subscription: &api::ResourceHandle,
        key: &api::PreferenceKey,
        value: &api::PreferenceValue,
    ) -> Result<Option<u8>, api::Failure> {
        if self.watch.as_ref() != Some(subscription)
            || self.key.as_ref() != Some(key)
            || value.revision <= self.value.revision
        {
            return Ok(None);
        }
        let selected = mode(value)?;
        self.value = value.clone();
        Ok(Some(selected))
    }

    /// Lifecycle withdrawal explicitly revokes watches; runtime retirement is the final cleanup guard.
    pub(super) fn close(&mut self) -> Result<(), api::Failure> {
        if let Some(watch) = self.watch.take() {
            api::guest::close_resource(watch)?;
        }
        self.key = None;
        self.value = Default::default();
        Ok(())
    }
}

/// Corrupt/unknown intent is preserved and reported; it never silently replaces an existing record.
fn mode(value: &api::PreferenceValue) -> Result<u8, api::Failure> {
    value.validate()?;
    match value.data.as_ref() {
        None => Ok(0),
        Some(value) => value
            .as_u64()
            .filter(|mode| *mode <= 2)
            .map(|mode| mode as u8)
            .ok_or_else(|| {
                api::Failure::new(api::ErrorCode::InvalidState, "Unknown display preference")
            }),
    }
}
