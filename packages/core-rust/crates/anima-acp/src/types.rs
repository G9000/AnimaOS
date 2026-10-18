//! Public, host-facing types. Wire shapes stay private to `session.rs`.
use serde::Serialize;
use std::{fmt, future::Future, path::Path, path::PathBuf, pin::Pin};

/// A boxed `Send` future, so host trait objects need no `async_trait`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How to run an ACP agent. `cwd` becomes the session root that every
/// `fs/*` request is confined to.
#[derive(Clone, Debug)]
pub struct AcpLaunch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// Start from an empty environment, so only `env` reaches the agent.
    pub env_clear: bool,
    /// Resume this agent session with `session/load` instead of starting one.
    pub resume_session_id: Option<String>,
    /// Session mode ids, in preference order, to switch to after the session
    /// opens when the agent offers one: the modes in which it asks before
    /// acting (Claude Code's `default`), so its requests reach the host.
    pub ask_modes: Vec<String>,
}

/// Decisions the crate cannot make itself. Hosts map them onto their own
/// approval policy; the crate never answers a permission on its own.
pub trait AcpClientHost: Send + Sync + 'static {
    /// Decide a permission request. Return one of the offered option ids, or
    /// None to answer `cancelled`.
    fn request_permission(&self, request: PermissionRequest) -> BoxFuture<'static, Option<String>>;
    /// Called before an `fs/write_text_file` inside cwd is applied, with the
    /// resolved path. Err(reason) refuses the write.
    fn approve_write(&self, path: &Path) -> BoxFuture<'static, Result<(), String>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    SwitchMode,
    Other,
}

impl ToolKind {
    /// Unknown or missing kinds are `Other`, as the protocol defines.
    pub(crate) fn parse(value: Option<&str>) -> Self {
        match value {
            Some("read") => Self::Read,
            Some("edit") => Self::Edit,
            Some("delete") => Self::Delete,
            Some("move") => Self::Move,
            Some("search") => Self::Search,
            Some("execute") => Self::Execute,
            Some("think") => Self::Think,
            Some("fetch") => Self::Fetch,
            Some("switch_mode") => Self::SwitchMode,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl ToolCallStatus {
    pub(crate) fn parse(value: Option<&str>) -> Option<Self> {
        match value? {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

impl PermissionOptionKind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "allow_once" => Some(Self::AllowOnce),
            "allow_always" => Some(Self::AllowAlways),
            "reject_once" => Some(Self::RejectOnce),
            "reject_always" => Some(Self::RejectAlways),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
    pub kind: PermissionOptionKind,
}

#[derive(Clone, Debug, Serialize)]
pub struct PermissionRequest {
    pub tool_call_id: String,
    pub title: String,
    pub kind: ToolKind,
    pub locations: Vec<PathBuf>,
    pub raw_input: Option<serde_json::Value>,
    pub options: Vec<PermissionOption>,
}

/// Priority and status stay strings: they are display-only and the protocol
/// may add values without a version bump.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlanEntry {
    pub content: String,
    pub priority: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AuthMethod {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct AgentInfo {
    pub name: Option<String>,
    pub title: Option<String>,
    pub version: Option<String>,
    pub protocol_version: u16,
    pub load_session: bool,
    pub prompt_image: bool,
    pub prompt_audio: bool,
    pub prompt_embedded_context: bool,
    pub auth_methods: Vec<AuthMethod>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AcpEvent {
    AgentMessage {
        text: String,
    },
    AgentThought {
        text: String,
    },
    UserMessage {
        text: String,
    },
    ToolCall {
        id: String,
        title: String,
        kind: ToolKind,
        status: ToolCallStatus,
        locations: Vec<PathBuf>,
    },
    ToolCallUpdate {
        id: String,
        status: Option<ToolCallStatus>,
        content_text: Option<String>,
    },
    Plan {
        entries: Vec<PlanEntry>,
    },
    PermissionAsked {
        tool_call_id: String,
        title: String,
        kind: ToolKind,
        options: Vec<PermissionOption>,
    },
    PermissionAnswered {
        tool_call_id: String,
        option_id: Option<String>,
    },
    FileWritten {
        path: PathBuf,
    },
    TurnEnded {
        stop_reason: String,
    },
    TurnFailed {
        code: i64,
        message: String,
    },
    Exited {
        code: Option<i32>,
    },
    /// Agent stderr, plus `[acp]`-prefixed diagnostics such as malformed lines.
    Stderr {
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TurnStatus {
    /// No prompt has been sent yet.
    Idle,
    Running,
    Finished {
        stop_reason: String,
    },
    /// The agent answered `session/prompt` with a JSON-RPC error.
    Failed {
        code: i64,
        message: String,
    },
    Exited {
        code: Option<i32>,
    },
}

/// Identifies a started turn. `cursor` is the event cursor at its start, so a
/// caller can read just this turn's events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PromptHandle {
    pub turn: u64,
    pub cursor: u64,
}

#[derive(Debug)]
pub enum AcpError {
    /// The program could not be started (missing, not executable, bad cwd).
    Spawn(String),
    Timeout {
        stage: &'static str,
    },
    /// The agent wants a login. We never authenticate on the owner's behalf.
    AuthRequired {
        methods: Vec<AuthMethod>,
    },
    /// A resume id was given but the agent does not advertise `loadSession`.
    ResumeUnsupported,
    /// A turn is already running; ACP allows one at a time per session.
    Busy,
    Rpc {
        code: i64,
        message: String,
    },
    Protocol(String),
    /// The agent process is gone. `stderr` is its last output, for diagnosis.
    Exited {
        code: Option<i32>,
        stderr: String,
    },
}

impl fmt::Display for AcpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(message) => write!(f, "could not start ACP agent: {message}"),
            Self::Timeout { stage } => write!(f, "ACP agent did not answer {stage} in time"),
            Self::AuthRequired { methods } => {
                let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
                write!(f, "ACP agent needs a login ({})", names.join(", "))
            }
            Self::ResumeUnsupported => write!(f, "ACP agent cannot resume sessions"),
            Self::Busy => write!(f, "a prompt turn is already running"),
            Self::Rpc { code, message } => write!(f, "ACP agent error {code}: {message}"),
            Self::Protocol(message) => write!(f, "ACP protocol error: {message}"),
            Self::Exited { code, stderr } => {
                match code {
                    Some(code) => write!(f, "ACP agent exited with code {code}")?,
                    None => write!(f, "ACP agent exited")?,
                }
                if !stderr.is_empty() {
                    write!(f, ": {stderr}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for AcpError {}
