//! Live sessions an agent keeps open across turns: interactive app sessions
//! (a program on a terminal) and ACP sessions (a coding agent spoken to over
//! the Agent Client Protocol).
//!
//! - At most [`MAX_LIVE_PER_AGENT`] live sessions per agent, both kinds
//!   together. A session idle for [`IDLE_STOP`] is stopped; a finished one is
//!   kept for [`FINISHED_KEEP`] so the agent can still read how it ended.
//! - Sessions belong to the daemon process, so they outlive a single run and
//!   die with the daemon: dropping an app session kills its tree, and an ACP
//!   session runs under the same custody.
//! - An ACP agent's permission requests wait in a queue until one of the
//!   agent's own tool calls is waiting on that session; that call then puts
//!   each request through the run's approval gate (rules, policy, and the
//!   owner's answer when the policy says ask).
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anima_acp::{
    AcpClientHost, AcpEvent, AcpSession, BoxFuture, PermissionOptionKind, PermissionRequest,
    ToolKind, TurnStatus,
};
use anima_core::{AgentState, DataValue, ToolCall};
use anima_process::session::{AppSession, Screen, WaitOutcome};
use anima_process::Containment;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

pub(crate) const MAX_LIVE_PER_AGENT: usize = 4;
const IDLE_STOP: Duration = Duration::from_secs(30 * 60);
const FINISHED_KEEP: Duration = Duration::from_secs(10 * 60);
const SWEEP_EVERY: Duration = Duration::from_secs(30);
/// Bound on new transcript text one read returns; the newest text wins.
pub(super) const READ_OUTPUT_BYTES: usize = 16 * 1024;
/// Bound on ACP event text one read returns.
const READ_EVENT_BYTES: usize = 48 * 1024;

pub(crate) struct LiveSessions {
    entries: Mutex<HashMap<String, Live>>,
    next_id: Mutex<u64>,
}

pub(super) struct Live {
    pub(super) agent_id: String,
    pub(super) command: String,
    pub(super) cwd: PathBuf,
    pub(super) started_at: chrono::DateTime<chrono::Utc>,
    pub(super) last_used: Instant,
    /// Set when the session was stopped or seen finished.
    pub(super) finished_at: Option<Instant>,
    pub(super) stopped: bool,
    pub(super) kind: LiveKind,
}

pub(super) enum LiveKind {
    App(AppLive),
    Acp(AcpLive),
}

pub(super) struct AppLive {
    pub(super) session: Arc<AppSession>,
    /// Where the agent's next read of the transcript starts.
    pub(super) read_from: u64,
}

pub(super) struct AcpLive {
    pub(super) session: Arc<AcpSession>,
    pub(super) read_from: u64,
    pub(super) agent_name: String,
    pub(super) permissions: Arc<PermissionQueue>,
}

/// The process-wide registry; sessions belong to the daemon, not a run.
pub(crate) fn live_sessions() -> &'static LiveSessions {
    static SESSIONS: std::sync::OnceLock<LiveSessions> = std::sync::OnceLock::new();
    SESSIONS.get_or_init(|| {
        super::process_runtime().spawn(sweep_forever());
        LiveSessions {
            entries: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
        }
    })
}

async fn sweep_forever() {
    loop {
        tokio::time::sleep(SWEEP_EVERY).await;
        live_sessions().sweep().await;
    }
}

impl LiveSessions {
    fn entries(&self) -> MutexGuard<'_, HashMap<String, Live>> {
        self.entries.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Reserves a slot for `agent_id`, or says why not.
    pub(super) fn check_capacity(&self, agent_id: &str) -> Result<(), String> {
        let live = self
            .entries()
            .values()
            .filter(|entry| entry.agent_id == agent_id && entry.finished_at.is_none())
            .count();
        if live >= MAX_LIVE_PER_AGENT {
            return Err(format!(
                "at most {MAX_LIVE_PER_AGENT} apps and coding agents can be open at once; stop one first (app_list, acp_list)"
            ));
        }
        Ok(())
    }

