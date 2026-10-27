//! Translate settled native outcomes into host-neutral completion observations.
mod command;
mod contracts;
mod evidence;

pub(crate) use evidence::HostCompletionReview;

#[cfg(test)]
mod change_bound_tests;

use anima_core::{
    completion::CompletionCheckpoint, AgentConfig, CompletionObservation, CompletionReview,
    Content, DataValue, TaskResult, TaskStatus, ToolCall,
};

pub(crate) fn enabled(config: &AgentConfig) -> bool {
    config
        .settings
        .as_ref()
        .and_then(|settings| settings.additional.get("completionReview"))
        == Some(&DataValue::Bool(true))
}

pub(super) fn observe(
    review: &CompletionReview,
    checkpoint: CompletionCheckpoint,
    call: &ToolCall,
    result: &TaskResult<Content>,
) {
    let success = result.status == TaskStatus::Success;
    let observation = match call.name.as_str() {
        "write_file" | "write_document" | "edit_file" | "multi_edit" if success => {
            path(call).map(|path| {
                let requires_check = requires_check(&path);
                CompletionObservation::Edited {
                    path,
                    requires_check,
                }
            })
        }
        "read_file" | "read_document_source"
            if success && call.args.get("limit") != Some(&DataValue::Number(0.0)) =>
        {
            path(call).map(|path| CompletionObservation::ReadBack { path })
        }
        "bash" => Some(
            if matches!(call.args.get("command"), Some(DataValue::String(text)) if command::recognizes(text))
            {
                CompletionObservation::VerificationFinished { passed: success }
            } else {
                CompletionObservation::CommandFinished { passed: success }
            },
        ),
        "todo_write" if success => {
            let Some(DataValue::Array(todos)) = call.args.get("todos") else {
                return;
            };
            Some(CompletionObservation::TasksSaved {
                unfinished: todos
                    .iter()
                    .filter(|todo| {
                        let DataValue::Object(fields) = todo else {
                            return true;
                        };
                        !matches!(fields.get("status"), Some(DataValue::String(status)) if matches!(status.as_str(), "completed"|"cancelled"))
                    })
                    .count(),
            })
        }
        "todo_list" if success && call.args.contains_key("todos") => result
            .data
            .as_ref()
            .and_then(|content| content.metadata.as_ref())
            .and_then(|metadata| metadata.get("structuredTodoUnfinished"))
            .and_then(|value| match value {
                DataValue::Number(n)
                    if n.is_finite() && *n >= 0.0 && *n <= 256.0 && n.fract() == 0.0 =>
                {
                    Some(*n as usize)
                }
                _ => None,
            })
            .map(|unfinished| CompletionObservation::TasksSaved { unfinished }),
        _ => None,
    };
    if let Some(observation) = observation {
        review.observe_after(checkpoint, observation);
    }
}

