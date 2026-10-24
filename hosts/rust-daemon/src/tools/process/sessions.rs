//! `app_*` and `acp_*` tools: open an interactive app or a coding agent, keep
//! it across turns, and stop it with its whole process tree.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anima_acp::{AcpError, AcpLaunch, AcpSession, TurnStatus};
use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use anima_process::session::{AppSession, AppSessionConfig, Key, Until, WaitFor};
use futures::future::BoxFuture;
use serde_json::{json, Value};

use super::super::workspace::{
    canonical_workspace_root, resolve_workspace_search_root, workspace_root_path,
};
use super::super::{ctx_workspace_root, ToolExecutionContext};
use super::live::{
    acp_snapshot, app_snapshot, judge_until, live_sessions, AcpLive, AppLive, DaemonAcpHost, Live,
    LiveKind, PermissionQueue,
};
use super::shell::{resolve_shell_launcher, revalidate_shell_launch, shell_env};

const WAIT_MAX_MS: u64 = 120_000;
const ACP_WAIT_DEFAULT_MS: u64 = 30_000;
const APP_READ_UNTIL_DEFAULT_MS: u64 = 30_000;
const MAX_UNTIL_CHARS: usize = 256;
const REGEX_SIZE_LIMIT: usize = 1 << 16;
/// How long output must stay quiet before a read calls the screen settled.
const SETTLE: Duration = Duration::from_millis(700);
/// A first look after start or typing, so the reply shows what happened.
const GLANCE: Duration = Duration::from_secs(5);
const MAX_TEXT_CHARS: usize = 32 * 1024;

fn reply(result: Result<Value, String>) -> TaskResult<Content> {
    match result {
        Ok(value) => TaskResult::success(
            Content {
                text: value.to_string(),
                attachments: None,
                metadata: None,
            },
            0,
        ),
        Err(error) => TaskResult::error(error, 0),
    }
}

fn string_arg(call: &ToolCall, key: &str) -> Result<Option<String>, String> {
    match call.args.get(key) {
        Some(DataValue::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(DataValue::String(_)) | None | Some(DataValue::Null) => Ok(None),
        Some(_) => Err(format!("{} {key} must be a string", call.name)),
    }
}

fn required_arg(call: &ToolCall, key: &str) -> Result<String, String> {
    string_arg(call, key)?
        .map(|value| value.trim().to_string())
        .ok_or_else(|| format!("{} {key} must be a non-empty string", call.name))
}

fn integer_arg(call: &ToolCall, key: &str, range: (u64, u64)) -> Result<Option<u64>, String> {
    match call.args.get(key) {
        None | Some(DataValue::Null) => Ok(None),
        Some(DataValue::Number(value))
            if value.is_finite() && value.fract() == 0.0 && *value >= 0.0 =>
        {
            let value = *value as u64;
            if value < range.0 || value > range.1 {
                return Err(format!(
                    "{} {key} must be between {} and {}",
                    call.name, range.0, range.1
                ));
            }
            Ok(Some(value))
        }
        Some(_) => Err(format!("{} {key} must be a whole number", call.name)),
    }
}

/// A literal, or a regex between slashes, as `bg_wait` takes it.
fn until_arg(call: &ToolCall) -> Result<Option<Until>, String> {
    let Some(value) = string_arg(call, "until")? else {
        return Ok(None);
    };
    if value.chars().count() > MAX_UNTIL_CHARS {
        return Err(format!(
            "{} until must be at most {MAX_UNTIL_CHARS} characters",
            call.name
        ));
    }
    match value
        .strip_prefix('/')
        .and_then(|rest| rest.strip_suffix('/'))
    {
        Some(pattern) if !pattern.is_empty() => regex::RegexBuilder::new(pattern)
            .size_limit(REGEX_SIZE_LIMIT)
            .build()
            .map(|regex| Some(Until::Pattern(regex)))
            .map_err(|error| format!("{} until is not a valid regex: {error}", call.name)),
        _ => Ok(Some(Until::Text(value))),
    }
}

/// The working folder inside the workspace, absolute.
fn session_cwd(context: &ToolExecutionContext, call: &ToolCall) -> Result<PathBuf, String> {
    let cwd = string_arg(call, "cwd")?.unwrap_or_else(|| ".".to_string());
    let root = workspace_root_path(&call.name, ctx_workspace_root(context))?;
    resolve_workspace_search_root(
        &canonical_workspace_root(&root, &call.name)?,
        cwd.trim(),
        &call.name,
    )
}

/// The child's whole environment: the shell allowlist, a capable terminal,
/// and the keys the owner chose to hand this agent's apps.
fn session_env(agent_id: &str, terminal: bool) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = shell_env()
        .into_iter()
        .filter(|(name, _)| !terminal || *name != "TERM")
        .map(|(name, value)| (name.to_string(), value.to_string_lossy().into_owned()))
        .collect();
    if terminal {
        env.push(("TERM".to_string(), "xterm-256color".to_string()));
        env.push(("COLORTERM".to_string(), "truecolor".to_string()));
    }
    env.extend(crate::coding_keys::child_env(agent_id));
    env
}

