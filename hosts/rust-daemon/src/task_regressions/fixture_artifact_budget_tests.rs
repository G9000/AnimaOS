use super::*;
use anima_core::execution_limits::{
    ExecutionAllowance, ExecutionBudgetPort, ExecutionContext, ExecutionLimits,
    ReservationAdmission, ReservationRequest, ReservationSettlement,
};
use anima_core::{CancelSignal, TaskStatus};
use async_trait::async_trait;
use std::sync::Arc;

fn actor() -> ActorFixture {
    ActorFixture {
        prompt: "Edit the public fixture.".into(),
        initial_files: BTreeMap::from([("input.txt".into(), "old".into())]),
        readable_files: vec!["input.txt".into()],
        writable_files: vec!["input.txt".into(), "answer.txt".into()],
        mock_effects: true,
    }
}

fn call(name: &str, fields: &[(&str, &str)]) -> ToolCall {
    ToolCall {
        // Reusing a model-supplied id cannot provide accounting or replay authority.
        id: "repeated-model-id".into(),
        name: name.into(),
        args: fields
            .iter()
            .map(|(key, value)| ((*key).into(), DataValue::String((*value).into())))
            .collect(),
    }
}

fn context(max_artifact_bytes: u64) -> ExecutionContext {
    let mut limits = ExecutionLimits::default();
    limits.budget.max_artifact_bytes = Some(max_artifact_bytes);
    ExecutionContext::ephemeral("original-fixture-run".into(), limits, CancelSignal::new()).unwrap()
}

struct AfterAdmissionPort {
    inner: Arc<dyn ExecutionBudgetPort>,
    after_admission: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

#[async_trait]
impl ExecutionBudgetPort for AfterAdmissionPort {
    async fn reserve(
        &self,
        scope_id: &str,
        request: ReservationRequest,
    ) -> Result<ReservationAdmission, String> {
        let admission = self.inner.reserve(scope_id, request).await?;
        if let Some(after_admission) = self.after_admission.lock().unwrap().take() {
            after_admission();
        }
        Ok(admission)
    }

    fn submit_settlement(
        &self,
        scope_id: &str,
        settlement: ReservationSettlement,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        self.inner.submit_settlement(scope_id, settlement)
    }

