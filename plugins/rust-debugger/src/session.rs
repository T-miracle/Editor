//! One source-owned target, its adapter handshake, and pause-scoped request correlations.
use crate::transport::{Transport, failure};
use plugin_protocol::{
    api::{self, ErrorCode, Failure, ResourceHandle},
    process, service,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};

/// What one DAP response completes; internal handshake exchanges have no public reply handle.
enum Pending {
    Initialize,
    Launch,
    InitialBreakpoints,
    Configured,
    Location(u64),
    Control {
        reply: Option<ResourceHandle>,
        moves: bool,
        pause: u64,
    },
    Stop(Option<ResourceHandle>),
    Breakpoints {
        reply: ResourceHandle,
        groups: VecDeque<(String, Vec<u32>)>,
        values: Vec<Value>,
        source: String,
    },
    Frames(ResourceHandle, u64),
    Scopes(ResourceHandle, u64),
    Variables {
        reply: ResourceHandle,
        pause: u64,
        references: VecDeque<u64>,
        values: Vec<Value>,
    },
}

/// All mutable adapter state stays in this guest; the handle is a host-owned native job root.
pub struct Session {
    pub handle: ResourceHandle,
    pub owner: String,
    id: String,
    transport: Transport,
    pending: BTreeMap<u32, Pending>,
    launch: Value,
    start_reply: Option<ResourceHandle>,
    breakpoints: VecDeque<(String, Vec<u32>)>,
    configured: bool,
    launched: bool,
    state: &'static str,
    thread: u64,
    /// Windows may create a breakpoint helper thread for Pause; inspect the requested stopped thread.
    pausing: Option<u64>,
    /// A pending motion blocks new inspections, while preserving the last observed pause on failure.
    moving: Option<u64>,
    pause: u64,
    reason: String,
    source: Option<String>,
    line: Option<u32>,
    output: String,
    diagnostics: String,
    stop_replies: Vec<ResourceHandle>,
}

