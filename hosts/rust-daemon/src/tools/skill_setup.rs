//! Generic documented commands for an exact owner-approved skill package.
use super::*;
use crate::skills::command_runtime::CommandRuntimeRequest;
use crate::skills::python_runtime::RuntimeAction;

pub(crate) const NAME: &str = "manage_skill_setup";

pub(super) fn register(registry: &mut ToolRegistry) {
    let mut schema = object_parameters(vec![
        required_parameter("action", string_enum_parameter("Read setup state, prepare a private working copy, or run an exact documented command", &["status", "prepare", "run"])),
        required_parameter("slug", non_blank_string_parameter("Exact approved skill slug")),
        required_parameter("hash", non_blank_string_parameter("Approved package SHA-256 from load_skill")),
        optional_parameter("command", non_blank_string_parameter("run only: exact documented shell command, up to 16384 bytes. Executes with the daemon user's OS access after owner approval; never include credentials.")),
        optional_parameter("cwd", string_parameter("run only: relative directory inside the prepared working copy, default .")),
        optional_parameter("stateHash", non_blank_string_parameter("run only: latest stateHash returned by prepare/status; binds approval to this working state")),
        optional_parameter("timeoutMs", bounded_integer_parameter("Entire operation deadline including queue wait and validation, default 120000", 1_000, 300_000)),
    ]);
    schema.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(tool_descriptor(NAME,
        "Follow an approved skill's original setup instructions using installed command tools such as npm, Node, Bun, uv, Python or shell scripts. Use status, prepare, then run with the returned stateHash and an exact documented command. Prepare and run require fresh exact owner approval, even in Free. The private dependency/work directory has a clean environment; approved commands still have the daemon user's OS and network access. Inspect README, SKILL.md, manifests and relevant scripts before executing. The runtime verifies the approved package, shell and working-state identity, and records only the exact command outcome. Failed commands, changed state, Stop or revoked authority cannot prove readiness. Missing software produces an actual error; use documented installation through permitted tools, without inventing a language restriction. Use an explicit installed interpreter for scripts when executable permissions are absent. Commands can affect external systems; local process cleanup does not roll back remote effects. Never pass credentials in command text or claim a successful --help proves every workflow or account connection.", schema), super::skill_runtime::execute);
}

pub(super) fn request(call: &ToolCall) -> Result<CommandRuntimeRequest, String> {
    let mut value = crate::routes::data_value_to_json(&DataValue::Object(call.args.clone()));
    if let Some(timeout) = call.args.get("timeoutMs") {
        match timeout {
            DataValue::Number(n)
                if n.is_finite() && n.fract() == 0.0 && *n >= 1_000.0 && *n <= 300_000.0 =>
            {
                value["timeoutMs"] = serde_json::json!(*n as u64);
            }
            _ => return Err("Skill setup arguments are invalid".into()),
        }
    }
    let request: CommandRuntimeRequest =
        serde_json::from_value(value).map_err(|_| "Skill setup arguments are invalid")?;
    request
        .validate()
        .map_err(|_| "Skill setup arguments are invalid")?;
    Ok(request)
}

pub(super) fn preflight(call: &ToolCall) -> Result<(), String> {
    request(call).map(|_| ())
}

pub(crate) fn arguments_preview(call: &ToolCall) -> String {
    if preflight(call).is_err() {
        "Skill setup arguments omitted: invalid request".into()
    } else {
        crate::routes::data_value_to_json(&DataValue::Object(call.args.clone())).to_string()
    }
}

pub(crate) fn requires_exact_approval(call: &ToolCall) -> bool {
    call.name == NAME && request(call).is_ok_and(|request| request.action != RuntimeAction::Status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(action: &str) -> ToolCall {
        ToolCall {
            id: "setup-contract".into(),
            name: NAME.into(),
            args: [
                ("action".into(), DataValue::String(action.into())),
                ("slug".into(), DataValue::String("sample".into())),
                ("hash".into(), DataValue::String("a".repeat(64))),
            ]
            .into(),
        }
    }

    #[test]
    fn generic_setup_binds_exact_commands_and_refuses_environment_overrides() {
        assert!(!requires_exact_approval(&call("status")));
        assert!(requires_exact_approval(&call("prepare")));
        let mut run = call("run");
        run.args
            .insert("command".into(), DataValue::String("npm ci".into()));
        assert!(
            preflight(&run).is_err(),
            "a command without a captured working state is not executable"
        );
        run.args
            .insert("stateHash".into(), DataValue::String("b".repeat(64)));
        run.args
            .insert("timeoutMs".into(), DataValue::Number(30_000.0));
        assert!(requires_exact_approval(&run));
        for field in ["environment", "token", "shell", "executable"] {
            let mut forged = run.clone();
            forged
                .args
                .insert(field.into(), DataValue::String("synthetic-secret".into()));
            assert!(preflight(&forged).is_err());
            assert!(!arguments_preview(&forged).contains("synthetic-secret"));
        }
    }
}