    pub(super) fn insert(&self, prefix: &str, live: Live) -> String {
        let id = {
            let mut next = self.next_id.lock().unwrap_or_else(|p| p.into_inner());
            let id = format!("{prefix}-{next}");
            *next += 1;
            id
        };
        self.entries().insert(id.clone(), live);
        id
    }

    /// Runs `f` on the agent's own session; another agent's id is unknown.
    pub(super) fn with<T>(
        &self,
        agent_id: &str,
        id: &str,
        f: impl FnOnce(&mut Live) -> T,
    ) -> Result<T, String> {
        let mut entries = self.entries();
        match entries.get_mut(id) {
            Some(entry) if entry.agent_id == agent_id => {
                entry.last_used = Instant::now();
                Ok(f(entry))
            }
            _ => Err(format!("no open app or coding agent with id {id}")),
        }
    }

    pub(super) fn list(&self, agent_id: &str, prefix: &str) -> Vec<Value> {
        let mut rows: Vec<(String, Value)> = self
            .entries()
            .iter_mut()
            .filter(|(id, entry)| entry.agent_id == agent_id && id.starts_with(prefix))
            .map(|(id, entry)| (id.clone(), summary(id, entry)))
            .collect();
        rows.sort_by(|a, b| natural_id(&a.0).cmp(&natural_id(&b.0)));
        rows.into_iter().map(|(_, row)| row).collect()
    }

    /// Stops the session; true once its process tree is proven gone.
    pub(crate) async fn stop(&self, agent_id: &str, id: &str) -> Result<bool, String> {
        enum Handle {
            App(Arc<AppSession>),
            Acp(Arc<AcpSession>),
        }
        let handle = self.with(agent_id, id, |entry| {
            entry.stopped = true;
            entry.finished_at.get_or_insert_with(Instant::now);
            match &entry.kind {
                LiveKind::App(app) => Handle::App(Arc::clone(&app.session)),
                LiveKind::Acp(acp) => {
                    acp.permissions.close();
                    Handle::Acp(Arc::clone(&acp.session))
                }
            }
        })?;
        Ok(match handle {
            Handle::App(session) => session.stop().await,
            Handle::Acp(session) => session.stop().await,
        })
    }

    pub(crate) async fn cancel_acp(&self, agent_id: &str, id: &str) -> Result<(), String> {
        let session = self.with(agent_id, id, |entry| match &entry.kind {
            LiveKind::Acp(acp) => {
                acp.permissions.cancel_waiting();
                Ok(Arc::clone(&acp.session))
            }
            LiveKind::App(_) => Err(format!("{id} is an app session, not a coding agent")),
        })??;
        session.cancel().await.map_err(|error| error.to_string())
    }

    /// Open app and ACP sessions, as `(apps, acp)`, of the agents `owns`
    /// accepts: what a drain waits for, since they die with the daemon.
    pub(crate) fn open_counts(&self, owns: impl Fn(&str) -> bool) -> (usize, usize) {
        self.entries()
            .values()
            .filter(|entry| entry.finished_at.is_none() && owns(&entry.agent_id))
            .fold((0, 0), |(apps, acp), entry| match &entry.kind {
                LiveKind::App(app) if app.session.exit().is_none() => (apps + 1, acp),
                LiveKind::Acp(live)
                    if !matches!(live.session.status(), TurnStatus::Exited { .. }) =>
                {
                    (apps, acp + 1)
                }
                _ => (apps, acp),
            })
    }

