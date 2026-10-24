//! One ACP agent process and its single session.
use crate::{
    fs,
    log::{truncate, EventLog},
    rpc::{
        CallError, Rpc, AUTH_REQUIRED, INTERNAL_ERROR, INVALID_PARAMS, METHOD_NOT_FOUND,
        RESOURCE_NOT_FOUND,
    },
    AcpClientHost, AcpError, AcpEvent, AcpLaunch, AgentInfo, AuthMethod, PermissionOption,
    PermissionOptionKind, PermissionRequest, PlanEntry, PromptHandle, ToolCallStatus, ToolKind,
    TurnStatus,
};
use anima_process::{Containment, Custody};
use process_wrap::tokio::{ChildWrapper, CommandWrap};
use serde_json::{json, Value};
use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
    sync::{mpsc, watch},
    task::JoinHandle,
    time::timeout,
};

/// The only ACP major version this client speaks.
const PROTOCOL_VERSION: u64 = 1;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);
/// File writes arrive as one JSON line, so this bounds the largest write.
const MAX_LINE: usize = 32 * 1024 * 1024;
const MAX_STDERR_LINE: usize = 4 * 1024;
/// Longer stderr lines are dropped whole rather than buffered.
const MAX_STDERR_READ: usize = 64 * 1024;
const STDERR_TAIL: usize = 2 * 1024;
const MAX_READ: u64 = 8 * 1024 * 1024;
const EXIT_POLL: Duration = Duration::from_millis(50);
/// After the process exits, how long its last stdout lines may take to drain
/// (a grandchild holding the pipe open would otherwise delay the exit).
const DRAIN_GRACE: Duration = Duration::from_millis(500);
/// How long a failed handshake waits for the exit code it reports.
const EXIT_REPORT_GRACE: Duration = Duration::from_secs(2);

type SharedChild = Arc<tokio::sync::Mutex<Box<dyn ChildWrapper>>>;

struct Inner {
    rpc: Rpc,
    host: Arc<dyn AcpClientHost>,
    /// Canonical session cwd; every fs request must resolve inside it.
    root: PathBuf,
    session_id: Mutex<Option<String>>,
    log: Mutex<EventLog>,
    status: watch::Sender<TurnStatus>,
    /// Bumped by cancel and stop; pending permission requests answer
    /// `cancelled` when it changes.
    interrupt: watch::Sender<u64>,
    /// Some(code) once the agent process is gone.
    exit: watch::Sender<Option<Option<i32>>>,
    stdout_done: watch::Sender<bool>,
    stderr_tail: Mutex<String>,
    produced_output: AtomicBool,
}

pub struct AcpSession {
    inner: Arc<Inner>,
    info: AgentInfo,
    session_id: String,
    /// The agent's session mode after setup, when it reports modes.
    mode: Option<String>,
    custody: Custody,
    child: SharedChild,
    tasks: Vec<JoinHandle<()>>,
    turns: AtomicU64,
}

impl AcpSession {
    /// Runs the agent under custody, then `initialize` and `session/new` (or
    /// `session/load` for a resume id). Any failure retires the process.
    pub async fn start(launch: AcpLaunch, host: Arc<dyn AcpClientHost>) -> Result<Self, AcpError> {
        Self::start_with_requirement(launch, host, false).await
    }

    /// Host-facing execution with complete descendant custody required.
    pub async fn start_contained(
        launch: AcpLaunch,
        host: Arc<dyn AcpClientHost>,
    ) -> Result<Self, AcpError> {
        Self::start_with_requirement(launch, host, true).await
    }

