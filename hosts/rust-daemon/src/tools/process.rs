pub(super) mod background;
pub(crate) mod live;
mod sessions;
pub(super) mod shell;

pub(super) use self::sessions::{
    execute_acp_cancel, execute_acp_list, execute_acp_prompt, execute_acp_read, execute_acp_start,
    execute_acp_stop, execute_app_list, execute_app_read, execute_app_send, execute_app_start,
    execute_app_stop,
};

use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use futures::future::BoxFuture;
use std::path::Path;
use std::sync::Arc;

use self::background::{
    list_background_processes, read_background_process_output, start_background_process,
    stop_background_process, wait_background_process, ExitNotifier, WaitUntil,
};
#[cfg(not(windows))]
use self::shell::execute_bash_command_bound;
use super::{ctx_workspace_root, ToolExecutionContext};

const BG_WAIT_DEFAULT_MS: u64 = 30_000;
const BG_WAIT_MAX_MS: u64 = 120_000;
const BG_WAIT_MAX_UNTIL_CHARS: usize = 256;
const BG_WAIT_REGEX_SIZE_LIMIT: usize = 1 << 16;

/// The runtime that owns every child the shell tools launch, independent of
/// the caller's runtime so synchronous callers and drops can still retire
/// process trees. It lives for the whole daemon process.
fn process_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("anima-process")
            .enable_all()
            .build()
            .expect("process runtime starts")
    })
}

/// Runs `work` on the process runtime and blocks the calling thread for its
/// result; safe from any thread, including async ones. `None` if it panicked.
#[cfg_attr(windows, allow(dead_code))]
fn run_on_process_runtime<F>(work: F) -> Option<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    process_runtime().spawn(async move {
        let _ = sender.send(work.await);
    });
    receiver.recv().ok()
}

#[cfg(all(test, windows))]
impl ToolExecutionContext {
    /// Shares one controlled fixture with shell and coordinator regressions
    /// without exporting the private process module or changing runtime APIs.
    pub(crate) fn m47_shell_fixture(mode: &str) -> shell::M47ShellFixture {
        shell::M47ShellFixture::new(mode)
    }
}

#[cfg(test)]
pub(crate) use self::background::start_background_process_from_root;
pub(crate) use self::background::{
    background_process_count, new_shared_process_manager_with_limit, SharedProcessManager,
    DEFAULT_MAX_BACKGROUND_PROCESSES,
};

#[cfg(windows)]
struct BashCallCancellation(anima_core::CancelSignal);
#[cfg(windows)]
impl Drop for BashCallCancellation {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(super) fn execute_bash(
    context: ToolExecutionContext,
    _agent: AgentState,
    _user_message: Message,
    tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let command = match tool_call.args.get("command") {
            Some(DataValue::String(value)) if !value.trim().is_empty() => value.trim().to_string(),
            _ => return TaskResult::error("bash command must be a non-empty string", 0),
        };

        let timeout_ms = match tool_call.args.get("timeout") {
            Some(DataValue::Number(value))
                if value.is_finite() && *value >= 1.0 && value.fract() == 0.0 =>
            {
                *value as u64
            }
            Some(DataValue::Number(_)) | Some(_) => {
                return TaskResult::error("bash timeout must be a positive integer", 0);
            }
            None => 120_000,
        };

        let cwd = match tool_call.args.get("cwd") {
            Some(DataValue::String(value)) if !value.trim().is_empty() => value.trim().to_string(),
            Some(DataValue::String(_)) => ".".to_string(),
            Some(_) => return TaskResult::error("bash cwd must be a string", 0),
            None => ".".to_string(),
        };

        // execute_bash_command spawns a child + polls in a busy-loop on a
        // worker thread; running it directly on a tokio worker would block the
        // entire runtime. spawn_blocking moves it onto the blocking pool.
        #[cfg(windows)]
        if let Some(error) = shell::cleanup_disabled() {
            return TaskResult::error(error, 0);
        }
        let configured_root = ctx_workspace_root(&context).map(Path::to_path_buf);
        let Some(command_binding) = context.command_binding.clone() else {
            return TaskResult::error(
                "Execution identity binding is missing; the command did not run",
                0,
            );
        };
        let cancel = context.cancel.clone();
        let profile_owner = context.profile_owner.clone();
        #[cfg(windows)]
        let call = BashCallCancellation(anima_core::CancelSignal::new());
        #[cfg(windows)]
        let worker_cancel = call.0.clone();
        #[cfg(windows)]
        let unproven_journal = context.action_journal.clone();
        #[cfg(windows)]
        let unproven_run_cancel = context.cancel.clone();
        let result = tokio::task::spawn_blocking(move || {
            if let Some(owner) = &profile_owner {
                owner.ensure_owned().map_err(|error| error.to_string())?;
            }
            #[cfg(windows)]
            {
                shell::execute_bash_command_owned(
                    configured_root.as_deref(),
                    &command,
                    timeout_ms,
                    &cwd,
                    cancel,
                    worker_cancel,
                    profile_owner,
                    command_binding,
                )
            }
            #[cfg(not(windows))]
            {
                let _profile_owner = profile_owner;
                execute_bash_command_bound(
                    configured_root.as_deref(),
                    &command,
                    timeout_ms,
                    &cwd,
                    cancel.as_ref(),
                    command_binding,
                )
            }
        })
        .await;
        #[cfg(windows)]
        if result.as_ref().is_ok_and(|result| {
            result
                .as_ref()
                .err()
                .is_some_and(|error| error == shell::BASH_CLEANUP_UNKNOWN)
        }) {
            if let Some(journal) = unproven_journal {
                journal.stop_for_unproven_effects(shell::BASH_CLEANUP_UNKNOWN);
            } else if let Some(cancel) = unproven_run_cancel {
                cancel.cancel();
            }
        }
        match result {
            Ok(Ok(result)) if result.status == "success" => TaskResult::success(
                Content {
                    text: result.output,
                    attachments: None,
                    metadata: None,
                },
                0,
            ),
            Ok(Ok(result)) => TaskResult::error(result.output, 0),
            Ok(Err(error)) => TaskResult::error(error, 0),
            Err(error) => TaskResult::error(format!("bash worker panicked: {error}"), 0),
        }
    })
}