    async fn sweep(&self) {
        let mut idle = Vec::new();
        {
            let mut entries = self.entries();
            let now = Instant::now();
            entries.retain(|_, entry| {
                entry
                    .finished_at
                    .is_none_or(|at| now.duration_since(at) < FINISHED_KEEP)
            });
            for (id, entry) in entries.iter_mut() {
                if entry.finished_at.is_some() {
                    continue;
                }
                let ended = match &entry.kind {
                    LiveKind::App(app) => app.session.exit().is_some(),
                    LiveKind::Acp(acp) => {
                        matches!(acp.session.status(), TurnStatus::Exited { .. })
                    }
                };
                if ended {
                    entry.finished_at = Some(now);
                    continue;
                }
                let quiet = match &entry.kind {
                    LiveKind::App(app) => app.session.idle_for(),
                    LiveKind::Acp(acp) if acp.session.status() == TurnStatus::Running => {
                        Duration::ZERO
                    }
                    LiveKind::Acp(_) => now.duration_since(entry.last_used),
                };
                if quiet >= IDLE_STOP {
                    idle.push((entry.agent_id.clone(), id.clone()));
                }
            }
        }
        for (agent_id, id) in idle {
            let _ = self.stop(&agent_id, &id).await;
        }
    }
}

fn natural_id(id: &str) -> (String, u64) {
    let (prefix, number) = id.rsplit_once('-').unwrap_or((id, "0"));
    (prefix.to_string(), number.parse().unwrap_or(0))
}

fn summary(id: &str, entry: &mut Live) -> Value {
    let (status, exit_code) = match &entry.kind {
        LiveKind::App(app) => app_status(entry.stopped, app.session.exit().map(|e| e.code)),
        LiveKind::Acp(acp) => (
            acp_status(
                entry.stopped,
                &acp.session.status(),
                acp.permissions.waiting(),
            ),
            None,
        ),
    };
    json!({
        "id": id,
        "command": entry.command,
        "cwd": entry.cwd,
        "status": status,
        "exitCode": exit_code,
        "startedAt": entry.started_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    })
}

pub(super) fn containment_name(containment: Containment) -> &'static str {
    match containment {
        Containment::JobObject => "job_object",
        Containment::Cgroup => "cgroup",
        Containment::ProcessGroup => "process_group",
        Containment::ProcessTree => "process_tree",
    }
}

fn app_status(stopped: bool, exit_code: Option<u32>) -> (&'static str, Option<u32>) {
    match (stopped, exit_code) {
        (true, code) => ("stopped", code),
        (false, Some(code)) => ("exited", Some(code)),
        (false, None) => ("running", None),
    }
}

fn acp_status(stopped: bool, turn: &TurnStatus, waiting: usize) -> &'static str {
    match turn {
        _ if stopped => "stopped",
        TurnStatus::Exited { .. } => "exited",
        TurnStatus::Running if waiting > 0 => "waiting_for_you",
        TurnStatus::Running => "running",
        _ => "idle",
    }
}

/// The JSON an `app_*` tool returns (the web renders it as a terminal).
pub(super) fn app_snapshot(
    id: &str,
    entry: &mut Live,
    outcome: Option<WaitOutcome>,
) -> Result<Value, String> {
    let LiveKind::App(app) = &mut entry.kind else {
        return Err(format!("{id} is a coding agent; use the acp_ tools"));
    };
    let exit = app.session.exit();
    if exit.is_some() {
        entry.finished_at.get_or_insert_with(Instant::now);
    }
    let output = app.session.output_since(app.read_from, READ_OUTPUT_BYTES);
    app.read_from = output.next;
    let screen: Screen = app.session.screen();
    let (status, exit_code) = app_status(entry.stopped, exit.map(|e| e.code));
    Ok(json!({
        "kind": "app_session",
        "id": id,
        "command": entry.command,
        "cwd": entry.cwd,
        "status": status,
        "exitCode": exit_code,
        "startedAt": entry.started_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "screen": {
            "rows": screen.rows,
            "cursor": [screen.cursor.0, screen.cursor.1],
            "size": [screen.size.0, screen.size.1],
            "alternate": screen.alternate,
        },
        "output": output.text,
        "truncated": output.truncated,
        "outcome": outcome,
        "containment": containment_name(app.session.containment()),
    }))
}