fn new_live(agent: &AgentState, command: &str, cwd: PathBuf, kind: LiveKind) -> Live {
    Live {
        agent_id: agent.id.to_string(),
        command: command.to_string(),
        cwd,
        started_at: chrono::Utc::now(),
        last_used: Instant::now(),
        finished_at: None,
        stopped: false,
        kind,
    }
}

fn app_of(agent_id: &str, id: &str) -> Result<Arc<AppSession>, String> {
    live_sessions().with(agent_id, id, |entry| match &entry.kind {
        LiveKind::App(app) => Ok(Arc::clone(&app.session)),
        LiveKind::Acp(_) => Err(format!("{id} is a coding agent; use the acp_ tools")),
    })?
}

fn app_snapshot_of(
    agent_id: &str,
    id: &str,
    outcome: Option<anima_process::session::WaitOutcome>,
) -> Result<Value, String> {
    live_sessions().with(agent_id, id, |entry| app_snapshot(id, entry, outcome))?
}

// --- app_* -------------------------------------------------------------------

pub(in super::super) fn execute_app_start(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move { reply(app_start(&context, &agent, &call).await) })
}

async fn app_start(
    context: &ToolExecutionContext,
    agent: &AgentState,
    call: &ToolCall,
) -> Result<Value, String> {
    let command = required_arg(call, "command")?;
    let cols = integer_arg(call, "cols", (20, 300))?.unwrap_or(120) as u16;
    let rows = integer_arg(call, "rows", (5, 100))?.unwrap_or(40) as u16;
    let cwd = session_cwd(context, call)?;
    let agent_id = agent.id.to_string();
    live_sessions().check_capacity(&agent_id)?;
    let (shell, flags) = resolve_shell_launcher()?;
    let mut args = flags.clone();
    args.push(command.clone());
    let mut config = AppSessionConfig::new(shell.clone(), args, &cwd);
    config.env = session_env(&agent_id, true);
    config.cols = cols;
    config.rows = rows;
    let binding = context.command_binding.as_deref().ok_or_else(|| {
        "Execution identity binding is missing; the app did not start".to_string()
    })?;
    revalidate_shell_launch(Some(binding), &shell, &flags, &command, &cwd)?;
    let session = Arc::new(
        AppSession::start_contained(config)
            .map_err(|error| format!("app_start failed to start: {error}"))?,
    );
    let id = live_sessions().insert(
        "app",
        new_live(
            agent,
            &command,
            cwd,
            LiveKind::App(AppLive {
                session: Arc::clone(&session),
                read_from: 0,
            }),
        ),
    );
    let outcome = session
        .wait(
            0,
            &WaitFor {
                until: None,
                settle: Some(SETTLE),
                max: GLANCE,
            },
        )
        .await;
    app_snapshot_of(&agent_id, &id, Some(outcome))
}