pub(super) fn execute_bg_start(
    context: ToolExecutionContext,
    _agent: AgentState,
    _user_message: Message,
    tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let command = match tool_call.args.get("command") {
            Some(DataValue::String(value)) if !value.trim().is_empty() => value.trim().to_string(),
            _ => return TaskResult::error("bg_start command must be a non-empty string", 0),
        };

        let cwd = match tool_call.args.get("cwd") {
            Some(DataValue::String(value)) if !value.trim().is_empty() => value.trim().to_string(),
            Some(DataValue::String(_)) => ".".to_string(),
            Some(_) => return TaskResult::error("bg_start cwd must be a string", 0),
            None => ".".to_string(),
        };

        let notify_on_exit = match tool_call.args.get("notify_on_exit") {
            Some(DataValue::Bool(value)) => *value,
            Some(_) => return TaskResult::error("bg_start notify_on_exit must be a boolean", 0),
            None => true,
        };
        let notify_on = match tool_call.args.get("notify_on") {
            None | Some(DataValue::Null) => Vec::new(),
            Some(DataValue::Array(items)) if items.len() <= background::MAX_NOTIFY_PATTERNS => {
                let mut patterns = Vec::new();
                for item in items {
                    let DataValue::String(text) = item else {
                        return TaskResult::error(
                            "bg_start notify_on must be a list of strings",
                            0,
                        );
                    };
                    match parse_wait_until(text) {
                        Ok(until) => patterns.push((text.clone(), until)),
                        Err(error) => {
                            return TaskResult::error(
                                error.replace("bg_wait until", "bg_start notify_on"),
                                0,
                            )
                        }
                    }
                }
                patterns
            }
            Some(_) => {
                return TaskResult::error(
                    format!(
                        "bg_start notify_on must be a list of at most {} strings",
                        background::MAX_NOTIFY_PATTERNS
                    ),
                    0,
                )
            }
        };
        let notifier = (notify_on_exit || !notify_on.is_empty())
            .then(|| exit_notifier(&context))
            .flatten();
        let watch = notifier
            .clone()
            .filter(|_| !notify_on.is_empty())
            .map(|notifier| (notifier, notify_on));
        let notifier = notifier.filter(|_| notify_on_exit);

        let Some(command_binding) = context.command_binding.clone() else {
            return TaskResult::error(
                "Execution identity binding is missing; the command did not run",
                0,
            );
        };

        match start_background_process(
            &context.process_manager,
            ctx_workspace_root(&context),
            &command,
            &cwd,
            notifier,
            watch,
            context.profile_owner.clone(),
            command_binding,
        ) {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    attachments: None,
                    metadata: None,
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

pub(super) fn execute_bg_output(
    context: ToolExecutionContext,
    _agent: AgentState,
    _user_message: Message,
    tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let id = match tool_call.args.get("id") {
            Some(DataValue::String(value)) if !value.trim().is_empty() => value.trim().to_string(),
            _ => return TaskResult::error("bg_output id must be a non-empty string", 0),
        };

        let all = match tool_call.args.get("all") {
            Some(DataValue::Bool(value)) => *value,
            Some(_) => return TaskResult::error("bg_output all must be a boolean", 0),
            None => false,
        };

        match read_background_process_output(&context.process_manager, &id, all) {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    attachments: None,
                    metadata: None,
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

pub(super) fn execute_bg_stop(
    context: ToolExecutionContext,
    _agent: AgentState,
    _user_message: Message,
    tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let id = match tool_call.args.get("id") {
            Some(DataValue::String(value)) if !value.trim().is_empty() => value.trim().to_string(),
            _ => return TaskResult::error("bg_stop id must be a non-empty string", 0),
        };

        // Retiring a tree blocks until it is proven gone.
        let manager = context.process_manager.clone();
        let profile_owner = context.profile_owner.clone();
        let stopped = tokio::task::spawn_blocking(move || {
            let _profile_owner = profile_owner;
            stop_background_process(&manager, &id)
        })
        .await
        .unwrap_or_else(|error| Err(format!("bg_stop worker panicked: {error}")));
        match stopped {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    attachments: None,
                    metadata: None,
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

pub(super) fn execute_bg_list(
    context: ToolExecutionContext,
    _agent: AgentState,
    _user_message: Message,
    _tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        match list_background_processes(&context.process_manager) {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    attachments: None,
                    metadata: None,
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

/// Wakes the agent in the conversation that started the job when it ends on
/// its own (the calendar follow-up path). `None` outside an agent run.
fn exit_notifier(context: &ToolExecutionContext) -> Option<ExitNotifier> {
    let coordinator = context.team.clone()?;
    let link = context.run_link.clone()?;
    let runtime = tokio::runtime::Handle::try_current().ok()?;
    Some(Arc::new(move |note: String| {
        // Each note is its own follow-up run.
        let reference = format!("bg-note:{}:{}", link.run_id, uuid::Uuid::new_v4());
        let request = crate::agent_runs::AgentRunRequest {
            agent_id: link.agent_id.clone(),
            content: Content {
                text: super::output_safety::sanitize_text(&note),
                attachments: None,
                metadata: None,
            },
            room: crate::agent_runs::RunRoom::Stable(link.session_id.clone()),
            idempotency_key: Some(reference.clone()),
            // Daemon-internal follow-ups are recorded like API runs.
            source: crate::runs::RunSource::Api,
            source_ref: Some(reference),
            parent: None,
        };
        let followup = crate::agent_runs::SystemFollowup {
            title: "Background job update".to_string(),
        };
        let coordinator = coordinator.clone();
        runtime.spawn(async move {
            let _ = coordinator.run_system_followup(request, followup).await;
        });
    }))
}

fn parse_wait_until(value: &str) -> Result<WaitUntil, String> {
    if value.is_empty() {
        return Err("bg_wait until must not be empty".to_string());
    }
    if value.chars().count() > BG_WAIT_MAX_UNTIL_CHARS {
        return Err(format!(
            "bg_wait until must be at most {BG_WAIT_MAX_UNTIL_CHARS} characters"
        ));
    }
    match value
        .strip_prefix('/')
        .and_then(|rest| rest.strip_suffix('/'))
    {
        Some(pattern) if !pattern.is_empty() => regex::RegexBuilder::new(pattern)
            .size_limit(BG_WAIT_REGEX_SIZE_LIMIT)
            .build()
            .map(WaitUntil::Pattern)
            .map_err(|error| format!("bg_wait until is not a valid regex: {error}")),
        _ => Ok(WaitUntil::Literal(value.to_string())),
    }
}

pub(super) fn execute_bg_wait(
    context: ToolExecutionContext,
    _agent: AgentState,
    _user_message: Message,
    tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let id = match tool_call.args.get("id") {
            Some(DataValue::String(value)) if !value.trim().is_empty() => value.trim().to_string(),
            _ => return TaskResult::error("bg_wait id must be a non-empty string", 0),
        };
        let timeout_ms = match tool_call.args.get("timeout_ms") {
            Some(DataValue::Number(value))
                if value.is_finite() && *value >= 1.0 && value.fract() == 0.0 =>
            {
                (*value as u64).min(BG_WAIT_MAX_MS)
            }
            Some(_) => {
                return TaskResult::error("bg_wait timeout_ms must be a positive integer", 0)
            }
            None => BG_WAIT_DEFAULT_MS,
        };
        let until = match tool_call.args.get("until") {
            Some(DataValue::String(value)) => match parse_wait_until(value) {
                Ok(until) => Some(until),
                Err(error) => return TaskResult::error(error, 0),
            },
            Some(_) => return TaskResult::error("bg_wait until must be a string", 0),
            None => None,
        };

        match wait_background_process(
            &context.process_manager,
            &id,
            std::time::Duration::from_millis(timeout_ms),
            until,
        )
        .await
        {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    attachments: None,
                    metadata: None,
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bg_wait_until_is_a_literal_unless_slash_delimited() {
        assert!(
            matches!(parse_wait_until("ready"), Ok(WaitUntil::Literal(text)) if text == "ready")
        );
        assert!(matches!(parse_wait_until("/"), Ok(WaitUntil::Literal(_))));
        let Ok(WaitUntil::Pattern(pattern)) = parse_wait_until(r"/listening on \d+/") else {
            panic!("expected a regex");
        };
        assert!(pattern.is_match("listening on 5173"));
    }

    #[test]
    fn bg_wait_until_rejects_long_invalid_and_oversized_patterns() {
        assert!(parse_wait_until("").is_err());
        assert!(parse_wait_until(&"a".repeat(257))
            .unwrap_err()
            .contains("at most 256"));
        assert!(parse_wait_until("/(unclosed/")
            .unwrap_err()
            .contains("not a valid regex"));
        let oversized = format!("/{}/", r"\w{100}".repeat(20));
        assert!(oversized.len() <= 256 + 2);
        assert!(parse_wait_until(&oversized).is_err());
    }
}