/// The JSON an `acp_*` tool returns: new events since the agent's last read.
pub(super) fn acp_snapshot(id: &str, entry: &mut Live) -> Result<Value, String> {
    let LiveKind::Acp(acp) = &mut entry.kind else {
        return Err(format!("{id} is an app session; use the app_ tools"));
    };
    let turn = acp.session.status();
    if matches!(turn, TurnStatus::Exited { .. }) {
        entry.finished_at.get_or_insert_with(Instant::now);
    }
    let (events, next) = acp.session.events_since(acp.read_from);
    acp.read_from = next;
    let queue = Arc::clone(&acp.permissions);
    let mut budget = READ_EVENT_BYTES;
    let mut shown = Vec::new();
    let mut truncated = acp.session.dropped_events() > 0;
    // Keep the newest events within the byte budget.
    for event in events.iter().rev() {
        let value = event_json(event, &queue);
        let size = value.to_string().len();
        if size > budget {
            truncated = true;
            break;
        }
        budget -= size;
        shown.push(value);
    }
    shown.reverse();
    let stop_reason = match &turn {
        TurnStatus::Finished { stop_reason } => Some(stop_reason.clone()),
        TurnStatus::Failed { message, .. } => Some(format!("error: {message}")),
        _ => None,
    };
    let info = acp.session.agent_info();
    Ok(json!({
        "kind": "acp_session",
        "id": id,
        "command": entry.command,
        "cwd": entry.cwd,
        "agent": {
            "name": acp.agent_name,
            "version": info.version,
        },
        "sessionId": acp.session.session_id(),
        "status": acp_status(entry.stopped, &turn, acp.permissions.waiting()),
        "stopReason": stop_reason,
        "events": shown,
        "truncated": truncated,
        "containment": containment_name(acp.session.containment()),
    }))
}

fn event_json(event: &AcpEvent, queue: &PermissionQueue) -> Value {
    match event {
        AcpEvent::AgentMessage { text } => json!({"type": "agent_message", "text": text}),
        AcpEvent::AgentThought { text } => json!({"type": "agent_thought", "text": text}),
        AcpEvent::UserMessage { text } => json!({"type": "user_message", "text": text}),
        AcpEvent::ToolCall {
            id,
            title,
            kind,
            status,
            locations,
        } => json!({
            "type": "tool_call", "id": id, "title": title, "toolKind": kind,
            "status": status, "locations": locations,
        }),
        AcpEvent::ToolCallUpdate {
            id,
            status,
            content_text,
        } => json!({"type": "tool_call_update", "id": id, "status": status, "text": content_text}),
        AcpEvent::Plan { entries } => json!({"type": "plan", "entries": entries}),
        AcpEvent::PermissionAsked {
            tool_call_id,
            title,
            kind,
            ..
        } => json!({
            "type": "permission_asked", "toolCallId": tool_call_id, "title": title, "toolKind": kind,
        }),
        AcpEvent::PermissionAnswered {
            tool_call_id,
            option_id,
        } => json!({
            "type": "permission_answered", "toolCallId": tool_call_id,
            "decision": match option_id {
                None => "cancelled",
                Some(_) => queue.decision_label(tool_call_id),
            },
        }),
        AcpEvent::FileWritten { path } => json!({"type": "file_written", "path": path}),
        AcpEvent::TurnEnded { stop_reason } => {
            json!({"type": "turn_ended", "stopReason": stop_reason})
        }
        AcpEvent::TurnFailed { code, message } => {
            json!({"type": "turn_failed", "code": code, "message": message})
        }
        AcpEvent::Exited { code } => json!({"type": "exited", "code": code}),
        AcpEvent::Stderr { text } => json!({"type": "stderr", "text": text}),
    }
}

// --- Previews ----------------------------------------------------------------