pub(in super::super) fn execute_app_send(
    _context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move { reply(app_send(&agent, &call).await) })
}

fn keys_arg(call: &ToolCall) -> Result<Vec<Key>, String> {
    let names = match call.args.get("keys") {
        None | Some(DataValue::Null) => return Ok(Vec::new()),
        Some(DataValue::String(name)) => vec![name.clone()],
        Some(DataValue::Array(items)) => items
            .iter()
            .map(|item| match item {
                DataValue::String(name) => Ok(name.clone()),
                _ => Err("app_send keys must be key names".to_string()),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("app_send keys must be a list of key names".to_string()),
    };
    if names.len() > 64 {
        return Err("app_send takes at most 64 keys at once".to_string());
    }
    names
        .iter()
        .map(|name| {
            Key::parse(name).ok_or_else(|| {
                format!(
                    "app_send does not know the key {name:?}; use one of {}",
                    Key::NAMES.join(", ")
                )
            })
        })
        .collect()
}

async fn app_send(agent: &AgentState, call: &ToolCall) -> Result<Value, String> {
    let id = required_arg(call, "id")?;
    let text = string_arg(call, "text")?;
    if text
        .as_ref()
        .is_some_and(|text| text.chars().count() > MAX_TEXT_CHARS)
    {
        return Err(format!(
            "app_send text must be at most {MAX_TEXT_CHARS} characters"
        ));
    }
    let keys = keys_arg(call)?;
    if text.is_none() && keys.is_empty() {
        return Err("app_send needs text, keys, or both".to_string());
    }
    let agent_id = agent.id.to_string();
    let session = app_of(&agent_id, &id)?;
    if session.exit().is_some() {
        return Err(format!(
            "{id} has already exited; app_read shows how it ended"
        ));
    }
    let from = session.output_end();
    session
        .send(text.as_deref(), &keys)
        .map_err(|error| format!("app_send could not type into {id}: {error}"))?;
    let outcome = session
        .wait(
            from,
            &WaitFor {
                until: None,
                settle: Some(SETTLE),
                max: GLANCE,
            },
        )
        .await;
    app_snapshot_of(&agent_id, &id, Some(outcome))
}

pub(in super::super) fn execute_app_read(
    _context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move { reply(app_read(&agent, &call).await) })
}

async fn app_read(agent: &AgentState, call: &ToolCall) -> Result<Value, String> {
    let id = required_arg(call, "id")?;
    let wait_ms = integer_arg(call, "wait_ms", (0, WAIT_MAX_MS))?;
    let until = until_arg(call)?;
    let agent_id = agent.id.to_string();
    let session = app_of(&agent_id, &id)?;
    let from = live_sessions().with(&agent_id, &id, |entry| match &entry.kind {
        LiveKind::App(app) => app.read_from,
        LiveKind::Acp(_) => 0,
    })?;
    let wait = match (until, wait_ms) {
        (Some(until), wait_ms) => Some(WaitFor {
            until: Some(until),
            settle: None,
            max: Duration::from_millis(wait_ms.unwrap_or(APP_READ_UNTIL_DEFAULT_MS)),
        }),
        (None, Some(ms)) if ms > 0 => Some(WaitFor {
            until: None,
            settle: Some(SETTLE),
            max: Duration::from_millis(ms),
        }),
        (None, _) => None,
    };
    let outcome = match wait {
        Some(wait) => Some(session.wait(from, &wait).await),
        None => None,
    };
    app_snapshot_of(&agent_id, &id, outcome)
}

pub(in super::super) fn execute_app_stop(
    _context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        reply(
            async {
                let id = required_arg(&call, "id")?;
                let agent_id = agent.id.to_string();
                app_of(&agent_id, &id)?;
                let gone = live_sessions().stop(&agent_id, &id).await?;
                let mut value = app_snapshot_of(&agent_id, &id, None)?;
                value["treeGone"] = json!(gone);
                Ok(value)
            }
            .await,
        )
    })
}

