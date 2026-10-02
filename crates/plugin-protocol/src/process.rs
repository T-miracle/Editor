//! Native execution declarations and typed process events; no terminal parsing belongs to the host.
use serde::{Deserialize, Serialize};

/// Installation approves this exact program and argument vector. Calls supply only its map key.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Approved dependency preparation supplies the private executable instead of searching PATH.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation: Option<crate::dependencies::Plan>,
}

impl Service {
    /// Relative paths containing directories could silently select code from an untrusted project.
    pub fn valid(&self) -> bool {
        !self.program.is_empty()
            && self.program.len() <= 4096
            && !self.program.contains('\0')
            && (std::path::Path::new(&self.program).is_absolute()
                || !self.program.contains(['/', '\\', ':']))
            && self.args.len() <= 128
            && self
                .args
                .iter()
                .all(|arg| arg.len() <= 32768 && !arg.contains('\0'))
            && self.args.iter().map(String::len).sum::<usize>() <= 65536
    }
}

/// Byte streams keep stdout and stderr separate; PTY output is explicitly merged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    Stdout,
    Stderr,
    Pty,
}

/// Transport choice is independent of executable identity and does not grant execution authority.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transport {
    Stdio,
    Pty { columns: u16, rows: u16 },
}

/// Lifecycle completion follows all output. Termination never implies rollback of native side effects.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Update {
    Output { stream: Stream, bytes: Vec<u8> },
    Exited { code: u32 },
    Terminated,
}

/// Start is separated from handle operations so service callers cannot override executable fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    StartService {
        service: String,
    },
    Execute {
        program: String,
        args: Vec<String>,
        transport: Transport,
    },
    Write {
        handle: crate::api::ResourceHandle,
        bytes: Vec<u8>,
    },
    Resize {
        handle: crate::api::ResourceHandle,
        columns: u16,
        rows: u16,
    },
    Terminate {
        handle: crate::api::ResourceHandle,
    },
}