/// A session result cut to `max` bytes and still valid JSON, so the chat can
/// draw the card from a live preview: the newest screen rows or events stay.
/// `None` when `result` is not a session result.
pub(crate) fn session_preview(result: &str, max: usize) -> Option<String> {
    let mut value: Value = serde_json::from_str(result).ok()?;
    let kind = value.get("kind")?.as_str()?.to_string();
    if !matches!(kind.as_str(), "app_session" | "acp_session") {
        return None;
    }
    let fits = |value: &Value| value.to_string().len() <= max;
    if fits(&value) {
        return Some(value.to_string());
    }
    value["truncated"] = json!(true);
    if kind == "app_session" {
        value["output"] = json!("");
        loop {
            if fits(&value) {
                return Some(value.to_string());
            }
            let rows = value.pointer_mut("/screen/rows")?.as_array_mut()?;
            if rows.is_empty() {
                break;
            }
            // Keep the bottom of the screen, where prompts and status sit.
            rows.remove(0);
        }
    } else {
        loop {
            if fits(&value) {
                return Some(value.to_string());
            }
            let events = value.get_mut("events")?.as_array_mut()?;
            if events.is_empty() {
                break;
            }
            events.remove(0);
        }
        // A single huge message: shorten texts.
        if let Some(events) = value.get_mut("events").and_then(Value::as_array_mut) {
            for event in events {
                if let Some(Value::String(text)) = event.get_mut("text") {
                    let cut: String = text.chars().take(200).collect();
                    *text = cut;
                }
            }
        }
    }
    for key in ["cwd", "command"] {
        if !fits(&value) {
            if let Some(Value::String(text)) = value.get_mut(key) {
                let cut: String = text.chars().take(60).collect();
                *text = cut;
            }
        }
    }
    fits(&value).then(|| value.to_string())
}

// --- ACP permissions ---------------------------------------------------------

/// What the agent is waiting on: a permission prompt or an unasked write.
pub(super) enum Pending {
    Permission(PermissionRequest),
    Write(PathBuf),
}

pub(super) struct PendingItem {
    pub(super) what: Pending,
    pub(super) reply: oneshot::Sender<Decision>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Decision {
    Allow,
    Deny,
    Cancel,
}

/// Requests from one ACP agent, waiting for a tool call to judge them.
pub(super) struct PermissionQueue {
    sender: mpsc::UnboundedSender<PendingItem>,
    receiver: tokio::sync::Mutex<mpsc::UnboundedReceiver<PendingItem>>,
    waiting: std::sync::atomic::AtomicUsize,
    /// Paths the owner already let the agent edit this session, so its write
    /// does not ask a second time.
    granted_paths: Mutex<HashSet<PathBuf>>,
    /// How each permission was decided, by the agent's tool-call id.
    decisions: Mutex<HashMap<String, Decision>>,
    closed: std::sync::atomic::AtomicBool,
}

impl PermissionQueue {
    pub(super) fn new() -> Arc<Self> {
        let (sender, receiver) = mpsc::unbounded_channel();
        Arc::new(Self {
            sender,
            receiver: tokio::sync::Mutex::new(receiver),
            waiting: Default::default(),
            granted_paths: Mutex::new(HashSet::new()),
            decisions: Mutex::new(HashMap::new()),
            closed: Default::default(),
        })
    }

    pub(super) fn waiting(&self) -> usize {
        self.waiting.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn close(&self) {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        self.cancel_waiting();
    }

    /// Answers everything queued with `cancelled` (the protocol's answer to a
    /// cancelled turn).
    fn cancel_waiting(&self) {
        if let Ok(mut receiver) = self.receiver.try_lock() {
            while let Ok(item) = receiver.try_recv() {
                self.waiting
                    .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                let _ = item.reply.send(Decision::Cancel);
            }
        }
    }

    async fn ask(&self, what: Pending) -> Decision {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Decision::Cancel;
        }
        let (reply, answer) = oneshot::channel();
        self.waiting
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.sender.send(PendingItem { what, reply }).is_err() {
            self.waiting
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            return Decision::Cancel;
        }
        answer.await.unwrap_or(Decision::Cancel)
    }