impl Session {
    /// Start only the installation-approved service; launch the target through DAP exactly once.
    pub fn start(id: String, call: service::Invocation) -> Result<Self, Failure> {
        let reply = call
            .reply
            .ok_or_else(|| failure("plugin.services 1.1 is required"))?;
        let api::Value::Resource(handle) = api::guest::request(api::Operation::Process {
            operation: process::Operation::StartService {
                service: "adapter".into(),
            },
        })?
        else {
            return Err(failure("Expected adapter process resource"));
        };
        let mut session = Self {
            handle,
            owner: call.caller.instance,
            id,
            transport: Transport::default(),
            pending: BTreeMap::new(),
            launch: call.arguments,
            start_reply: Some(reply),
            breakpoints: VecDeque::new(),
            configured: false,
            launched: false,
            state: "starting",
            thread: 0,
            pausing: None,
            moving: None,
            pause: 0,
            reason: String::new(),
            source: None,
            line: None,
            output: String::new(),
            diagnostics: String::new(),
            stop_replies: Vec::new(),
        };
        session.breakpoints = group_breakpoints(&session.launch)?;
        // Presentation is a public editor request owned by the same source as the adapter job.
        // Installation has no such request; only an explicit debug launch reveals its output.
        if let Err(error) = api::guest::EditorTask::start(
            api::EditorOperation::SetPanelVisibility {
                panel: "debug-output".into(),
                visible: true,
            },
            30_000,
        ) {
            session.release();
            return Err(error);
        }
        if let Err(error) = session.send("initialize", json!({"adapterID":"codelldb","clientID":"me-editor","linesStartAt1":true,"columnsStartAt1":true,"pathFormat":"path","supportsRunInTerminalRequest":false}), Pending::Initialize) {
            session.release(); return Err(error);
        }
        Ok(session)
    }
    /// Native completion is authoritative even if the adapter never sent a DAP terminated event.
    pub fn ended(&self) -> bool {
        self.state == "exited"
    }
    /// Each output choice has a stable session identity and its original user-visible configuration name.
    pub fn caption(&self) -> String {
        format!(
            "{} · {} [{}]",
            self.launch["name"].as_str().unwrap_or("调试 / Debug"),
            self.id,
            self.state
        )
    }
    /// Show bounded target output separately from typed call stack/variable results.
    pub fn transcript(&self) -> String {
        format!(
            "Session {} [{}]\n{}{}",
            self.id, self.state, self.output, self.diagnostics
        )
    }
    /// Exact source ownership was checked by the dispatcher before every control or read.
    pub fn call(&mut self, call: service::Invocation) -> Result<Option<Value>, Failure> {
        if call.method == "status" {
            return Ok(Some(self.status()));
        }
        if self.ended() {
            return if call.method == "stop" {
                Ok(Some(self.receipt()))
            } else {
                Err(Failure::new(ErrorCode::InvalidState, "Debug target ended"))
            };
        }
        if self.pending.len() >= 32 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Debug request quota exceeded",
            ));
        }
        let reply = call
            .reply
            .ok_or_else(|| failure("Missing deferred reply"))?;
        match call.method.as_str() {
            "stop" => {
                self.send(
                    "disconnect",
                    json!({"terminateDebuggee":true}),
                    Pending::Stop(Some(reply)),
                )?;
            }
            "resume" | "step" => {
                self.require_pause(&call.arguments)?;
                let command = if call.method == "resume" {
                    "continue"
                } else {
                    match call.arguments["kind"].as_str() {
                        Some("in" | "into" | "step_in") => "stepIn",
                        Some("over" | "step_over") => "next",
                        Some("out" | "step_out") => "stepOut",
                        _ => {
                            return Err(Failure::new(
                                ErrorCode::InvalidRequest,
                                "Unknown step kind",
                            ));
                        }
                    }
                };
                // A rejected or unsent command did not move the target. Only an acknowledged
                // motion or an actual continued/stopped event changes its published pause epoch.
                self.send(
                    command,
                    json!({"threadId":self.thread}),
                    Pending::Control {
                        reply: Some(reply),
                        moves: true,
                        pause: self.pause,
                    },
                )?;
                self.moving = Some(self.pause);
            }
            "pause" => {
                if self.state != "running" {
                    return Err(Failure::new(
                        ErrorCode::InvalidState,
                        "Target is not running",
                    ));
                }
                self.send(
                    "pause",
                    json!({"threadId":self.thread}),
                    Pending::Control {
                        reply: Some(reply),
                        moves: false,
                        pause: self.pause,
                    },
                )?;
                self.pausing = Some(self.thread);
            }
            "frames" => {
                self.require_pause(&call.arguments)?;
                self.send(
                    "stackTrace",
                    json!({"threadId":self.thread,"startFrame":0,"levels":256}),
                    Pending::Frames(reply, self.pause),
                )?;
            }
            "variables" => {
                self.require_pause(&call.arguments)?;
                self.send(
                    "scopes",
                    json!({"frameId":call.arguments["frame"]}),
                    Pending::Scopes(reply, self.pause),
                )?;
            }
            "set_breakpoints" => {
                let mut groups = group_breakpoints(&call.arguments)?;
                // Empty groups explicitly remove every previously bound source; retained set lives in launch.
                for (source, _) in group_breakpoints(&self.launch)? {
                    if !groups.iter().any(|(path, _)| path == &source) {
                        groups.push_back((source, vec![]));
                    }
                }
                self.launch["breakpoints"] = call.arguments["breakpoints"].clone();
                if let Some((source, lines)) = groups.pop_front() {
                    self.send(
                        "setBreakpoints",
                        breakpoint_args(&source, &lines),
                        Pending::Breakpoints {
                            reply,
                            groups,
                            values: vec![],
                            source,
                        },
                    )?;
                } else {
                    return Ok(Some(json!({"breakpoints":[]})));
                }
            }
            _ => {
                return Err(Failure::new(
                    ErrorCode::UnsupportedOperation,
                    "Unknown debug method",
                ));
            }
        }
        Ok(None)
    }
    /// A cancelled public wait releases only its correlation; cancellation never replays a launch.
    pub fn cancel_reply(&mut self, request: &ResourceHandle) {
        if self.start_reply.as_ref() == Some(request) {
            self.start_reply = None;
        }
        // Cancelling a wait cannot discard the correlation that will finish an already-sent motion
        // or disconnect. Keep that protocol state and remove only the public reply authority.
        for pending in self.pending.values_mut() {
            match pending {
                Pending::Control { reply, .. } | Pending::Stop(reply)
                    if reply.as_ref() == Some(request) =>
                {
                    *reply = None
                }
                _ => {}
            }
        }
        self.pending
            .retain(|_, pending| pending.reply() != Some(request));
        self.stop_replies.retain(|reply| reply != request);
    }
    /// Release guest authority; the host retains the native observer through asynchronous tree/EOF cleanup.
    pub fn release(&mut self) {
        let _ = api::guest::request(api::Operation::CloseResource {
            handle: self.handle.clone(),
        });
        self.state = "exited";
    }
    /// stdout carries framed messages, stderr carries diagnostics, and terminal events settle waits.
    pub fn update(&mut self, update: process::Update) {
        let result = match update {
            process::Update::Output {
                stream: process::Stream::Stdout,
                bytes,
            } => self.transport.receive(&bytes).and_then(|messages| {
                for message in messages {
                    self.message(message)?;
                }
                Ok(())
            }),
            process::Update::Output { bytes, .. } => {
                append_bounded(&mut self.diagnostics, &String::from_utf8_lossy(&bytes));
                Ok(())
            }
            process::Update::Exited { .. } | process::Update::Terminated => {
                self.finish_exit();
                Ok(())
            }
        };
        if let Err(error) = result {
            append_bounded(&mut self.diagnostics, &error.message);
            self.fail_pending(error);
            self.release();
        }
    }
    /// Start completes only after launch and configuration acknowledgements, in either response order.
    fn maybe_started(&mut self) {
        if self.configured && self.launched {
            if self.state == "starting" {
                self.state = "running";
            }
            if let Some(reply) = self.start_reply.take() {
                answer(reply, Ok(self.receipt()));
            }
        }
    }
    /// Correlate responses by sequence and inspections by pause generation, never by arrival order.
    fn message(&mut self, message: Value) -> Result<(), Failure> {
        if message["type"] == "event" {
            return self.event(&message);
        }
        if message["type"] == "request" {
            return Err(failure(
                "Adapter requested an unsupported reverse operation",
            ));
        }
        let Some(sequence) = message["request_seq"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
        else {
            return Err(failure("DAP response has no correlation"));
        };
        let Some(pending) = self.pending.remove(&sequence) else {
            return Ok(());
        };
        if message["success"] != true {
            let error = failure(
                message["message"]
                    .as_str()
                    .unwrap_or("Adapter rejected the operation"),
            );
            if let Pending::Control { moves, pause, .. } = &pending {
                if *moves && self.moving == Some(*pause) {
                    self.moving = None;
                }
                if !moves {
                    self.pausing = None;
                }
            }
            if let Some(reply) = pending.reply().cloned() {
                answer(reply, Err(error));
                return Ok(());
            }
            if matches!(pending, Pending::Control { .. } | Pending::Stop(_)) {
                return Ok(());
            }
            return Err(error);
        }
        let body = &message["body"];
        match pending {
            Pending::Initialize => {
                let mut args = json!({"program":self.launch["program"],"args":self.launch["args"],"console":"internalConsole","stopOnEntry":self.launch["stop_on_entry"].as_bool().unwrap_or(false)});
                if let Some(cwd) = self.launch.get("cwd") {
                    args["cwd"] = cwd.clone();
                }
                if let Some(env) = self.launch["env"].as_array() {
                    args["env"] = Value::Object(
                        env.iter()
                            .filter_map(|entry| {
                                Some((entry["name"].as_str()?.into(), entry["value"].clone()))
                            })
                            .collect(),
                    );
                }
                self.send("launch", args, Pending::Launch)?;
            }
            Pending::Launch => {
                self.launched = true;
                self.maybe_started();
            }
            Pending::InitialBreakpoints => self.configure_next()?,
            Pending::Configured => {
                self.configured = true;
                self.maybe_started();
            }
            Pending::Control {
                reply,
                moves,
                pause,
            } => {
                if moves && self.moving == Some(pause) {
                    self.moving = None;
                    // A stopped event may have overtaken this response; preserve that newer pause.
                    if self.pause == pause && self.state == "paused" {
                        self.invalidate_pause()?;
                        self.state = "running";
                    }
                }
                if let Some(reply) = reply {
                    answer(reply, Ok(self.receipt()));
                }
            }
            Pending::Stop(reply) => {
                // DAP acknowledges termination intent. Only actual native job completion ends stop.
                if let Some(reply) = reply {
                    self.stop_replies.push(reply);
                }
                api::guest::request(api::Operation::Process {
                    operation: process::Operation::RequestExit {
                        handle: self.handle.clone(),
                        mode: process::ExitMode::Force,
                    },
                })?;
            }
            Pending::Location(pause) => {
                if pause == self.pause && self.state == "paused" {
                    // A manual Windows pause may stop inside ntdll's synthetic breakpoint. Locate
                    // the first actual source frame instead of inventing a path for disassembly.
                    if let Some(frame) = body["stackFrames"].as_array().and_then(|frames| {
                        frames
                            .iter()
                            .find(|frame| frame["source"]["path"].as_str().is_some())
                    }) {
                        self.source = frame["source"]["path"].as_str().map(str::to_owned);
                        self.line = frame["line"]
                            .as_u64()
                            .and_then(|line| u32::try_from(line).ok());
                    }
                }
            }
            Pending::Frames(reply, pause) => {
                if self.valid_pause(pause, &reply) {
                    let frames = body["stackFrames"].as_array().ok_or_else(|| failure("Missing DAP stack frames"))?.iter().take(256)
                        .filter_map(|frame| Some(json!({"id":frame["id"],"name":bounded(frame["name"].as_str()?,512),"source":frame["source"]["path"].as_str()?,"line":frame["line"].as_u64()?.max(1)}))).collect::<Vec<_>>();
                    answer(reply, Ok(json!({"frames":frames})));
                }
            }
            Pending::Scopes(reply, pause) => {
                if self.valid_pause(pause, &reply) {
                    let mut references: VecDeque<_> = body["scopes"]
                        .as_array()
                        .ok_or_else(|| failure("Missing DAP scopes"))?
                        .iter()
                        .take(16)
                        .filter(|scope| scope["expensive"] != true)
                        .filter_map(|scope| {
                            scope["variablesReference"].as_u64().filter(|id| *id != 0)
                        })
                        .collect();
                    if let Some(reference) = references.pop_front() {
                        self.send(
                            "variables",
                            json!({"variablesReference":reference,"count":512}),
                            Pending::Variables {
                                reply,
                                pause,
                                references,
                                values: vec![],
                            },
                        )?;
                    } else {
                        answer(reply, Ok(json!({"variables":[]})));
                    }
                }
            }
            Pending::Variables {
                reply,
                pause,
                mut references,
                mut values,
            } => {
                if self.valid_pause(pause, &reply) {
                    for variable in body["variables"]
                        .as_array()
                        .ok_or_else(|| failure("Missing DAP variables"))?
                    {
                        if values.len() == 512 {
                            break;
                        }
                        values.push(json!({"name":bounded(variable["name"].as_str().unwrap_or_default(),512),"value":bounded(variable["value"].as_str().unwrap_or_default(),4096)}));
                    }
                    if let Some(reference) = references.pop_front() {
                        self.send(
                            "variables",
                            json!({"variablesReference":reference,"count":512-values.len()}),
                            Pending::Variables {
                                reply,
                                pause,
                                references,
                                values,
                            },
                        )?;
                    } else {
                        answer(reply, Ok(json!({"variables":values})));
                    }
                }
            }
            Pending::Breakpoints {
                reply,
                mut groups,
                mut values,
                source,
            } => {
                for breakpoint in body["breakpoints"]
                    .as_array()
                    .ok_or_else(|| failure("Missing breakpoint acknowledgements"))?
                {
                    values.push(json!({"source":source,"line":breakpoint["line"].as_u64().unwrap_or(1).max(1),"verified":breakpoint["verified"] == true}));
                }
                if let Some((source, lines)) = groups.pop_front() {
                    self.send(
                        "setBreakpoints",
                        breakpoint_args(&source, &lines),
                        Pending::Breakpoints {
                            reply,
                            groups,
                            values,
                            source,
                        },
                    )?;
                } else {
                    answer(reply, Ok(json!({"breakpoints":values})));
                }
            }
        }
        Ok(())
    }
    /// Adapter events update only this target; pause state is never inferred from elapsed time.
    fn event(&mut self, message: &Value) -> Result<(), Failure> {
        let body = &message["body"];
        match message["event"].as_str().unwrap_or_default() {
            "initialized" => self.configure_next()?,
            "thread" => {
                if body["reason"] == "started" && self.thread == 0 {
                    self.thread = body["threadId"].as_u64().unwrap_or(self.thread);
                }
            }
            "stopped" => {
                if let Some(description) = body["description"].as_str() {
                    append_bounded(&mut self.diagnostics, description);
                }
                self.invalidate_pause()?;
                self.state = "paused";
                self.moving = None;
                let requested = self
                    .pausing
                    .take()
                    .filter(|_| body["allThreadsStopped"] == true);
                self.thread = requested
                    .or_else(|| body["threadId"].as_u64())
                    .unwrap_or(self.thread);
                self.reason = bounded(body["reason"].as_str().unwrap_or("paused"), 64);
                self.send(
                    "stackTrace",
                    json!({"threadId":self.thread,"startFrame":0,"levels":256}),
                    Pending::Location(self.pause),
                )?;
            }
            "continued" => {
                self.invalidate_pause()?;
                self.state = "running";
                self.moving = None;
            }
            "output" => append_bounded(
                &mut self.output,
                body["output"].as_str().unwrap_or_default(),
            ),
            "terminated" => {
                // The target is gone. Release the enclosing adapter job and complete outstanding stops.
                self.release();
                self.finish_exit();
            }
            _ => {}
        }
        Ok(())
    }
    /// Group breakpoints by source because DAP replaces the entire set of each source.
    fn configure_next(&mut self) -> Result<(), Failure> {
        if let Some((source, lines)) = self.breakpoints.pop_front() {
            self.send(
                "setBreakpoints",
                breakpoint_args(&source, &lines),
                Pending::InitialBreakpoints,
            )?;
        } else {
            self.send("configurationDone", json!({}), Pending::Configured)?;
        }
        Ok(())
    }
    fn send(&mut self, command: &str, arguments: Value, pending: Pending) -> Result<(), Failure> {
        if self.pending.len() >= 32 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "DAP correlation quota exceeded",
            ));
        }
        let sequence = self.transport.send(&self.handle, command, arguments)?;
        self.pending.insert(sequence, pending);
        Ok(())
    }
    fn receipt(&self) -> Value {
        json!({"session":self.id,"state":self.state,"pause":self.pause})
    }
    fn status(&self) -> Value {
        let mut value = self.receipt();
        if self.state == "paused" {
            value["reason"] = json!(self.reason);
            if let Some(source) = &self.source {
                value["source"] = json!(source);
            }
            if let Some(line) = self.line {
                value["line"] = json!(line);
            }
        }
        value
    }
    /// Epochs bind public frame IDs to the pause the caller actually observed, including delayed calls.
    fn require_pause(&self, arguments: &Value) -> Result<(), Failure> {
        if self.state != "paused" || self.moving.is_some() {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Target has no inspectable pause",
            ));
        }
        if arguments["pause"].as_u64() != Some(self.pause) {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Pause reference expired",
            ));
        }
        Ok(())
    }
    fn invalidate_pause(&mut self) -> Result<(), Failure> {
        self.pause = self
            .pause
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or_else(|| {
                Failure::new(ErrorCode::LimitExceeded, "Debug pause identity exhausted")
            })?;
        self.source = None;
        self.line = None;
        Ok(())
    }
    fn valid_pause(&self, pause: u64, reply: &ResourceHandle) -> bool {
        if self.state == "paused" && self.moving.is_none() && self.pause == pause {
            true
        } else {
            answer(
                reply.clone(),
                Err(Failure::new(
                    ErrorCode::InvalidState,
                    "Pause reference expired",
                )),
            );
            false
        }
    }
    fn finish_exit(&mut self) {
        self.state = "exited";
        self.moving = None;
        let _ = self.invalidate_pause();
        let mut stops = std::mem::take(&mut self.stop_replies);
        // Adapter shutdown may precede its disconnect response, but actual tree completion is enough.
        for pending in self.pending.values() {
            if let Pending::Stop(Some(reply)) = pending {
                stops.push(reply.clone());
            }
        }
        for reply in stops {
            answer(reply, Ok(self.receipt()));
        }
        self.pending
            .retain(|_, pending| !matches!(pending, Pending::Stop(_)));
        self.fail_pending(failure("Debug adapter exited"));
    }
    fn fail_pending(&mut self, error: Failure) {
        if let Some(reply) = self.start_reply.take() {
            answer(reply, Err(error.clone()));
        }
        for (_, pending) in std::mem::take(&mut self.pending) {
            if let Some(reply) = pending.reply().cloned() {
                answer(reply, Err(error.clone()));
            }
        }
    }
}
impl Pending {
    /// This is correlation metadata only; resource ownership remains with the original source.
    fn reply(&self) -> Option<&ResourceHandle> {
        match self {
            Self::Control { reply, .. } | Self::Stop(reply) => reply.as_ref(),
            Self::Frames(reply, _)
            | Self::Scopes(reply, _)
            | Self::Variables { reply, .. }
            | Self::Breakpoints { reply, .. } => Some(reply),
            _ => None,
        }
    }
}
/// Apply the service byte quota before replying. A refused reply retains its handle so an explicit
/// bounded failure can finish the caller instead of silently converting valid work into a timeout.
fn answer(reply: ResourceHandle, value: Result<Value, Failure>) {
    let value = bounded_reply(value);
    if let Err(error) = service::guest::reply(&reply, value) {
        if error.code != ErrorCode::InvalidHandle {
            let failure = Failure::new(error.code, bounded(&error.message, 512));
            if service::guest::reply(&reply, Err(failure)).is_err() {
                let _ = api::guest::request(api::Operation::CloseResource { handle: reply });
            }
        }
    }
}
/// Measure the final encoded value, since item counts alone do not bound strings or escaping.
fn bounded_reply(value: Result<Value, Failure>) -> Result<Value, Failure> {
    if let Ok(value) = &value {
        if serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > 60 * 1024) {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Debug result exceeds the service byte budget",
            ));
        }
    }
    value
}
/// UTF-8 clipping never cuts a scalar and keeps both native UI and per-session memory bounded.
fn bounded(text: &str, maximum: usize) -> String {
    let mut end = text.len().min(maximum);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
fn append_bounded(target: &mut String, text: &str) {
    target.push_str(text);
    if target.len() > 8192 {
        let mut start = target.len() - 8192;
        while !target.is_char_boundary(start) {
            start += 1;
        }
        target.drain(..start);
    }
}
fn group_breakpoints(arguments: &Value) -> Result<VecDeque<(String, Vec<u32>)>, Failure> {
    let mut groups: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for breakpoint in arguments["breakpoints"].as_array().into_iter().flatten() {
        let source = breakpoint["source"]
            .as_str()
            .ok_or_else(|| failure("Missing breakpoint source"))?;
        let line = breakpoint["line"]
            .as_u64()
            .and_then(|line| u32::try_from(line).ok())
            .filter(|line| *line != 0)
            .ok_or_else(|| failure("Invalid breakpoint line"))?;
        groups.entry(source.into()).or_default().push(line);
    }
    Ok(groups.into_iter().collect())
}
fn breakpoint_args(source: &str, lines: &[u32]) -> Value {
    json!({"source":{"path":source},"breakpoints":lines.iter().map(|line| json!({"line":line})).collect::<Vec<_>>()})
}