    async fn start_with_requirement(
        launch: AcpLaunch,
        host: Arc<dyn AcpClientHost>,
        require_strong: bool,
    ) -> Result<Self, AcpError> {
        if !launch.cwd.is_absolute() {
            return Err(AcpError::Spawn(format!(
                "cwd {} is not absolute",
                launch.cwd.display()
            )));
        }
        let root = std::fs::canonicalize(&launch.cwd)
            .ok()
            .filter(|root| root.is_dir())
            .ok_or_else(|| {
                AcpError::Spawn(format!("cwd {} is not a directory", launch.cwd.display()))
            })?;
        let (mut custody, mut native) = Custody::prepare(&launch.program, launch.args.clone());
        if require_strong {
            custody
                .require_strong()
                .map_err(|error| AcpError::Spawn(error.to_string()))?;
        }
        native
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if launch.env_clear {
            native.env_clear();
        }
        native.envs(launch.env.iter().map(|(k, v)| (k, v)));
        let mut command = CommandWrap::from(native);
        custody.wrap(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| AcpError::Spawn(format!("{}: {error}", launch.program.display())))?;
        custody.attach(child.as_ref());
        let stdin = child.stdin().take();
        let stdout = child.stdout().take();
        let stderr = child.stderr().take();

        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            rpc: Rpc::new(out_tx),
            host,
            root,
            session_id: Mutex::new(None),
            log: Mutex::new(EventLog::new()),
            status: watch::Sender::new(TurnStatus::Idle),
            interrupt: watch::Sender::new(0),
            exit: watch::Sender::new(None),
            stdout_done: watch::Sender::new(false),
            stderr_tail: Mutex::new(String::new()),
            produced_output: AtomicBool::new(false),
        });
        let child: SharedChild = Arc::new(tokio::sync::Mutex::new(child));
        let mut tasks = Vec::new();
        if let Some(stdin) = stdin {
            tasks.push(tokio::spawn(write_loop(stdin, out_rx)));
        }
        if let Some(stdout) = stdout {
            tasks.push(tokio::spawn(read_loop(inner.clone(), stdout)));
        }
        if let Some(stderr) = stderr {
            tasks.push(tokio::spawn(stderr_loop(inner.clone(), stderr)));
        }
        tasks.push(tokio::spawn(exit_loop(
            inner.clone(),
            Arc::downgrade(&child),
        )));

        let mut session = Self {
            inner,
            info: AgentInfo::default(),
            session_id: String::new(),
            mode: None,
            custody,
            child,
            tasks,
            turns: AtomicU64::new(0),
        };
        match session.handshake(&launch).await {
            Ok(()) => Ok(session),
            Err(error) => {
                let error = session.spawn_failure(error);
                session.stop().await;
                Err(error)
            }
        }
    }

    async fn handshake(&mut self, launch: &AcpLaunch) -> Result<(), AcpError> {
        let init = self
            .call(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "clientCapabilities": {
                        "fs": {"readTextFile": true, "writeTextFile": true},
                        "terminal": false,
                    },
                    "clientInfo": {
                        "name": "anima",
                        "title": "Anima",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                }),
                "initialize",
            )
            .await?;
        if init["protocolVersion"].as_u64() != Some(PROTOCOL_VERSION) {
            return Err(AcpError::Protocol(format!(
                "agent speaks protocol version {}, this client speaks {PROTOCOL_VERSION}",
                init["protocolVersion"]
            )));
        }
        self.info = agent_info(&init);
        let cwd = launch.cwd.to_string_lossy();
        let auth = |error| match error {
            AcpError::Rpc { code, .. } if code == AUTH_REQUIRED => AcpError::AuthRequired {
                methods: self.info.auth_methods.clone(),
            },
            other => other,
        };
        if let Some(resume) = &launch.resume_session_id {
            if !self.info.load_session {
                return Err(AcpError::ResumeUnsupported);
            }
            // Replayed history arrives as session/update before the reply.
            *self
                .inner
                .session_id
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(resume.clone());
            let loaded = self
                .call(
                    "session/load",
                    json!({"sessionId": resume, "cwd": cwd, "mcpServers": []}),
                    "session/load",
                )
                .await
                .map_err(auth)?;
            self.session_id = resume.clone();
            self.choose_ask_mode(&loaded, &launch.ask_modes).await?;
        } else {
            let created = self
                .call(
                    "session/new",
                    json!({"cwd": cwd, "mcpServers": []}),
                    "session/new",
                )
                .await
                .map_err(auth)?;
            let Some(id) = created["sessionId"].as_str() else {
                return Err(AcpError::Protocol(
                    "session/new returned no sessionId".into(),
                ));
            };
            *self
                .inner
                .session_id
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(id.into());
            self.session_id = id.into();
            self.choose_ask_mode(&created, &launch.ask_modes).await?;
        }
        Ok(())
    }

    /// Switches to the first preferred mode the agent offers, unless it is
    /// already in one; an agent without modes is left as it is.
    async fn choose_ask_mode(
        &mut self,
        opened: &Value,
        preferred: &[String],
    ) -> Result<(), AcpError> {
        let modes = &opened["modes"];
        let current = modes["currentModeId"].as_str().map(str::to_string);
        let offered: Vec<&str> = modes["availableModes"]
            .as_array()
            .map(|modes| {
                modes
                    .iter()
                    .filter_map(|mode| mode["id"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        self.mode = current.clone();
        if current
            .as_deref()
            .is_some_and(|id| preferred.iter().any(|want| want == id))
        {
            return Ok(());
        }
        let Some(target) = preferred
            .iter()
            .find(|want| offered.contains(&want.as_str()))
        else {
            return Ok(());
        };
        self.call(
            "session/set_mode",
            json!({"sessionId": self.session_id, "modeId": target}),
            "session/set_mode",
        )
        .await?;
        self.mode = Some(target.clone());
        Ok(())
    }

    /// The agent's session mode after setup, when it reports modes.
    pub fn mode(&self) -> Option<&str> {
        self.mode.as_deref()
    }

    async fn call(
        &self,
        method: &str,
        params: Value,
        stage: &'static str,
    ) -> Result<Value, AcpError> {
        match timeout(HANDSHAKE_TIMEOUT, self.inner.rpc.request(method, params)).await {
            Err(_) => Err(AcpError::Timeout { stage }),
            Ok(Ok(value)) => Ok(value),
            Ok(Err(CallError::Rpc { code, message })) => Err(AcpError::Rpc { code, message }),
            Ok(Err(CallError::Closed)) => Err(self.inner.exited_error().await),
        }
    }

    /// A program that never ran is a spawn failure, even when only a wrapper
    /// shell (cgroup join, `exec` of a missing binary) reported it.
    fn spawn_failure(&self, error: AcpError) -> AcpError {
        let AcpError::Exited {
            code: Some(code), ..
        } = &error
        else {
            return error;
        };
        let produced = self.inner.produced_output.load(Ordering::Relaxed);
        if self.custody.join_failed(*code, produced) {
            return AcpError::Spawn("could not join the process cgroup".into());
        }
        if self.custody.containment() == Containment::Cgroup && *code == 127 && !produced {
            return AcpError::Spawn(format!("program not found ({error})"));
        }
        error
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn agent_info(&self) -> AgentInfo {
        self.info.clone()
    }

    pub fn containment(&self) -> Containment {
        self.custody.containment()
    }

    /// Starts a turn and returns at once; follow it with [`Self::wait`] and
    /// [`Self::events_since`]. Only one turn runs at a time.
    pub async fn prompt(&self, text: String) -> Result<PromptHandle, AcpError> {
        if self.inner.exit.borrow().is_some() {
            return Err(self.inner.exited_error().await);
        }
        let started = self.inner.status.send_if_modified(|status| {
            if *status == TurnStatus::Running {
                false
            } else {
                *status = TurnStatus::Running;
                true
            }
        });
        if !started {
            return Err(AcpError::Busy);
        }
        let cursor = self.inner.lock_log().next();
        let turn = self.turns.fetch_add(1, Ordering::Relaxed) + 1;
        let weak = Arc::downgrade(&self.inner);
        self.inner.rpc.call(
            "session/prompt",
            json!({
                "sessionId": self.session_id,
                "prompt": [{"type": "text", "text": text}],
            }),
            Box::new(move |result| {
                if let Some(inner) = weak.upgrade() {
                    inner.finish_turn(result);
                }
            }),
        );
        Ok(PromptHandle { turn, cursor })
    }

    /// Waits up to `max` for the running turn to end; returns the status then.
    pub async fn wait(&self, max: Duration) -> TurnStatus {
        let mut status = self.inner.status.subscribe();
        let _ = timeout(max, async {
            let _ = status.wait_for(|s| *s != TurnStatus::Running).await;
        })
        .await;
        let current = status.borrow().clone();
        current
    }

    pub fn status(&self) -> TurnStatus {
        self.inner.status.borrow().clone()
    }

    /// Events from `cursor` on and the next cursor. Older events beyond the
    /// 1 MiB budget are gone; [`Self::dropped_events`] counts them.
    pub fn events_since(&self, cursor: u64) -> (Vec<AcpEvent>, u64) {
        self.inner.lock_log().since(cursor)
    }

    pub fn dropped_events(&self) -> u64 {
        self.inner.lock_log().dropped()
    }

    /// Sends `session/cancel` and answers pending permission requests with
    /// `cancelled`. The turn ends when the agent replies `cancelled`.
    pub async fn cancel(&self) -> Result<(), AcpError> {
        if self.inner.exit.borrow().is_some()
            || !self
                .inner
                .rpc
                .notify("session/cancel", json!({"sessionId": self.session_id}))
        {
            return Err(self.inner.exited_error().await);
        }
        self.inner.interrupt.send_modify(|n| *n += 1);
        Ok(())
    }

    /// Kills the agent's whole tree. True only once every member is proven
    /// gone (see [`Custody::retire`]). Safe to call more than once.
    pub async fn stop(&self) -> bool {
        self.inner.interrupt.send_modify(|n| *n += 1);
        let mut child = self.child.lock().await;
        let retired = self.custody.retire(&mut child).await;
        let code = child.try_wait().ok().flatten().and_then(|s| s.code());
        drop(child);
        self.inner.on_exit(code);
        for task in &self.tasks {
            task.abort();
        }
        retired
    }
}

impl Drop for AcpSession {
    /// Dropping the child kills the agent (kill-on-drop); [`Self::stop`] is
    /// the way to also prove the tree gone.
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        self.inner.rpc.close();
        self.inner.exit.send_if_modified(|exit| {
            let first = exit.is_none();
            if first {
                *exit = Some(None);
            }
            first
        });
    }
}

impl Inner {
    fn lock_log(&self) -> std::sync::MutexGuard<'_, EventLog> {
        self.log.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn push(&self, event: AcpEvent) {
        self.lock_log().push(event);
    }

    fn diagnostic(&self, text: String) {
        self.push(AcpEvent::Stderr {
            text: format!("[acp] {text}"),
        });
    }

    async fn exited_error(&self) -> AcpError {
        let mut exit = self.exit.subscribe();
        let code = timeout(EXIT_REPORT_GRACE, async {
            exit.wait_for(Option::is_some).await.ok().and_then(|v| *v)
        })
        .await
        .ok()
        .flatten()
        .flatten();
        let stderr = self
            .stderr_tail
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .trim()
            .to_owned();
        AcpError::Exited { code, stderr }
    }

    fn on_exit(&self, code: Option<i32>) {
        let first = self.exit.send_if_modified(|exit| {
            let first = exit.is_none();
            if first {
                *exit = Some(code);
            }
            first
        });
        if !first {
            return;
        }
        self.rpc.close();
        self.push(AcpEvent::Exited { code });
        self.end_turn(TurnStatus::Exited { code });
        self.interrupt.send_modify(|n| *n += 1);
    }

    /// Moves a running turn to `status`; a turn that already ended keeps its
    /// first outcome.
    fn end_turn(&self, status: TurnStatus) {
        self.status.send_if_modified(|current| {
            let running = *current == TurnStatus::Running;
            if running {
                *current = status;
            }
            running
        });
    }

    fn finish_turn(&self, result: Result<Value, CallError>) {
        match result {
            Ok(value) => {
                let stop_reason = value["stopReason"].as_str().unwrap_or("unknown").to_owned();
                self.push(AcpEvent::TurnEnded {
                    stop_reason: stop_reason.clone(),
                });
                self.end_turn(TurnStatus::Finished { stop_reason });
            }
            Err(CallError::Rpc { code, message }) => {
                self.push(AcpEvent::TurnFailed {
                    code,
                    message: message.clone(),
                });
                self.end_turn(TurnStatus::Failed { code, message });
            }
            // Exit handling ends the turn; here only when exit already won.
            Err(CallError::Closed) => {
                if let Some(code) = *self.exit.borrow() {
                    self.end_turn(TurnStatus::Exited { code });
                }
            }
        }
    }

    fn dispatch(self: &Arc<Self>, line: &[u8]) {
        let message: Value = match serde_json::from_slice(line) {
            Ok(message) => message,
            Err(_) => {
                let mut text = String::from_utf8_lossy(line).into_owned();
                truncate(&mut text, 200);
                self.diagnostic(format!("malformed line from agent: {text}"));
                return;
            }
        };
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").filter(|id| !id.is_null()).cloned();
        match (method, id) {
            (Some(method), Some(id)) => {
                let inner = self.clone();
                let method = method.to_owned();
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                tokio::spawn(async move {
                    let result = inner.serve(&method, params).await;
                    inner.rpc.respond(id, result);
                });
            }
            (Some("session/update"), None) => {
                if let Some(params) = message.get("params") {
                    self.on_update(params);
                }
            }
            (Some(_), None) => {}
            (None, Some(_)) => self.rpc.resolve(&message),
            (None, None) => self.diagnostic("agent sent a message with no method or id".into()),
        }
    }

    async fn serve(&self, method: &str, params: Value) -> Result<Value, (i64, String)> {
        match method {
            "session/request_permission" => Ok(self.permission(&params).await),
            "fs/read_text_file" => self.read_file(&params).await,
            "fs/write_text_file" => self.write_file(&params).await,
            _ => Err((METHOD_NOT_FOUND, format!("method not found: {method}"))),
        }
    }

    async fn permission(&self, params: &Value) -> Value {
        let tool = &params["toolCall"];
        let request = PermissionRequest {
            tool_call_id: text(&tool["toolCallId"]),
            title: text(&tool["title"]),
            kind: ToolKind::parse(tool["kind"].as_str()),
            locations: locations(&tool["locations"]),
            raw_input: Some(tool["rawInput"].clone()).filter(|v| !v.is_null()),
            options: params["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    Some(PermissionOption {
                        id: option["optionId"].as_str()?.to_owned(),
                        name: text(&option["name"]),
                        kind: PermissionOptionKind::parse(option["kind"].as_str()?)?,
                    })
                })
                .collect(),
        };
        self.push(AcpEvent::PermissionAsked {
            tool_call_id: request.tool_call_id.clone(),
            title: request.title.clone(),
            kind: request.kind,
            options: request.options.clone(),
        });
        let tool_call_id = request.tool_call_id.clone();
        let offered: Vec<String> = request.options.iter().map(|o| o.id.clone()).collect();
        let mut interrupt = self.interrupt.subscribe();
        let mut exit = self.exit.subscribe();
        let answer = tokio::select! {
            answer = self.host.request_permission(request) => answer,
            _ = interrupt.changed() => None,
            _ = async { let _ = exit.wait_for(Option::is_some).await; } => None,
        };
        let answer = match answer {
            Some(id) if !offered.contains(&id) => {
                self.diagnostic(format!("host chose unoffered permission option {id}"));
                None
            }
            answer => answer,
        };
        self.push(AcpEvent::PermissionAnswered {
            tool_call_id,
            option_id: answer.clone(),
        });
        match answer {
            Some(id) => json!({"outcome": {"outcome": "selected", "optionId": id}}),
            None => json!({"outcome": {"outcome": "cancelled"}}),
        }
    }

    async fn read_file(&self, params: &Value) -> Result<Value, (i64, String)> {
        let path = self.confined(params)?;
        let metadata = tokio::fs::metadata(&path).await.map_err(io_error)?;
        if metadata.len() > MAX_READ {
            return Err((
                INVALID_PARAMS,
                format!("{} is larger than {MAX_READ} bytes", path.display()),
            ));
        }
        let content = tokio::fs::read_to_string(&path).await.map_err(io_error)?;
        let content = fs::slice_lines(content, params["line"].as_u64(), params["limit"].as_u64());
        Ok(json!({"content": content}))
    }

    async fn write_file(&self, params: &Value) -> Result<Value, (i64, String)> {
        let path = self.confined(params)?;
        let Some(content) = params["content"].as_str() else {
            return Err((INVALID_PARAMS, "content is required".into()));
        };
        let mut exit = self.exit.subscribe();
        let approval = tokio::select! {
            approval = self.host.approve_write(&path) => approval,
            _ = async { let _ = exit.wait_for(Option::is_some).await; } => Err("session ended".into()),
        };
        approval.map_err(|reason| (INVALID_PARAMS, format!("write refused: {reason}")))?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_error)?;
        }
        tokio::fs::write(&path, content).await.map_err(io_error)?;
        self.push(AcpEvent::FileWritten { path });
        Ok(json!({}))
    }

    fn confined(&self, params: &Value) -> Result<PathBuf, (i64, String)> {
        let Some(path) = params["path"].as_str() else {
            return Err((INVALID_PARAMS, "path is required".into()));
        };
        fs::confine(&self.root, Path::new(path)).map_err(|reason| (INVALID_PARAMS, reason))
    }

    fn on_update(&self, params: &Value) {
        if let Some(id) = params["sessionId"].as_str() {
            let ours = self.session_id.lock().unwrap_or_else(|e| e.into_inner());
            if ours.as_deref().is_some_and(|ours| ours != id) {
                return;
            }
        }
        let update = &params["update"];
        let chunk = |make: fn(String) -> AcpEvent| {
            let event = make(content_text(&update["content"]));
            self.lock_log()
                .push_chunk(event, update["messageId"].as_str());
        };
        match update["sessionUpdate"].as_str() {
            Some("agent_message_chunk") => chunk(|text| AcpEvent::AgentMessage { text }),
            Some("agent_thought_chunk") => chunk(|text| AcpEvent::AgentThought { text }),
            Some("user_message_chunk") => chunk(|text| AcpEvent::UserMessage { text }),
            Some("tool_call") => self.push(AcpEvent::ToolCall {
                id: text(&update["toolCallId"]),
                title: text(&update["title"]),
                kind: ToolKind::parse(update["kind"].as_str()),
                status: ToolCallStatus::parse(update["status"].as_str())
                    .unwrap_or(ToolCallStatus::Pending),
                locations: locations(&update["locations"]),
            }),
            Some("tool_call_update") => self.push(AcpEvent::ToolCallUpdate {
                id: text(&update["toolCallId"]),
                status: ToolCallStatus::parse(update["status"].as_str()),
                content_text: tool_content_text(&update["content"]),
            }),
            Some("plan") => self.push(AcpEvent::Plan {
                entries: update["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|entry| PlanEntry {
                        content: text(&entry["content"]),
                        priority: text(&entry["priority"]),
                        status: text(&entry["status"]),
                    })
                    .collect(),
            }),
            // Modes, commands, usage and other updates are not surfaced yet.
            _ => {}
        }
    }

    fn on_stderr(&self, mut line: String) {
        truncate(&mut line, MAX_STDERR_LINE);
        {
            let mut tail = self.stderr_tail.lock().unwrap_or_else(|e| e.into_inner());
            tail.push_str(&line);
            tail.push('\n');
            if tail.len() > STDERR_TAIL {
                let mut cut = tail.len() - STDERR_TAIL;
                while !tail.is_char_boundary(cut) {
                    cut += 1;
                }
                tail.drain(..cut);
            }
        }
        self.push(AcpEvent::Stderr { text: line });
    }
}

