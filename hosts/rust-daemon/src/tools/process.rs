pub(super) mod background;
pub(super) mod shell;

use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use futures::future::BoxFuture;
use std::path::Path;

use self::background::{
    list_background_processes, read_background_process_output, start_background_process,
    stop_background_process,
};
#[cfg(not(windows))]
use self::shell::execute_bash_command;
use super::{ctx_workspace_root, ToolExecutionContext};

#[cfg(all(test, windows))]
impl ToolExecutionContext {
    /// Shares one controlled fixture with shell and coordinator regressions
    /// without exporting the private process module or changing runtime APIs.
    pub(crate) fn m47_shell_fixture(mode: &str) -> shell::M47ShellFixture {
        shell::M47ShellFixture::new(mode)
    }
}

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
        let cancel = context.cancel.clone();
        #[cfg(windows)]
        let call = BashCallCancellation(anima_core::CancelSignal::new());
        #[cfg(windows)]
        let worker_cancel = call.0.clone();
        #[cfg(windows)]
        let unproven_journal = context.action_journal.clone();
        #[cfg(windows)]
        let unproven_run_cancel = context.cancel.clone();
        let result = tokio::task::spawn_blocking(move || {
            #[cfg(windows)]
            {
                shell::execute_bash_command_owned(
                    configured_root.as_deref(),
                    &command,
                    timeout_ms,
                    &cwd,
                    cancel,
                    worker_cancel,
                )
            }
            #[cfg(not(windows))]
            {
                execute_bash_command(
                    configured_root.as_deref(),
                    &command,
                    timeout_ms,
                    &cwd,
                    cancel.as_ref(),
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

        match start_background_process(
            &context.process_manager,
            ctx_workspace_root(&context),
            &command,
            &cwd,
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

        match stop_background_process(&context.process_manager, &id) {
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