pub(in super::super) fn execute_app_list(
    _context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    _call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let sessions = live_sessions().list(&agent.id.to_string(), "app-");
        reply(Ok(json!({"kind": "app_sessions", "sessions": sessions})))
    })
}

// --- acp_* -------------------------------------------------------------------

struct AcpHandle {
    session: Arc<AcpSession>,
    queue: Arc<PermissionQueue>,
    agent_name: String,
}

fn acp_of(agent_id: &str, id: &str) -> Result<AcpHandle, String> {
    live_sessions().with(agent_id, id, |entry| match &entry.kind {
        LiveKind::Acp(acp) => Ok(AcpHandle {
            session: Arc::clone(&acp.session),
            queue: Arc::clone(&acp.permissions),
            agent_name: acp.agent_name.clone(),
        }),
        LiveKind::App(_) => Err(format!("{id} is an app session; use the app_ tools")),
    })?
}

fn acp_snapshot_of(agent_id: &str, id: &str) -> Result<Value, String> {
    live_sessions().with(agent_id, id, |entry| acp_snapshot(id, entry))?
}

fn acp_error(command: &str, error: AcpError) -> String {
    match error {
        AcpError::AuthRequired { methods } => {
            let names = methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{command} needs a sign-in first ({names}). Ask the owner to sign in to it on this computer, then start it again. Never type credentials for them."
            )
        }
        AcpError::ResumeUnsupported => {
            format!("{command} cannot resume earlier sessions; start without session_id")
        }
        AcpError::Busy => {
            "the coding agent is still working on the last prompt; acp_read it or acp_cancel first"
                .to_string()
        }
        other => format!("{command}: {other}"),
    }
}

/// Judges the agent's permission requests until the turn stops running or
/// `wait` runs out.
async fn pump(
    context: &ToolExecutionContext,
    agent: &AgentState,
    id: &str,
    handle: &AcpHandle,
    wait: Duration,
) {
    let deadline = tokio::time::Instant::now() + wait;
    let session = Arc::clone(&handle.session);
    let turn_done = async move {
        if session.status() == TurnStatus::Running {
            session.wait(wait).await;
        }
    };
    judge_until(
        &handle.queue,
        context.approvals.as_ref(),
        agent,
        &handle.agent_name,
        id,
        deadline,
        turn_done,
    )
    .await;
}

pub(in super::super) fn execute_acp_start(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move { reply(acp_start(&context, &agent, &call).await) })
}

async fn acp_start(
    context: &ToolExecutionContext,
    agent: &AgentState,
    call: &ToolCall,
) -> Result<Value, String> {
    let command = required_arg(call, "command")?;
    let resume = string_arg(call, "session_id")?;
    let cwd = session_cwd(context, call)?;
    let agent_id = agent.id.to_string();
    live_sessions().check_capacity(&agent_id)?;
    let (shell, flags) = resolve_shell_launcher()?;
    let mut args = flags.clone();
    args.push(command.clone());
    let queue = PermissionQueue::new();
    let host = Arc::new(DaemonAcpHost {
        queue: Arc::clone(&queue),
    });
    let launch = AcpLaunch {
        program: PathBuf::from(&shell),
        args,
        cwd: cwd.clone(),
        env: session_env(&agent_id, false),
        env_clear: true,
        resume_session_id: resume,
        // Modes in which a coding agent asks before acting, so its edits and
        // commands reach the owner's approvals (Claude Code: `default`).
        ask_modes: vec!["default".to_string()],
    };
    let binding = context.command_binding.as_deref().ok_or_else(|| {
        "Execution identity binding is missing; the coding agent did not start".to_string()
    })?;
    revalidate_shell_launch(Some(binding), &shell, &flags, &command, &cwd)?;
    let session = Arc::new(
        AcpSession::start_contained(launch, host)
            .await
            .map_err(|error| acp_error(&command, error))?,
    );
    let info = session.agent_info();
    let agent_name = info
        .title
        .clone()
        .or(info.name.clone())
        .unwrap_or_else(|| program_name(&command));
    let id = live_sessions().insert(
        "acp",
        new_live(
            agent,
            &command,
            cwd,
            LiveKind::Acp(AcpLive {
                session,
                read_from: 0,
                agent_name,
                permissions: queue,
            }),
        ),
    );
    acp_snapshot_of(&agent_id, &id)
}