fn agent_info(init: &Value) -> AgentInfo {
    let caps = &init["agentCapabilities"];
    let prompt = &caps["promptCapabilities"];
    let agent = &init["agentInfo"];
    let string = |v: &Value| v.as_str().map(str::to_owned);
    AgentInfo {
        name: string(&agent["name"]),
        title: string(&agent["title"]),
        version: string(&agent["version"]),
        protocol_version: init["protocolVersion"]
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or_default(),
        load_session: caps["loadSession"].as_bool().unwrap_or(false),
        prompt_image: prompt["image"].as_bool().unwrap_or(false),
        prompt_audio: prompt["audio"].as_bool().unwrap_or(false),
        prompt_embedded_context: prompt["embeddedContext"].as_bool().unwrap_or(false),
        auth_methods: init["authMethods"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|method| {
                Some(AuthMethod {
                    id: method["id"].as_str()?.to_owned(),
                    name: text(&method["name"]),
                    description: string(&method["description"]),
                })
            })
            .collect(),
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn locations(value: &Value) -> Vec<PathBuf> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|location| location["path"].as_str().map(PathBuf::from))
        .collect()
}

/// Text of an ACP content block; non-text blocks become a short marker.
fn content_text(block: &Value) -> String {
    match block["type"].as_str() {
        Some("text") => text(&block["text"]),
        Some("resource_link") => format!("[resource_link {}]", text(&block["uri"])),
        Some("resource") => match block["resource"]["text"].as_str() {
            Some(text) => text.to_owned(),
            None => format!("[resource {}]", text(&block["resource"]["uri"])),
        },
        Some(other) => format!("[{other}]"),
        None => String::new(),
    }
}

