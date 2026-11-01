//! Owner-approved package execution, separate from the general host shell.
use super::*;
use crate::skills::command_runtime::CommandRuntimeRequest;
use crate::skills::python_runtime::{PythonRuntimeRequest, RuntimeAction, RuntimeOutput};

pub(crate) const NAME: &str = "manage_skill_runtime";

pub(super) fn register(registry: &mut ToolRegistry) {
    let mut schema = object_parameters(vec![
        required_parameter("action", string_enum_parameter("Inspect setup, prepare a dedicated Python environment, or run an exact reviewed Python entrypoint", &["status", "prepare", "run"])),
        required_parameter("slug", non_blank_string_parameter("Exact owner-approved skill slug")),
        required_parameter("hash", non_blank_string_parameter("Exact approved package SHA-256 returned by load_skill or Skills")),
        optional_parameter("entrypoint", non_blank_string_parameter("Reviewed relative .py file for run, for example scripts/last30days.py")),
        optional_parameter("args", array_parameter("Arguments for the exact check or task; never credentials. No shell expansion. Successful --help proves only that exact command starts.", string_parameter("Literal argument"), None)),
        optional_parameter("requirements", string_parameter("Reviewed relative requirements.txt path; omitted detects requirements.txt; empty explicitly selects no third-party dependencies")),
        optional_parameter("timeoutMs", bounded_integer_parameter("Deadline for the complete operation, default 120000, maximum 300000", 1_000, 300_000)),
    ]);
    schema.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(tool_descriptor(NAME, "Manage an owner-approved skill's real Python package runtime. First load_skill and inspect the original README, scripts and requirements. status reports setup without executing code. prepare creates a dedicated environment and installs supported reviewed PyPI wheel requirements; run executes the exact approved relative Python script with literal arguments. Both prepare and run require a fresh exact owner approval, even in Free. This is local execution with the daemon user's operating-system access; the environment isolates dependencies, not filesystem or network access. General host shell execution remains separately controlled. Ambient account secrets are not inherited. Package changes, disabled skills, Stop and revoked policy refuse execution. Report installed, environment prepared and the exact successful check separately; no successful check proves account sign-in or every skill workflow. Do not replace a full repository package with a Markdown helper, claim readiness after a failed check, or use this for unrelated commands. Other runtime types need their supported execution tools.", schema), execute);
}

pub(super) fn request(call: &ToolCall) -> Result<PythonRuntimeRequest, String> {
    let mut value = crate::routes::data_value_to_json(&DataValue::Object(call.args.clone()));
    if let Some(timeout) = call.args.get("timeoutMs") {
        match timeout {
            DataValue::Number(n)
                if n.is_finite() && n.fract() == 0.0 && *n >= 1.0 && *n <= 300_000.0 =>
            {
                value["timeoutMs"] = serde_json::json!(*n as u64)
            }
            _ => return Err("Skill runtime arguments are invalid".into()),
        }
    }
    let request: PythonRuntimeRequest =
        serde_json::from_value(value).map_err(|_| "Skill runtime arguments are invalid")?;
    request
        .validate()
        .map_err(|_| "Skill runtime arguments are invalid")?;
    Ok(request)
}

pub(super) fn preflight(call: &ToolCall) -> Result<(), String> {
    request(call).map(|_| ())
}
pub(crate) fn arguments_preview(call: &ToolCall) -> String {
    if preflight(call).is_err() {
        "Skill runtime arguments omitted: invalid request".into()
    } else {
        crate::routes::data_value_to_json(&DataValue::Object(call.args.clone())).to_string()
    }
}
pub(crate) fn requires_exact_approval(call: &ToolCall) -> bool {
    (call.name == NAME && request(call).is_ok_and(|r| r.action != RuntimeAction::Status))
        || super::skill_setup::requires_exact_approval(call)
}

enum PackageRuntimeRequest {
    Python(PythonRuntimeRequest),
    Commands(CommandRuntimeRequest),
}

impl PackageRuntimeRequest {
    fn action(&self) -> RuntimeAction {
        match self {
            Self::Python(request) => request.action,
            Self::Commands(request) => request.action,
        }
    }
    fn slug(&self) -> &str {
        match self {
            Self::Python(request) => &request.slug,
            Self::Commands(request) => &request.slug,
        }
    }
    fn hash(&self) -> &str {
        match self {
            Self::Python(request) => &request.hash,
            Self::Commands(request) => &request.hash,
        }
    }
}