    /// Takes the next request, waiting until `deadline`.
    pub(super) async fn next(&self, deadline: tokio::time::Instant) -> Option<PendingItem> {
        let mut receiver = self.receiver.lock().await;
        let item = tokio::time::timeout_at(deadline, receiver.recv())
            .await
            .ok()
            .flatten()?;
        self.waiting
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        Some(item)
    }

    pub(super) fn grant(&self, paths: &[PathBuf]) {
        self.granted_paths
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .extend(paths.iter().cloned());
    }

    fn record(&self, tool_call_id: &str, decision: Decision) {
        self.decisions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(tool_call_id.to_string(), decision);
    }

    fn decision_label(&self, tool_call_id: &str) -> &'static str {
        match self
            .decisions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(tool_call_id)
        {
            Some(Decision::Deny) => "denied",
            Some(Decision::Cancel) => "cancelled",
            _ => "allowed",
        }
    }

    fn granted(&self, path: &Path) -> bool {
        self.granted_paths
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(path)
    }
}

/// The daemon's side of an ACP session: every decision goes to the queue.
pub(super) struct DaemonAcpHost {
    pub(super) queue: Arc<PermissionQueue>,
}

impl AcpClientHost for DaemonAcpHost {
    fn request_permission(&self, request: PermissionRequest) -> BoxFuture<'static, Option<String>> {
        let queue = Arc::clone(&self.queue);
        Box::pin(async move {
            let options = request.options.clone();
            let paths = request.locations.clone();
            let is_edit = matches!(
                request.kind,
                ToolKind::Edit | ToolKind::Delete | ToolKind::Move
            );
            let tool_call_id = request.tool_call_id.clone();
            let decision = queue.ask(Pending::Permission(request)).await;
            queue.record(&tool_call_id, decision);
            let pick = |kind: PermissionOptionKind| {
                options
                    .iter()
                    .find(|option| option.kind == kind)
                    .map(|option| option.id.clone())
            };
            match decision {
                Decision::Allow => {
                    if is_edit {
                        queue.grant(&paths);
                    }
                    // Never "allow always": standing grants are the owner's
                    // rules, made in Anima.
                    pick(PermissionOptionKind::AllowOnce)
                }
                Decision::Deny => pick(PermissionOptionKind::RejectOnce)
                    .or_else(|| pick(PermissionOptionKind::RejectAlways)),
                Decision::Cancel => None,
            }
        })
    }

    fn approve_write(&self, path: &Path) -> BoxFuture<'static, Result<(), String>> {
        let queue = Arc::clone(&self.queue);
        let path = path.to_path_buf();
        Box::pin(async move {
            if queue.granted(&path) {
                return Ok(());
            }
            match queue.ask(Pending::Write(path.clone())).await {
                Decision::Allow => {
                    queue.grant(std::slice::from_ref(&path));
                    Ok(())
                }
                _ => Err("the owner's approval policy did not allow this write".to_string()),
            }
        })
    }
}

/// The approval call a request becomes: a name the policy table classifies
/// and arguments its matchers read.
pub(super) fn approval_call(
    agent_name: &str,
    session_id: &str,
    what: &Pending,
) -> Option<ToolCall> {
    let mut args = std::collections::BTreeMap::new();
    args.insert(
        "agent".to_string(),
        DataValue::String(agent_name.to_string()),
    );
    args.insert(
        "sessionId".to_string(),
        DataValue::String(session_id.to_string()),
    );
    let (name, title, locations, raw) = match what {
        Pending::Write(path) => (
            "acp_edit",
            format!("Write {}", path.display()),
            vec![path.clone()],
            None,
        ),
        Pending::Permission(request) => {
            let name = match request.kind {
                // Reading never asks (spec §7.2).
                ToolKind::Read | ToolKind::Search | ToolKind::Think => return None,
                ToolKind::Edit | ToolKind::Delete | ToolKind::Move => "acp_edit",
                ToolKind::Execute => "acp_execute",
                ToolKind::Fetch => "acp_fetch",
                ToolKind::SwitchMode | ToolKind::Other => "acp_other",
            };
            (
                name,
                request.title.clone(),
                request.locations.clone(),
                request.raw_input.clone(),
            )
        }
    };
    args.insert("title".to_string(), DataValue::String(title));
    let first = locations.first().map(|path| path.display().to_string());
    match name {
        "acp_edit" => {
            if let Some(path) = first {
                args.insert("file_path".to_string(), DataValue::String(path));
            }
        }
        "acp_execute" => {
            if let Some(command) = raw_string(raw.as_ref(), &["command", "cmd"]) {
                args.insert("command".to_string(), DataValue::String(command));
            }
        }
        "acp_fetch" => {
            if let Some(url) = raw_string(raw.as_ref(), &["url", "uri"]) {
                args.insert("url".to_string(), DataValue::String(url));
            }
        }
        _ => {}
    }
    args.insert(
        "locations".to_string(),
        DataValue::Array(
            locations
                .iter()
                .map(|path| DataValue::String(path.display().to_string()))
                .collect(),
        ),
    );
    Some(ToolCall {
        id: format!("acp-permission-{}", uuid::Uuid::new_v4()),
        name: name.to_string(),
        args,
    })
}

