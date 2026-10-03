//! Native execution declarations and typed process events; no terminal parsing belongs to the host.
use serde::{Deserialize, Serialize};

/// Installation approves this exact program and argument vector. Calls supply only its map key.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Ordered absolute globs (or `${HOME}/...`) locate approved tool distributions before PATH.
    #[serde(default)]
    pub search_paths: Vec<String>,
    /// Optional argument vector checks a candidate's exit status before selecting it; never a shell.
    #[serde(default)]
    pub check_args: Vec<String>,
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
            && valid_args(&self.args)
            && valid_args(&self.check_args)
            && self.search_paths.len() <= 32
            && self.search_paths.iter().all(|pattern| {
                !pattern.is_empty()
                    && pattern.len() <= 4096
                    && !pattern.chars().any(char::is_control)
                    && !pattern.contains("**")
                    && !pattern.split(['/', '\\']).any(|part| part == "..")
                    && (pattern
                        .strip_prefix("${HOME}/")
                        .is_some_and(|tail| !tail.is_empty() && !tail.contains("${"))
                        || (std::path::Path::new(pattern).is_absolute() && !pattern.contains("${")))
            })
    }
}

/// Both startup and probe arguments share the same bounded, literal argv contract.
fn valid_args(args: &[String]) -> bool {
    args.len() <= 128
        && args
            .iter()
            .all(|arg| arg.len() <= 32768 && !arg.contains('\0'))
        && args.iter().map(String::len).sum::<usize>() <= 65536
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
    Pty {
        columns: u16,
        rows: u16,
        /// process 1.3: Windows asks the byte-stream consumer for its existing cursor position.
        /// The caller must answer the VT query through Write; other platforms reject true.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        inherit_cursor: bool,
    },
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
        /// process 1.2: an explicit absolute native directory under the already granted execution authority.
        /// Omission preserves the instance's workspace/private-data default; this grants no WASI access.
        #[serde(default)]
        cwd: Option<String>,
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

/// Omitted/default options retain compatibility with older strict process decoders.
#[cfg(test)]
mod tests {
    use super::Transport;

    #[test]
    fn default_pty_omits_cursor_extension_and_opt_in_is_explicit() {
        let old = serde_json::json!({"kind":"pty", "columns":80, "rows":24});
        let value: Transport = serde_json::from_value(old.clone()).unwrap();
        assert!(matches!(
            value,
            Transport::Pty {
                inherit_cursor: false,
                ..
            }
        ));
        assert_eq!(serde_json::to_value(value).unwrap(), old);
        let requested = Transport::Pty {
            columns: 80,
            rows: 24,
            inherit_cursor: true,
        };
        assert_eq!(
            serde_json::to_value(requested).unwrap()["inherit_cursor"],
            true
        );
    }
}