/// `npx -y @zed-industries/codex-acp` reads as `codex-acp`.
fn program_name(command: &str) -> String {
    let words: Vec<&str> = command.split_whitespace().collect();
    let skip = ["npx", "bunx", "pnpm", "dlx", "-y", "--yes", "exec"];
    let word = words
        .iter()
        .find(|word| !skip.contains(word))
        .copied()
        .unwrap_or(command);
    Path::new(word.rsplit('/').next().unwrap_or(word))
        .to_string_lossy()
        .into_owned()
}

pub(in super::super) fn execute_acp_prompt(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move { reply(acp_prompt(&context, &agent, &call).await) })
}

async fn acp_prompt(
    context: &ToolExecutionContext,
    agent: &AgentState,
    call: &ToolCall,
) -> Result<Value, String> {
    let id = required_arg(call, "id")?;
    let text = required_arg(call, "text")?;
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!(
            "acp_prompt text must be at most {MAX_TEXT_CHARS} characters"
        ));
    }
    let wait = integer_arg(call, "wait_ms", (0, WAIT_MAX_MS))?.unwrap_or(ACP_WAIT_DEFAULT_MS);
    let agent_id = agent.id.to_string();
    let handle = acp_of(&agent_id, &id)?;
    let command = live_sessions().with(&agent_id, &id, |entry| entry.command.clone())?;
    handle
        .session
        .prompt(text)
        .await
        .map_err(|error| acp_error(&command, error))?;
    pump(context, agent, &id, &handle, Duration::from_millis(wait)).await;
    acp_snapshot_of(&agent_id, &id)
}

pub(in super::super) fn execute_acp_read(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        reply(
            async {
                let id = required_arg(&call, "id")?;
                let wait = integer_arg(&call, "wait_ms", (0, WAIT_MAX_MS))?.unwrap_or(0);
                let agent_id = agent.id.to_string();
                let handle = acp_of(&agent_id, &id)?;
                pump(&context, &agent, &id, &handle, Duration::from_millis(wait)).await;
                acp_snapshot_of(&agent_id, &id)
            }
            .await,
        )
    })
}

pub(in super::super) fn execute_acp_cancel(
    _context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        reply(
            async {
                let id = required_arg(&call, "id")?;
                let agent_id = agent.id.to_string();
                live_sessions().cancel_acp(&agent_id, &id).await?;
                let handle = acp_of(&agent_id, &id)?;
                // The agent ends the turn with stop reason "cancelled".
                handle.session.wait(Duration::from_secs(5)).await;
                acp_snapshot_of(&agent_id, &id)
            }
            .await,
        )
    })
}

pub(in super::super) fn execute_acp_stop(
    _context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        reply(
            async {
                let id = required_arg(&call, "id")?;
                let agent_id = agent.id.to_string();
                acp_of(&agent_id, &id)?;
                let gone = live_sessions().stop(&agent_id, &id).await?;
                let mut value = acp_snapshot_of(&agent_id, &id)?;
                value["treeGone"] = json!(gone);
                Ok(value)
            }
            .await,
        )
    })
}

pub(in super::super) fn execute_acp_list(
    _context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    _call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let sessions = live_sessions().list(&agent.id.to_string(), "acp-");
        reply(Ok(json!({"kind": "acp_sessions", "sessions": sessions})))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_names_skip_runners() {
        assert_eq!(
            program_name("npx -y @zed-industries/codex-acp"),
            "codex-acp"
        );
        assert_eq!(program_name("opencode acp"), "opencode");
        assert_eq!(
            program_name("/usr/local/bin/gemini --experimental-acp"),
            "gemini"
        );
    }
}

#[cfg(all(test, unix))]
#[path = "sessions_tests.rs"]
mod sessions_tests;