    async fn allowance(&self, scope_id: &str) -> Result<ExecutionAllowance, String> {
        self.inner.allowance(scope_id).await
    }
}

fn after_admission(context: &mut ExecutionContext, action: impl FnOnce() + Send + 'static) {
    context.port = Arc::new(AfterAdmissionPort {
        inner: context.port.clone(),
        after_admission: Mutex::new(Some(Box::new(action))),
    });
}

#[tokio::test]
async fn task_regressions_fixture_artifact_budget_denies_before_creating_file() {
    let workspace = FixtureWorkspace::new(&actor(), false).unwrap();
    let context = context(1);
    let result = workspace
        .execute_controlled(
            call(
                "write_file",
                &[("path", "answer.txt"), ("content", &"x".repeat(100))],
            ),
            &context,
            &CancelSignal::new(),
        )
        .await;
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(result.error.as_deref(), Some("fixture_budget_denied"));
    assert!(
        !workspace.path().join("answer.txt").exists(),
        "denied output must not be created"
    );
}

#[tokio::test]
async fn task_regressions_fixture_artifact_budget_is_shared_across_workspaces_and_replacements() {
    let first = FixtureWorkspace::new(&actor(), false).unwrap();
    let second = FixtureWorkspace::new(&actor(), false).unwrap();
    let original = context(8);
    let clone = original.clone();
    let cancel = CancelSignal::new();
    assert_eq!(
        first
            .execute_controlled(
                call("write_file", &[("path", "answer.txt"), ("content", "abc")]),
                &original,
                &cancel,
            )
            .await
            .status,
        TaskStatus::Success
    );
    assert_eq!(
        second
            .execute_controlled(
                call("write_file", &[("path", "answer.txt"), ("content", "de")]),
                &clone,
                &cancel,
            )
            .await
            .status,
        TaskStatus::Success
    );
    assert_eq!(
        first
            .execute_controlled(
                call(
                    "replace_file",
                    &[("path", "answer.txt"), ("old", "abc"), ("new", "xyz")]
                ),
                &original,
                &cancel,
            )
            .await
            .status,
        TaskStatus::Success
    );
    let denied = second
        .execute_controlled(
            call("write_file", &[("path", "answer.txt"), ("content", "q")]),
            &clone,
            &cancel,
        )
        .await;
    assert_eq!(denied.status, TaskStatus::Error);
    assert_eq!(
        std::fs::read_to_string(first.path().join("answer.txt")).unwrap(),
        "xyz"
    );
    assert_eq!(
        std::fs::read_to_string(second.path().join("answer.txt")).unwrap(),
        "de",
        "denied replacement must preserve its preimage"
    );
}

#[tokio::test]
async fn task_regressions_fixture_input_and_setup_do_not_spend_output_or_replace_authority() {
    let workspace = FixtureWorkspace::new(&actor(), false).unwrap();
    let context = context(1);
    let original_binding = context.binding_hint();
    let original_port = context.port.clone();
    let cancel = CancelSignal::new();
    let read = workspace
        .execute_controlled(
            call("read_file", &[("path", "input.txt")]),
            &context,
            &cancel,
        )
        .await;
    assert_eq!(read.status, TaskStatus::Success);
    assert_eq!(read.data.unwrap().text, "old");
    assert_eq!(
        workspace
            .execute_controlled(
                call("write_file", &[("path", "answer.txt"), ("content", "x")]),
                &context,
                &cancel,
            )
            .await
            .status,
        TaskStatus::Success
    );
    assert_eq!(
        context.binding_hint(),
        original_binding,
        "fixture operations must retain the original scope, run and deadline"
    );
    assert!(
        std::sync::Arc::ptr_eq(&context.port, &original_port),
        "fixture operations must use the retained budget authority"
    );
}

#[tokio::test]
async fn task_regressions_fixture_artifact_denials_are_fixed_and_actor_capability_owned() {
    let workspace = FixtureWorkspace::new(&actor(), false).unwrap();
    let context = context(1);
    let cancel = CancelSignal::new();
    for path in [
        "private-oracle.txt",
        "../outside",
        "C:secret",
        "CON",
        "input.txt:stream",
    ] {
        let result = workspace
            .execute_controlled(
                call("write_file", &[("path", path), ("content", "x")]),
                &context,
                &cancel,
            )
            .await;
        assert_eq!(
            result.error.as_deref(),
            Some("fixture_tool_or_path_denied"),
            "denied path {path}"
        );
    }
    assert_eq!(
        workspace
            .execute_controlled(
                call("write_file", &[("path", "answer.txt"), ("content", "x")]),
                &context,
                &cancel,
            )
            .await
            .status,
        TaskStatus::Success,
        "denied paths must not consume the artifact allowance"
    );
}

#[tokio::test]
async fn task_regressions_fixture_cancel_and_deadline_stop_mutation() {
    let workspace = FixtureWorkspace::new(&actor(), false).unwrap();
    let mut context = context(100);
    let cancel = CancelSignal::new();
    cancel.cancel();
    let write = || call("write_file", &[("path", "answer.txt"), ("content", "x")]);
    assert_eq!(
        workspace
            .execute_controlled(write(), &context, &cancel)
            .await
            .error
            .as_deref(),
        Some("fixture_authority_closed")
    );
    context.deadline_at_ms = context.clock.now_ms();
    assert_eq!(
        workspace
            .execute_controlled(write(), &context, &CancelSignal::new())
            .await
            .error
            .as_deref(),
        Some("fixture_authority_closed")
    );
    assert!(!workspace.path().join("answer.txt").exists());
}

#[tokio::test]
async fn task_regressions_fixture_rechecks_logical_cancel_after_byte_admission() {
    let workspace = FixtureWorkspace::new(&actor(), false).unwrap();
    let mut context = context(3);
    let cancel = CancelSignal::new();
    let stop = cancel.clone();
    after_admission(&mut context, move || stop.cancel());
    let write = || call("write_file", &[("path", "answer.txt"), ("content", "new")]);
    let result = workspace
        .execute_controlled(write(), &context, &cancel)
        .await;
    assert_eq!(result.error.as_deref(), Some("fixture_authority_closed"));
    assert!(
        !workspace.path().join("answer.txt").exists(),
        "logical stop while admission awaited must prevent mutation"
    );
    assert_eq!(
        workspace
            .execute_controlled(write(), &context, &CancelSignal::new())
            .await
            .status,
        TaskStatus::Success,
        "a proven unsent mutation may retire without charging bytes or acquiring a fresh scope"
    );
}

#[tokio::test]
async fn task_regressions_fixture_applies_only_prepared_preimage_and_retains_uncertain_bytes() {
    let workspace = FixtureWorkspace::new(&actor(), false).unwrap();
    let mut context = context(3);
    let path = workspace.path().join("input.txt");
    after_admission(&mut context, move || {
        std::fs::write(path, "outside change").unwrap()
    });
    let result = workspace
        .execute_controlled(
            call(
                "replace_file",
                &[("path", "input.txt"), ("old", "old"), ("new", "new")],
            ),
            &context,
            &CancelSignal::new(),
        )
        .await;
    assert_eq!(result.error.as_deref(), Some("fixture_changed_reload"));
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("input.txt")).unwrap(),
        "outside change",
        "a stale prepared operation must never recapture and overwrite a changed file"
    );
    let other = FixtureWorkspace::new(&actor(), false).unwrap();
    assert_eq!(
        other
            .execute_controlled(
                call("write_file", &[("path", "answer.txt"), ("content", "x")]),
                &context,
                &CancelSignal::new(),
            )
            .await
            .status,
        TaskStatus::Error,
        "an uncertain filesystem result must retain its reserved upper bound"
    );
    assert!(!other.path().join("answer.txt").exists());
}

