//! A revocable guest binding handles opaque CAS/watch transport; plugins retain their own schemas.
use super::*;

#[derive(Default)]
pub struct PreferenceBinding {
    key: Option<PreferenceKey>,
    watch: Option<ResourceHandle>,
    value: PreferenceValue,
}

impl PreferenceBinding {
    /// Bind one file-type/name in the caller's owning workspace, closing the old watch on change.
    /// Returns true when the scope changed; callers decode `value()` and provide their own defaults.
    pub fn bind(&mut self, key: Option<PreferenceKey>) -> Result<bool, Failure> {
        if self.key == key {
            return Ok(false);
        }
        self.close()?;
        if let Some(key) = key {
            let read = guest::read_preference(key.clone(), true)?;
            self.key = Some(key);
            self.watch = read.subscription;
            self.value = read.value;
        }
        Ok(true)
    }

    /// Inspect opaque intent; it contains no document identity, text, caret or native resource state.
    pub fn value(&self) -> &PreferenceValue {
        &self.value
    }

    /// Write the caller's schema using the last observed revision; a concurrent winner is returned
    /// instead of being overwritten by a blind retry. An unbound writer is an InvalidState error.
    pub fn write(&mut self, data: serde_json::Value) -> Result<&PreferenceValue, Failure> {
        let key = self
            .key
            .clone()
            .ok_or_else(|| Failure::new(ErrorCode::InvalidState, "Preference is not bound"))?;
        self.value = match guest::write_preference(key.clone(), self.value.revision, data) {
            Ok(value) => value,
            Err(error) if error.code == ErrorCode::Conflict => {
                guest::read_preference(key, false)?.value
            }
            Err(error) => return Err(error),
        };
        Ok(&self.value)
    }

    /// Accept only a newer notification from this current owned watch; stale events do nothing.
    pub fn changed(&mut self, event: &Notification) -> Result<bool, Failure> {
        if let Notification::SubscriptionFailed {
            subscription,
            error,
        } = event
            && self.watch.as_ref() == Some(subscription)
        {
            // Runtime has already revoked this owned watch; a later bind must read again.
            self.watch = None;
            self.key = None;
            return Err(error.clone());
        }
        let Notification::PreferenceChanged {
            subscription,
            key,
            value,
        } = event
        else {
            return Ok(false);
        };
        if self.watch.as_ref() != Some(subscription)
            || self.key.as_ref() != Some(key)
            || value.revision <= self.value.revision
        {
            return Ok(false);
        }
        value.validate()?;
        self.value = value.clone();
        Ok(true)
    }

    /// Withdrawal releases the watch. Runtime retirement remains the final lifecycle cleanup guard.
    pub fn close(&mut self) -> Result<(), Failure> {
        if let Some(watch) = self.watch.take() {
            match guest::close_resource(watch) {
                Ok(()) => {}
                // An already-revoked owned watch needs no second resource close.
                Err(error) if error.code == ErrorCode::InvalidHandle => {}
                Err(error) => return Err(error),
            }
        }
        self.key = None;
        self.value = Default::default();
        Ok(())
    }
}