pub(super) fn execute(
    context: ToolExecutionContext,
    agent: AgentState,
    user: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if !context.standing_owner_turn
            || context.has_upstream_tool_origin()
            || crate::agent_runs::is_helper_config(&agent.config)
            || user.agent_id != agent.id
            || user.role != anima_core::MessageRole::User
        {
            return TaskResult::error("Skill setup requires a direct owner web/API chat", 0);
        }
        let request = match match call.name.as_str() {
            NAME => request(&call).map(PackageRuntimeRequest::Python),
            super::skill_setup::NAME => {
                super::skill_setup::request(&call).map(PackageRuntimeRequest::Commands)
            }
            _ => Err("Unknown skill runtime tool".into()),
        } {
            Ok(r) => r,
            Err(e) => return TaskResult::error(e, 0),
        };
        if request.action() != RuntimeAction::Status && !context.action_approved {
            return TaskResult::error(
                "Skill preparation and execution require exact owner approval",
                0,
            );
        }
        let (Some(coordinator), Some(run), Some(journal), Some(cancel)) = (
            &context.team,
            &context.run_link,
            &context.action_journal,
            &context.cancel,
        ) else {
            return TaskResult::error(
                "Skill setup requires a live owner run and native action receipts",
                0,
            );
        };
        if journal.bound_run() != run || run.agent_id != agent.id {
            return TaskResult::error("Skill runtime owner changed", 0);
        }
        let (workspace, runtime_root) = match coordinator.skill_runtime_paths().await {
            Ok(paths) => paths,
            Err(e) => return TaskResult::error(e, 0),
        };
        let authority: crate::skills::python_runtime::RuntimeAuthority = {
            let journal = journal.clone();
            let call = call.clone();
            let workspace = workspace.clone();
            let slug = request.slug().to_owned();
            let hash = request.hash().to_owned();
            let approved = context.action_approved;
            Arc::new(move || {
                let journal = journal.clone();
                let call = call.clone();
                let workspace = workspace.clone();
                let slug = slug.clone();
                let hash = hash.clone();
                Box::pin(async move {
                    journal
                        .skill_runtime_authority(&call, approved, &workspace, &slug, &hash)
                        .await
                })
            })
        };
        if let Err(error) = authority().await {
            return TaskResult::error(error, 0);
        }
        let root = workspace.clone();
        let slug = request.slug().to_owned();
        let package = match crate::skills::service::blocking(move || {
            crate::skills::package::read_package(&root, &slug)
        })
        .await
        {
            Ok(Ok(Some(p))) if p.hash() == request.hash() => p,
            _ => return TaskResult::error("Approved skill package changed or is missing", 0),
        };
        let custody_root = runtime_root.clone();
        let action = request.action();
        let result = match request {
            PackageRuntimeRequest::Python(request) => crate::skills::python_runtime::execute(
                runtime_root,
                workspace,
                package,
                request,
                cancel.clone(),
                authority,
            )
            .await
            .map(|result| (serde_json::to_string(&result), result.status, result.output)),
            PackageRuntimeRequest::Commands(request) => crate::skills::command_runtime::execute(
                runtime_root,
                workspace,
                package,
                request,
                cancel.clone(),
                authority,
            )
            .await
            .map(|result| (serde_json::to_string(&result), result.status, result.output)),
        };
        match result {
            Ok((text, status, output)) => {
                if output
                    .as_ref()
                    .is_some_and(|output| !output.cleanup_confirmed)
                {
                    journal.stop_for_unproven_effects("Skill runtime physical cleanup is unconfirmed; further effects were stopped");
                }
                let text = match text {
                    Ok(text) => text,
                    Err(_) => {
                        return TaskResult::error("Skill runtime result could not be encoded", 0)
                    }
                };
                result_task(action, &status, text, output.as_ref())
            }
            Err(error) => {
                if error.contains("unresolved physical custody")
                    || crate::skills::python_runtime::persisted_marker_count(&custody_root) != 0
                {
                    journal.stop_for_unproven_effects("Skill runtime physical cleanup is unresolved; further effects were stopped");
                }
                TaskResult::error(error, 0)
            }
        }
    })
}

fn result_task(
    action: RuntimeAction,
    status: &str,
    text: String,
    output: Option<&RuntimeOutput>,
) -> TaskResult<Content> {
    let accepted = match action {
        RuntimeAction::Status => matches!(
            status,
            "installed" | "needs_prepare" | "environment_prepared" | "command_verified"
        ),
        RuntimeAction::Prepare => status == "environment_prepared",
        RuntimeAction::Run => status == "command_verified" && output.is_some(),
    };
    if !accepted
        || output.is_some_and(|output| {
            output.exit_code != Some(0) || output.stop.is_some() || !output.cleanup_confirmed
        })
    {
        TaskResult::error(text, 0)
    } else {
        TaskResult::success(
            Content {
                text,
                ..Content::default()
            },
            0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_skill_command_state_is_an_error_even_when_the_process_exited_zero() {
        let output = RuntimeOutput {
            exit_code: Some(0),
            stdout: "check exited zero".into(),
            stderr: String::new(),
            stop: None,
            cleanup_confirmed: true,
        };
        let rejected = result_task(
            RuntimeAction::Run,
            "command_failed",
            "source seal rejected".into(),
            Some(&output),
        );
        assert_eq!(
            rejected.status.as_str(),
            "error",
            "zero process exit does not accept a rejected source or environment seal"
        );
        assert_eq!(
            result_task(
                RuntimeAction::Run,
                "environment_prepared",
                String::new(),
                Some(&output)
            )
            .status
            .as_str(),
            "error"
        );
        assert_eq!(
            result_task(
                RuntimeAction::Run,
                "command_verified",
                String::new(),
                Some(&output)
            )
            .status
            .as_str(),
            "success"
        );
        assert_eq!(
            result_task(RuntimeAction::Status, "needs_prepare", String::new(), None)
                .status
                .as_str(),
            "success"
        );
    }
    #[test]
    fn runtime_contract_never_accepts_shell_or_ambient_secrets() {
        let descriptor = ToolRegistry::new().descriptor(NAME).unwrap();
        assert!(descriptor
            .description
            .contains("fresh exact owner approval"));
        for field in ["command", "environment", "token", "interpreter"] {
            let call = ToolCall {
                id: "test".into(),
                name: NAME.into(),
                args: [
                    ("action".into(), DataValue::String("status".into())),
                    ("slug".into(), DataValue::String("sample".into())),
                    ("hash".into(), DataValue::String("a".repeat(64))),
                    (field.into(), DataValue::String("secret".into())),
                ]
                .into(),
            };
            assert!(preflight(&call).is_err());
        }
    }
}