#[tokio::test]
async fn task_regressions_fixture_empty_outputs_still_obey_file_bounds() {
    let workspace = FixtureWorkspace::new(&actor(), false).unwrap();
    let context = context(1);
    let cancel = CancelSignal::new();
    assert_eq!(
        workspace
            .execute_controlled(
                call("write_file", &[("path", "answer.txt"), ("content", "")]),
                &context,
                &cancel,
            )
            .await
            .status,
        TaskStatus::Success
    );
    assert!(workspace.path().join("answer.txt").exists());
    let oversized = "x".repeat(FILE_BYTES + 1);
    let result = workspace
        .execute_controlled(
            call(
                "write_file",
                &[("path", "answer.txt"), ("content", &oversized)],
            ),
            &context,
            &cancel,
        )
        .await;
    assert_eq!(result.error.as_deref(), Some("fixture_content_limit"));
    assert_eq!(
        std::fs::read(workspace.path().join("answer.txt")).unwrap(),
        b""
    );
}

#[test]
fn task_regressions_fixture_mock_effects_preserve_first_execution_order() {
    let workspace = FixtureWorkspace::new(&actor(), true).unwrap();
    for key in ["z", "a", "z"] {
        assert_eq!(
            workspace
                .execute(call("record_effect", &[("key", key), ("payload", "same")]))
                .status,
            TaskStatus::Success
        );
    }
    assert_eq!(
        workspace
            .effects()
            .iter()
            .map(|effect| effect.key.as_str())
            .collect::<Vec<_>>(),
        ["z", "a"]
    );
    assert_eq!(
        workspace
            .execute(call(
                "record_effect",
                &[("key", "z"), ("payload", "different")]
            ))
            .error
            .as_deref(),
        Some("fixture_effect_key_conflict")
    );
    assert_eq!(
        workspace
            .effects()
            .iter()
            .map(|effect| effect.key.as_str())
            .collect::<Vec<_>>(),
        ["z", "a"]
    );
}