fn tool_content_text(content: &Value) -> Option<String> {
    let parts: Vec<String> = content
        .as_array()?
        .iter()
        .map(|item| match item["type"].as_str() {
            Some("content") => content_text(&item["content"]),
            Some("diff") => format!("[diff {}]", text(&item["path"])),
            Some("terminal") => format!("[terminal {}]", text(&item["terminalId"])),
            _ => String::new(),
        })
        .filter(|part| !part.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn io_error(error: io::Error) -> (i64, String) {
    if error.kind() == io::ErrorKind::NotFound {
        (RESOURCE_NOT_FOUND, error.to_string())
    } else {
        (INTERNAL_ERROR, error.to_string())
    }
}

async fn write_loop(mut stdin: ChildStdin, mut lines: mpsc::UnboundedReceiver<String>) {
    while let Some(mut line) = lines.recv().await {
        line.push('\n');
        if stdin.write_all(line.as_bytes()).await.is_err() || stdin.flush().await.is_err() {
            break;
        }
    }
}

async fn read_loop(inner: Arc<Inner>, stdout: ChildStdout) {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        match read_line(&mut reader, &mut line, MAX_LINE).await {
            Ok(Some(true)) => {
                inner.produced_output.store(true, Ordering::Relaxed);
                if !line.iter().all(u8::is_ascii_whitespace) {
                    inner.dispatch(&line);
                }
            }
            Ok(Some(false)) => {
                inner.produced_output.store(true, Ordering::Relaxed);
                inner.diagnostic(format!("dropped a line over {MAX_LINE} bytes"));
            }
            Ok(None) | Err(_) => break,
        }
    }
    inner.rpc.close();
    inner.stdout_done.send_replace(true);
}

async fn stderr_loop(inner: Arc<Inner>, stderr: tokio::process::ChildStderr) {
    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    while let Ok(Some(complete)) = read_line(&mut reader, &mut line, MAX_STDERR_READ).await {
        if complete {
            inner.on_stderr(String::from_utf8_lossy(&line).into_owned());
        } else {
            inner.diagnostic(format!(
                "dropped a stderr line over {MAX_STDERR_READ} bytes"
            ));
        }
    }
}

/// Polls rather than awaits the exit, so `stop` never waits behind a lock
/// held across the agent's whole life.
async fn exit_loop(inner: Arc<Inner>, child: Weak<tokio::sync::Mutex<Box<dyn ChildWrapper>>>) {
    let code = loop {
        tokio::time::sleep(EXIT_POLL).await;
        let Some(child) = child.upgrade() else {
            return;
        };
        let polled = match child.try_lock() {
            Ok(mut child) => child.try_wait(),
            Err(_) => continue,
        };
        match polled {
            Ok(None) => continue,
            Ok(Some(status)) => break status.code(),
            Err(_) => break None,
        }
    };
    let mut done = inner.stdout_done.subscribe();
    let _ = timeout(DRAIN_GRACE, async {
        let _ = done.wait_for(|done| *done).await;
    })
    .await;
    inner.on_exit(code);
}

/// Reads one `\n`-terminated line into `buf`. None at EOF; Some(false) when
/// the line exceeded `max` (its bytes are discarded).
async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max: usize,
) -> io::Result<Option<bool>> {
    buf.clear();
    let mut too_long = false;
    let mut any = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(any.then_some(!too_long));
        }
        any = true;
        let (chunk, used, done) = match available.iter().position(|b| *b == b'\n') {
            Some(i) => (&available[..i], i + 1, true),
            None => (available, available.len(), false),
        };
        if !too_long {
            if buf.len() + chunk.len() > max {
                too_long = true;
                buf.clear();
            } else {
                buf.extend_from_slice(chunk);
            }
        }
        reader.consume(used);
        if done {
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(Some(!too_long));
        }
    }
}
