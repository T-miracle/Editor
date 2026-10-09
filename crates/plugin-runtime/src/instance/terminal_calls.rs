//! Native terminal input, geometry and closure retain the guest's original resource authority.
use super::process_calls::process_failure;
use super::*;
use api::{ErrorCode, Failure};
use plugin_protocol::process::Operation;

impl Instance {
    /// Workspace closure uses ordinary resource release, retaining real tree/EOF cleanup observation.
    pub(crate) fn release_presented(&mut self, handle: &api::ResourceHandle) {
        let _ = self
            .store
            .data_mut()
            .resource_request(api::Operation::CloseResource {
                handle: handle.clone(),
            });
    }
    /// Stop admission keeps the guest root through real EOF and applies the existing PTY interrupt.
    pub(crate) fn exit_presented(
        &mut self,
        handle: &api::ResourceHandle,
        mode: process::ExitMode,
    ) -> Result<(), Failure> {
        let state = self.store.data_mut();
        state.presented_id(handle)?;
        state
            .process_request(Operation::RequestExit {
                handle: handle.clone(),
                mode,
            })
            .map(|_| ())
    }
    /// A native view may address only an explicitly presented, still-authenticated process resource.
    pub(crate) fn has_presented_terminal(&self, handle: &api::ResourceHandle) -> bool {
        let state = self.store.data();
        state.roots.resolve(handle).is_ok() && state.host_resources.terminals.contains(handle)
    }
    /// Revalidate lifetime and quotas on every native input; presentation itself grants no permission.
    pub(crate) fn write_presented(
        &mut self,
        handle: &api::ResourceHandle,
        bytes: &[u8],
    ) -> Result<(), Failure> {
        let state = self.store.data_mut();
        let id = state.presented_id(handle)?;
        if !state.host_resources.terminals.interactive(handle) || !state.processes.is_pty(id) {
            return Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Decoded terminal output is read-only",
            ));
        }
        if bytes.len() > 65536 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Terminal input quota exceeded",
            ));
        }
        state.processes.write(id, bytes).map_err(process_failure)
    }
    /// Resize follows the exact same authentication as literal input and the existing settle policy.
    pub(crate) fn resize_presented(
        &mut self,
        handle: &api::ResourceHandle,
        columns: u16,
        rows: u16,
    ) -> Result<(), Failure> {
        let state = self.store.data_mut();
        let id = state.presented_id(handle)?;
        if !state.host_resources.terminals.interactive(handle) || !state.processes.is_pty(id) {
            return Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Decoded terminal output is read-only",
            ));
        }
        state
            .processes
            .resize(id, columns, rows)
            .map_err(process_failure)
    }
}

impl State {
    fn presented_id(&self, handle: &api::ResourceHandle) -> Result<u64, Failure> {
        let id = self.process_id(handle)?;
        if !self.host_resources.terminals.contains(handle)
            || self
                .plugin_services
                .resources
                .get(&handle.resource)
                .is_some_and(|(_, context)| {
                    context
                        .lifetimes
                        .iter()
                        .any(|alive| !alive.load(std::sync::atomic::Ordering::Acquire))
                })
        {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Terminal source was revoked",
            ));
        }
        Ok(id)
    }
}