fn path(call: &ToolCall) -> Option<String> {
    let DataValue::String(path) = call.args.get("file_path")? else {
        return None;
    };
    let path = path
        .replace('\\', "/")
        .split('/')
        .filter(|part| *part != "." && !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    #[cfg(windows)]
    let path = path.to_ascii_lowercase();
    Some(path)
}

fn requires_check(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let extension = file.rsplit_once('.').map(|(_, extension)| extension);
    !matches!(
        extension,
        Some(
            "md" | "markdown"
                | "mdx"
                | "rst"
                | "txt"
                | "text"
                | "adoc"
                | "asciidoc"
                | "org"
                | "log"
                | "csv"
                | "tsv"
        )
    ) && !matches!(
        file.as_str(),
        "license" | "licence" | "notice" | "authors" | "contributors" | "changelog" | "codeowners"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anima_core::{AgentRuntime, Evaluator, Message, ModelAdapter};
    use std::sync::Arc;

    #[tokio::test]
    async fn fp03_structured_completion_cancelled_is_terminal() {
        let review = CompletionReview::default();
        let mut tasks = call("todo_write");
        tasks.args.insert(
            "todos".into(),
            DataValue::Array(vec![DataValue::Object(
                [("status".into(), DataValue::String("cancelled".into()))].into(),
            )]),
        );
        settled(&review, &tasks, &ok());
        assert!(!needs_review(&review).await);
    }

    #[tokio::test]
    async fn fp03_structured_completion_uses_acknowledged_whole_saved_list() {
        let review = CompletionReview::default();
        let mut tasks = call("todo_list");
        tasks.args.insert("todos".into(), DataValue::Array(vec![]));
        tasks.args.insert("merge".into(), DataValue::Bool(true));
        let mut result = ok();
        result.data.as_mut().unwrap().metadata =
            Some([("structuredTodoUnfinished".into(), DataValue::Number(2.0))].into());
        settled(&review, &tasks, &result);
        assert!(
            needs_review(&review).await,
            "empty submitted merge must not erase pending saved tasks"
        );
        result
            .data
            .as_mut()
            .unwrap()
            .metadata
            .as_mut()
            .unwrap()
            .insert("structuredTodoUnfinished".into(), DataValue::Number(0.0));
        settled(&review, &tasks, &result);
        assert!(!needs_review(&review).await);
    }

    async fn needs_review(review: &CompletionReview) -> bool {
        let config = crate::agent_runs::test_support::companion_config("completion");
        let model: Arc<dyn ModelAdapter> =
            crate::agent_runs::test_support::ScriptedModel::new(vec![]);
        let runtime = AgentRuntime::new(config, model);
        let message = Message {
            id: "m".into(),
            agent_id: "a".into(),
            room_id: "r".into(),
            role: anima_core::MessageRole::User,
            content: Content::default(),
            created_at_ms: 1,
        };
        matches!(
            review
                .evaluate(&runtime, &message, &Content::default())
                .await
                .unwrap()
                .decision,
            anima_core::EvaluatorDecision::Retry { .. }
        )
    }
    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: "call".into(),
            name: name.into(),
            args: [("file_path".into(), DataValue::String("src/lib.rs".into()))].into(),
        }
    }
    fn ok() -> TaskResult<Content> {
        TaskResult::success(
            Content {
                text: "settled".into(),
                ..Default::default()
            },
            0,
        )
    }
    fn bash(command: &str) -> ToolCall {
        let mut call = call("bash");
        call.args
            .insert("command".into(), DataValue::String(command.into()));
        call
    }
    fn settled(review: &CompletionReview, call: &ToolCall, result: &TaskResult<Content>) {
        observe(review, review.checkpoint(), call, result);
    }

    #[tokio::test]
    async fn completion_review_settled_native_write_requires_review() {
        let review = CompletionReview::default();
        settled(&review, &call("write_file"), &ok());
        assert!(needs_review(&review).await);
    }
    #[tokio::test]
    async fn completion_review_failed_or_unknown_write_is_not_success_evidence() {
        let review = CompletionReview::default();
        settled(
            &review,
            &call("write_file"),
            &TaskResult::error("outcome save failed", 0),
        );
        assert!(!needs_review(&review).await);
    }
    #[tokio::test]
    async fn completion_review_post_edit_read_and_unscoped_command_leave_verification_pending() {
        let review = CompletionReview::default();
        settled(&review, &call("edit_file"), &ok());
        settled(&review, &call("read_file"), &ok());
        settled(&review, &bash("cargo test -p example"), &ok());
        assert!(needs_review(&review).await);
    }

    #[tokio::test]
    async fn quality_change_bound_completion_foreign_green_check_leaves_changed_code_pending() {
        for command in [
            "cargo test -p unrelated",
            "bun x nx run @animaos/docs:build",
        ] {
            let review = CompletionReview::default();
            settled(&review, &call("write_file"), &ok());
            settled(&review, &call("read_file"), &ok());
            settled(&review, &bash(command), &ok());
            assert!(needs_review(&review).await, "unresolved scope: {command}");
        }
    }

    #[tokio::test]
    async fn completion_review_informational_and_masked_commands_are_not_checks() {
        for command in [
            "echo done",
            "git status",
            "cargo test --help",
            "cargo test --no-run",
            "node --test --help",
            "cargo test || echo passed",
            "cargo test; echo passed",
            "cargo test | head",
            "cargo test && echo passed",
            "cargo test > output.txt",
            "cargo fmt",
            "cargo test -- --list",
            "cargo test -qh",
            "cargo test --unknown-option",
            "cargo test --package",
            "cargo test --package=",
            "cargo test --package --help",
            "cargo clippy --fix",
            "cargo build --dry-run",
            "cargo test\necho passed",
            "cargo test &",
            "cargo test $(echo --help)",
            "cargo test `echo --help`",
            "cargo test 'some filter'",
            "CARGO_HOME=/tmp cargo test",
            "cargo test *.rs",
            "node --test --watch",
            "node --test --test-list-tests",
            "pytest --collect-only",
            "pytest --fixtures",
            "pytest @args.txt",
            "pytest -k @args.txt",
            "python -m pytest @args.txt",
            "python3 -m pytest @args.txt",
            "py -m pytest @args.txt",
            "bun test --watch",
            "bun x nx run app:test --graph",
            "bun x nx run app:test --dryRun",
            "bun x nx run app:serve",
            "bun x nx run app:test:development:extra",
            "bun x nx run :test",
            "npm run test",
        ] {
            let review = CompletionReview::default();
            settled(&review, &call("write_file"), &ok());
            settled(&review, &call("read_file"), &ok());
            settled(&review, &bash(command), &ok());
            assert!(needs_review(&review).await, "not verification: {command}");
        }
    }

    #[tokio::test]
    async fn completion_review_unclassified_commands_do_not_clear_failed_checks() {
        for failed in ["cargo test", "git status"] {
            let review = CompletionReview::default();
            settled(&review, &call("write_file"), &ok());
            settled(&review, &call("read_file"), &ok());
            settled(&review, &bash("cargo test"), &ok());
            settled(&review, &bash(failed), &TaskResult::error("failed", 0));
            settled(&review, &bash("echo done"), &ok());
            assert!(needs_review(&review).await, "failure remains: {failed}");
        }
        let review = CompletionReview::default();
        settled(&review, &call("write_file"), &ok());
        settled(&review, &call("read_file"), &ok());
        settled(&review, &bash("cargo test"), &ok());
        settled(&review, &bash("git status"), &ok());
        assert!(
            needs_review(&review).await,
            "unscoped success never establishes coverage"
        );
    }

    #[tokio::test]
    async fn completion_review_missing_malformed_and_oversized_commands_are_unclassified() {
        let mut calls = vec![call("bash")];
        for value in [
            DataValue::Bool(true),
            DataValue::Number(0.0),
            DataValue::String(String::new()),
        ] {
            let mut call = call("bash");
            call.args.insert("command".into(), value);
            calls.push(call);
        }
        calls.push(bash(&format!("cargo test {}", "x".repeat(4096))));
        calls.push(bash(&format!("cargo test {}", "x ".repeat(128))));
        for native_call in calls {
            let review = CompletionReview::default();
            settled(&review, &call("write_file"), &ok());
            settled(&review, &call("read_file"), &ok());
            settled(&review, &native_call, &ok());
            assert!(needs_review(&review).await);
        }
        assert!(command::recognizes(&format!(
            "cargo test {}",
            "x".repeat(4096 - "cargo test ".len())
        )));
        assert!(command::recognizes(&format!(
            "cargo test {}",
            vec!["x"; 126].join(" ")
        )));
    }

    #[tokio::test]
    async fn completion_review_supported_simple_checks_still_require_resolved_scope() {
        for command in [
            "cargo test -p example --lib -- --exact selected_test",
            "cargo check --workspace --all-features",
            "cargo build --locked",
            "cargo clippy --all-targets",
            "cargo fmt --all -- --check",
            "node --test tests/report.test.cjs",
            "pytest -q tests/test_report.py",
            "python -m pytest tests/test_report.py",
            "python3 -m pytest -k report",
            "bun test tests/report.test.ts",
            "bun x nx run rust-daemon:test --skipNxCache",
            "bun x nx run @animaos/docs:build",
            "pnpm exec nx run app:typecheck:production",
            "npx nx run app:lint",
        ] {
            let review = CompletionReview::default();
            settled(&review, &call("write_file"), &ok());
            settled(&review, &call("read_file"), &ok());
            settled(&review, &bash(command), &ok());
            assert!(
                needs_review(&review).await,
                "recognized but unscoped check: {command}"
            );
        }
    }

    #[tokio::test]
    async fn completion_review_scripted_runtime_requires_a_recognized_check() {
        use crate::agent_runs::test_support::{ScriptedModel, Step};
        let identified = |id: &str, mut call: ToolCall| {
            call.id = id.into();
            call
        };
        let model = ScriptedModel::new(vec![
            Step::Tools(vec![identified("write", call("write_file"))]),
            Step::Tools(vec![
                identified("read", call("read_file")),
                identified("echo", bash("echo done")),
            ]),
            Step::Text(vec!["Premature final after an ordinary command."]),
            Step::Tools(vec![identified("check", bash("cargo test -p example"))]),
            Step::Text(vec!["Recognized command completed; scope remains limited."]),
        ]);
        let mut config = crate::agent_runs::test_support::companion_config("review");
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(["write_file", "read_file", "bash"])
                .unwrap(),
        );
        let mut runtime = AgentRuntime::new(config, model.clone());
        let review = CompletionReview::default();
        runtime.register_evaluator(Arc::new(review.clone()));
        let result = runtime
            .run_in_room_with_context_and_tools(
                "test:verification".into(),
                Vec::new(),
                Content::default(),
                |_, _, call| {
                    let review = review.clone();
                    async move {
                        // Controlled outcomes exercise the production translator/evaluator;
                        // this fixture does not execute a shell or prove real test success.
                        let result = ok();
                        settled(&review, &call, &result);
                        result
                    }
                },
            )
            .await;
        assert_eq!(result.status, TaskStatus::Success);
        assert_eq!(
            result.data.unwrap().text,
            "Recognized command completed; scope remains limited."
        );
        assert_eq!(model.requests().len(), 5);
        assert_eq!(
            runtime
                .messages()
                .iter()
                .filter(|message| message
                    .content
                    .text
                    .contains("Evaluator requested a revision: Completion review:"))
                .count(),
            1
        );
        assert_eq!(
            runtime
                .messages()
                .iter()
                .filter(|message| message.role == anima_core::MessageRole::Tool)
                .count(),
            4
        );
        let mut pending = std::collections::BTreeSet::new();
        for message in runtime.messages() {
            let Some(metadata) = &message.content.metadata else {
                continue;
            };
            if message.role == anima_core::MessageRole::Assistant {
                if let Some(DataValue::Array(calls)) = metadata.get("toolCalls") {
                    for call in calls {
                        let DataValue::Object(fields) = call else {
                            panic!("call object")
                        };
                        let DataValue::String(id) = &fields["id"] else {
                            panic!("call id")
                        };
                        assert!(pending.insert(id.clone()));
                    }
                }
            } else if message.role == anima_core::MessageRole::Tool {
                let Some(DataValue::String(id)) = metadata.get("toolCallId") else {
                    panic!("result id")
                };
                assert!(pending.remove(id));
            }
        }
        assert!(pending.is_empty());
    }
    #[tokio::test]
    async fn completion_review_failed_check_and_task_progress_are_retained() {
        let review = CompletionReview::default();
        settled(&review, &call("multi_edit"), &ok());
        settled(&review, &call("read_file"), &ok());
        settled(
            &review,
            &call("bash"),
            &TaskResult::error("tests failed", 0),
        );
        assert!(needs_review(&review).await);
        let review = CompletionReview::default();
        let mut tasks = call("todo_write");
        tasks.args.insert(
            "todos".into(),
            DataValue::Array(vec![DataValue::Object(
                [("status".into(), DataValue::String("pending".into()))].into(),
            )]),
        );
        settled(&review, &tasks, &ok());
        assert!(needs_review(&review).await);
    }
    #[test]
    fn completion_review_requires_exact_boolean_opt_in() {
        let mut config = crate::agent_runs::test_support::companion_config("completion");
        assert!(!enabled(&config));
        for value in [
            DataValue::String("true".into()),
            DataValue::Number(1.0),
            DataValue::Bool(false),
        ] {
            config
                .settings
                .as_mut()
                .unwrap()
                .additional
                .insert("completionReview".into(), value);
            assert!(!enabled(&config));
        }
        config
            .settings
            .as_mut()
            .unwrap()
            .additional
            .insert("completionReview".into(), DataValue::Bool(true));
        assert!(enabled(&config));
    }

    #[tokio::test]
    async fn completion_review_helper_claims_and_empty_reads_cannot_clear_parent_edits() {
        let review = CompletionReview::default();
        settled(&review, &call("write_file"), &ok());
        settled(&review, &call("spawn_helper"), &ok());
        let mut read = call("read_file");
        read.args.insert("limit".into(), DataValue::Number(0.0));
        settled(&review, &read, &ok());
        assert!(needs_review(&review).await);
    }
}
