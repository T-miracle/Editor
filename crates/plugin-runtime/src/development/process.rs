//! Host-owned build/development processes reuse the runtime's bounded pipes and tree cleanup.
pub use plugin_protocol::process::{Stream, Update};
use std::{collections::BTreeMap, path::Path};

/// One supervised process, independent of a plugin provider or shell command language.
/// Dropping it terminates its owned process tree and releases pipes.
pub struct HostProcess {
    processes: crate::process::Processes,
    id: u64,
}
impl HostProcess {
    /// Start an executable with explicit argv/cwd and host-selected environment overrides.
    /// The caller must establish workspace trust before invoking this host-only interface.
    pub fn spawn(
        program: &Path,
        args: &[String],
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) -> anyhow::Result<Self> {
        let mut processes = crate::process::Processes::default();
        let id = processes.spawn_stdio(program, args, cwd, env)?;
        Ok(Self { processes, id })
    }
    /// Drain bounded native output and real exit events; output text never determines completion.
    pub fn poll(&mut self) -> anyhow::Result<Vec<Update>> {
        Ok(self
            .processes
            .poll_native()?
            .into_iter()
            .filter_map(|(id, event)| (id == self.id).then_some(event))
            .collect())
    }
    /// Send a bounded control message to the development instance's stdin.
    pub fn write(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.processes.write(self.id, bytes)
    }
    /// Terminate the whole owned process tree; polling still observes final native completion.
    pub fn terminate(&mut self) -> anyhow::Result<()> {
        self.processes.terminate(self.id)
    }
}