/// A command may arrive as a string or as argv.
fn raw_string(raw: Option<&Value>, keys: &[&str]) -> Option<String> {
    let raw = raw?;
    keys.iter().find_map(|key| match raw.get(key)? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    })
}

/// Judges queued requests through the run's approval gate until `deadline`
/// or until `done` resolves. Without a gate (no owner to ask) every request
/// that would need one is cancelled.
pub(super) async fn judge_until(
    queue: &PermissionQueue,
    gate: Option<&crate::approvals::ApprovalGate>,
    agent: &AgentState,
    agent_name: &str,
    session_id: &str,
    deadline: tokio::time::Instant,
    done: impl std::future::Future<Output = ()>,
) {
    tokio::pin!(done);
    loop {
        let item = tokio::select! {
            () = &mut done => return,
            item = queue.next(deadline) => match item {
                Some(item) => item,
                None => return,
            },
        };
        let decision = match approval_call(agent_name, session_id, &item.what) {
            None => Decision::Allow,
            Some(call) => match gate {
                None => Decision::Cancel,
                Some(gate) => match gate.check(agent, &call).await {
                    crate::approvals::GateOutcome::Proceed
                    | crate::approvals::GateOutcome::Approved => Decision::Allow,
                    crate::approvals::GateOutcome::Refuse(_) => Decision::Deny,
                },
            },
        };
        let _ = item.reply.send(decision);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(kind: ToolKind, raw: Option<Value>) -> PermissionRequest {
        PermissionRequest {
            tool_call_id: "t1".into(),
            title: "Edit src/upload.ts".into(),
            kind,
            locations: vec![PathBuf::from("/w/src/upload.ts")],
            raw_input: raw,
            options: Vec::new(),
        }
    }

    #[test]
    fn requests_become_calls_the_policy_table_classifies() {
        use crate::approvals::{policy::risk_class, RiskClass};
        let edit = approval_call(
            "Codex",
            "acp-1",
            &Pending::Permission(request(ToolKind::Edit, None)),
        )
        .unwrap();
        assert_eq!(edit.name, "acp_edit");
        assert_eq!(risk_class(&edit.name), RiskClass::Write);
        assert_eq!(
            edit.args.get("file_path"),
            Some(&DataValue::String("/w/src/upload.ts".into()))
        );
        let run = approval_call(
            "Codex",
            "acp-1",
            &Pending::Permission(request(
                ToolKind::Execute,
                Some(json!({"command": ["npm", "test"]})),
            )),
        )
        .unwrap();
        assert_eq!(run.name, "acp_execute");
        assert_eq!(risk_class(&run.name), RiskClass::Exec);
        assert_eq!(
            run.args.get("command"),
            Some(&DataValue::String("npm test".into()))
        );
        let fetch = approval_call(
            "Codex",
            "acp-1",
            &Pending::Permission(request(
                ToolKind::Fetch,
                Some(json!({"url": "https://docs.rs"})),
            )),
        )
        .unwrap();
        assert_eq!(risk_class(&fetch.name), RiskClass::Network);
        assert!(approval_call(
            "Codex",
            "acp-1",
            &Pending::Permission(request(ToolKind::Read, None))
        )
        .is_none());
        let write =
            approval_call("Codex", "acp-1", &Pending::Write(PathBuf::from("/w/a.txt"))).unwrap();
        assert_eq!(write.name, "acp_edit");
    }

    #[tokio::test]
    async fn without_a_gate_requests_that_would_ask_are_cancelled_and_reads_pass() {
        let queue = PermissionQueue::new();
        let host = DaemonAcpHost {
            queue: Arc::clone(&queue),
        };
        let mut edit = request(ToolKind::Edit, None);
        edit.options = vec![
            anima_acp::PermissionOption {
                id: "allow".into(),
                name: "Allow".into(),
                kind: PermissionOptionKind::AllowOnce,
            },
            anima_acp::PermissionOption {
                id: "always".into(),
                name: "Always".into(),
                kind: PermissionOptionKind::AllowAlways,
            },
        ];
        let mut read = request(ToolKind::Read, None);
        read.options = edit.options.clone();
        let edit_answer = tokio::spawn(host.request_permission(edit));
        let read_answer = tokio::spawn(host.request_permission(read));
        let agent: AgentState = serde_json::from_value(json!({
            "id": "agent-live-test", "name": "t", "status": "Idle",
            "config": {"name": "t", "model": "test"}, "created_at_ms": 0,
            "token_usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
        }))
        .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(300);
        judge_until(
            &queue,
            None,
            &agent,
            "Codex",
            "acp-1",
            deadline,
            std::future::pending(),
        )
        .await;
        assert_eq!(edit_answer.await.unwrap(), None);
        // A read is allowed once, never always.
        assert_eq!(read_answer.await.unwrap(), Some("allow".to_string()));
        assert_eq!(queue.waiting(), 0);
    }

    #[test]
    fn previews_keep_the_bottom_of_the_screen_and_stay_json() {
        let rows: Vec<String> = (0..40)
            .map(|row| format!("row {row} {}", "x".repeat(90)))
            .collect();
        let full = json!({
            "kind": "app_session", "id": "app-1", "command": "claude", "cwd": "/w",
            "status": "running", "screen": {"rows": rows, "cursor": [39, 0], "size": [40, 120], "alternate": true},
            "output": "y".repeat(10_000), "truncated": false,
        })
        .to_string();
        let preview = session_preview(&full, 2048).unwrap();
        assert!(preview.len() <= 2048);
        let value: Value = serde_json::from_str(&preview).unwrap();
        assert_eq!(value["truncated"], json!(true));
        let kept = value["screen"]["rows"].as_array().unwrap();
        assert!(!kept.is_empty());
        assert!(kept
            .last()
            .unwrap()
            .as_str()
            .unwrap()
            .starts_with("row 39 "));
        let events: Vec<Value> = (0..50)
            .map(|n| json!({"type": "agent_message", "text": format!("message {n} {}", "z".repeat(100))}))
            .collect();
        let acp = json!({"kind": "acp_session", "id": "acp-1", "events": events}).to_string();
        let value: Value = serde_json::from_str(&session_preview(&acp, 2048).unwrap()).unwrap();
        assert!(value["events"].as_array().unwrap().last().unwrap()["text"]
            .as_str()
            .unwrap()
            .starts_with("message 49 "));
        assert!(session_preview("plain text", 2048).is_none());
    }

    #[test]
    fn answers_are_labelled_by_how_they_were_decided() {
        let queue = PermissionQueue::new();
        queue.record("t1", Decision::Deny);
        queue.record("t2", Decision::Allow);
        assert_eq!(queue.decision_label("t1"), "denied");
        assert_eq!(queue.decision_label("t2"), "allowed");
    }
}
