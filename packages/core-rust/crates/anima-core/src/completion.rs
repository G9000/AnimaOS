//! Bounded, advisory verification before a task's final answer.
//!
//! Adapted from Hermes's verification stop/evidence pattern; see HERMES-NOTICE.
//! The host supplies settled observations, not model claims. Even a recognized
//! check's success and a file excerpt are not a correctness proof.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::components::{Evaluator, EvaluatorResult};
use crate::{AgentRuntime, Content, Message};

/// Facts supplied by the host after a tool and its outcome have settled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionObservation {
    Edited {
        path: String,
        requires_check: bool,
    },
    ReadBack {
        path: String,
    },
    /// An ordinary or unclassified command cannot prove verification on success.
    CommandFinished {
        passed: bool,
    },
    /// The host recognized a verification invocation; scope/relevance remain unproven.
    VerificationFinished {
        passed: bool,
    },
    TasksSaved {
        unfinished: usize,
    },
}

/// A per-run shared review, never a grant or an executor. Clones share evidence.
#[derive(Clone, Default)]
pub struct CompletionReview(Arc<Mutex<ReviewState>>);

/// Capture before dispatch so a concurrent edit cannot be verified by a
/// read/check that was already in flight.
#[derive(Clone, Copy)]
pub struct CompletionCheckpoint(u64);

#[derive(Default)]
struct ReviewState {
    unread: BTreeSet<String>,
    overflow: bool,
    code_changed: bool,
    needs_check: bool,
    unfinished: usize,
    prompted: bool,
    revision: u64,
}

impl CompletionReview {
    pub fn checkpoint(&self) -> CompletionCheckpoint {
        CompletionCheckpoint(
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .revision,
        )
    }

    /// Use `observe_after(checkpoint, ...)` when an observation involves async
    /// work. This convenience method records an immediately settled fact.
    pub fn observe(&self, observation: CompletionObservation) {
        self.observe_after(self.checkpoint(), observation);
    }

    pub fn observe_after(
        &self,
        checkpoint: CompletionCheckpoint,
        observation: CompletionObservation,
    ) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if matches!(
            &observation,
            CompletionObservation::ReadBack { .. }
                | CompletionObservation::CommandFinished { .. }
                | CompletionObservation::VerificationFinished { .. }
        ) && checkpoint.0 != state.revision
        {
            return;
        }
        match observation {
            CompletionObservation::Edited {
                path,
                requires_check,
            } => {
                state.revision = state.revision.saturating_add(1);
                // Evidence remains bounded even for a long-running caller.
                if path.len() > 4096 || (state.unread.len() >= 64 && !state.unread.contains(&path))
                {
                    state.overflow = true;
                } else {
                    state.unread.insert(path);
                }
                state.code_changed |= requires_check;
                state.needs_check |= requires_check;
            }
            CompletionObservation::ReadBack { path } => {
                state.unread.remove(&path);
            }
            CompletionObservation::CommandFinished { passed } => {
                if state.code_changed && !passed {
                    state.needs_check = true;
                }
            }
            CompletionObservation::VerificationFinished { passed } => {
                if state.code_changed {
                    state.needs_check = !passed;
                }
            }
            CompletionObservation::TasksSaved { unfinished } => {
                state.unfinished = unfinished;
            }
        }
    }

    fn take_feedback(&self) -> Option<String> {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.prompted
            || (state.unread.is_empty()
                && !state.overflow
                && !state.needs_check
                && state.unfinished == 0)
        {
            return None;
        }
        state.prompted = true;
        let mut gaps = Vec::new();
        if !state.unread.is_empty() || state.overflow {
            gaps.push("some edited files have no later read-back observation".to_string());
        }
        if state.needs_check {
            gaps.push("no successful recognized verification command follows the latest code edit, or a later command failed".to_string());
        }
        if state.unfinished > 0 {
            gaps.push(format!(
                "{} unfinished saved tasks remain",
                state.unfinished
            ));
        }
        Some(format!("Completion review: {}. Before finishing, inspect the actual settled results, perform relevant permitted checks and read back the saved output. Update only tasks you actually completed; preserve unrelated or blocked tasks. A successful command or file excerpt does not prove the whole task. If checks are unavailable, denied, failed or outside the remaining budget, stop and clearly report the completed work, remaining work and limitations. Do not invent evidence or repeat uncertain external actions. This is the only automatic completion reminder.", gaps.join("; ")))
    }
}

#[async_trait]
impl Evaluator for CompletionReview {
    fn name(&self) -> &str {
        "completion_review"
    }
    fn description(&self) -> &str {
        "One bounded review of settled task evidence before finishing"
    }
    fn requires_tool_budget(&self) -> bool {
        true
    }
    async fn validate(&self, _runtime: &AgentRuntime, _message: &Message) -> Result<bool, String> {
        Ok(true)
    }
    async fn evaluate(
        &self,
        _runtime: &AgentRuntime,
        _message: &Message,
        _response: &Content,
    ) -> Result<EvaluatorResult, String> {
        Ok(match self.take_feedback() {
            Some(feedback) => EvaluatorResult::retry(feedback),
            None => EvaluatorResult::accept(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(review: &CompletionReview, path: &str, requires_check: bool) {
        review.observe(CompletionObservation::Edited {
            path: path.into(),
            requires_check,
        });
    }
    fn read(review: &CompletionReview, path: &str) {
        review.observe(CompletionObservation::ReadBack { path: path.into() });
    }
    fn check(review: &CompletionReview, passed: bool) {
        review.observe(CompletionObservation::VerificationFinished { passed });
    }

    #[test]
    fn completion_review_requests_one_bounded_corrective_pass_after_edits() {
        let review = CompletionReview::default();
        edit(&review, "src/lib.rs", true);
        let feedback = review
            .take_feedback()
            .expect("unfinished edits need a review");
        assert!(feedback.contains("read-back"));
        assert!(feedback.contains("command"));
        assert!(
            review.take_feedback().is_none(),
            "never force an unbounded loop"
        );
    }

    #[test]
    fn completion_review_accepts_fresh_check_and_read_back_without_extra_inference() {
        let review = CompletionReview::default();
        edit(&review, "src/lib.rs", true);
        read(&review, "src/lib.rs");
        check(&review, true);
        review.observe(CompletionObservation::TasksSaved { unfinished: 0 });
        assert!(review.take_feedback().is_none());
    }

    #[test]
    fn completion_review_invalidates_checks_and_read_back_on_later_edit() {
        let review = CompletionReview::default();
        edit(&review, "src/lib.rs", true);
        read(&review, "src/lib.rs");
        check(&review, true);
        edit(&review, "src/lib.rs", true);
        let feedback = review.take_feedback().unwrap();
        assert!(feedback.contains("read-back"));
        assert!(feedback.contains("command"));
    }

    #[test]
    fn autonomy_completion_reediting_a_tracked_path_at_capacity_does_not_create_a_false_gap() {
        let review = CompletionReview::default();
        for index in 0..64 {
            edit(&review, &format!("report-{index}.txt"), false);
        }
        edit(&review, "report-0.txt", false);
        for index in 0..64 {
            read(&review, &format!("report-{index}.txt"));
        }
        assert!(review.take_feedback().is_none());
    }

    #[test]
    fn completion_review_failed_command_does_not_satisfy_check() {
        let review = CompletionReview::default();
        edit(&review, "src/lib.rs", true);
        read(&review, "src/lib.rs");
        check(&review, true);
        check(&review, false);
        assert!(review.take_feedback().unwrap().contains("command"));
    }

    #[test]
    fn completion_review_ordinary_success_is_not_verification() {
        let review = CompletionReview::default();
        edit(&review, "src/lib.rs", true);
        read(&review, "src/lib.rs");
        review.observe(CompletionObservation::CommandFinished { passed: true });
        assert!(review.take_feedback().is_some());
    }

    #[test]
    fn completion_review_docs_only_need_read_back_not_a_runtime_check() {
        let review = CompletionReview::default();
        edit(&review, "README.md", false);
        read(&review, "README.md");
        assert!(review.take_feedback().is_none());
    }

    #[test]
    fn completion_review_read_only_work_is_free_and_clones_share_progress() {
        let review = CompletionReview::default();
        read(&review, "src/lib.rs");
        check(&review, false);
        assert!(review.take_feedback().is_none());
        review
            .clone()
            .observe(CompletionObservation::TasksSaved { unfinished: 2 });
        assert!(review.take_feedback().unwrap().contains("2 unfinished"));
    }

    #[test]
    fn completion_review_cleared_tasks_and_unrelated_reads_do_not_hide_edits() {
        let review = CompletionReview::default();
        edit(&review, "report.txt", false);
        read(&review, "other.txt");
        review.observe(CompletionObservation::TasksSaved { unfinished: 3 });
        review.observe(CompletionObservation::TasksSaved { unfinished: 0 });
        assert!(review.take_feedback().unwrap().contains("read-back"));
    }

    #[test]
    fn completion_review_inflight_reads_and_checks_cannot_verify_a_concurrent_edit() {
        let review = CompletionReview::default();
        edit(&review, "src/lib.rs", true);
        let before = review.checkpoint();
        edit(&review, "src/lib.rs", true);
        review.observe_after(
            before,
            CompletionObservation::ReadBack {
                path: "src/lib.rs".into(),
            },
        );
        review.observe_after(
            before,
            CompletionObservation::VerificationFinished { passed: true },
        );
        let feedback = review.take_feedback().unwrap();
        assert!(feedback.contains("read-back"));
        assert!(feedback.contains("command"));
    }

    struct AnswerModel(std::sync::atomic::AtomicUsize);

    #[async_trait]
    impl crate::ModelAdapter for AnswerModel {
        fn provider(&self) -> &str {
            "completion-test"
        }
        async fn generate(
            &self,
            _config: &crate::AgentConfig,
            _request: &crate::ModelGenerateRequest,
        ) -> Result<crate::ModelGenerateResponse, String> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(crate::ModelGenerateResponse {
                content: Content {
                    text: "Saved changes; verification remains unavailable.".into(),
                    ..Default::default()
                },
                tool_calls: None,
                usage: crate::TokenUsage::default(),
                stop_reason: crate::ModelStopReason::End,
            })
        }
    }

    #[test]
    fn completion_review_runtime_preserves_honest_limitations_and_budget_wrapup() {
        for (rounds, expected_calls) in [(0, 1), (8, 2)] {
            let model = Arc::new(AnswerModel(std::sync::atomic::AtomicUsize::new(0)));
            let mut config: crate::AgentConfig = serde_json::from_value(
                serde_json::json!({"name":"completion-test","model":"offline"}),
            )
            .unwrap();
            config.settings = Some(crate::AgentSettings {
                max_tool_iterations: Some(rounds),
                ..Default::default()
            });
            let mut runtime = AgentRuntime::new(config, model.clone());
            let review = CompletionReview::default();
            edit(&review, "src/lib.rs", true);
            runtime.register_evaluator(Arc::new(review));
            let result = futures::executor::block_on(runtime.run(Content {
                text: "finish the task".into(),
                ..Default::default()
            }));
            assert_eq!(result.status, crate::TaskStatus::Success);
            assert_eq!(
                model.0.load(std::sync::atomic::Ordering::SeqCst),
                expected_calls
            );
            assert_eq!(
                runtime
                    .messages()
                    .iter()
                    .filter(|message| message
                        .content
                        .text
                        .contains("Evaluator requested a revision: Completion review:"))
                    .count(),
                expected_calls - 1
            );
            assert!(result.data.unwrap().text.contains("unavailable"));
        }
    }

    #[test]
    fn completion_review_cancelled_run_does_not_request_a_correction() {
        let model = Arc::new(AnswerModel(std::sync::atomic::AtomicUsize::new(0)));
        let config =
            serde_json::from_value(serde_json::json!({"name":"completion-test","model":"offline"}))
                .unwrap();
        let mut runtime = AgentRuntime::new(config, model.clone());
        let review = CompletionReview::default();
        edit(&review, "src/lib.rs", true);
        runtime.register_evaluator(Arc::new(review));
        let control = crate::RunControl::new();
        control.cancel.cancel();
        runtime.set_run_control(control);
        let result = futures::executor::block_on(runtime.run(Content::default()));
        assert_eq!(result.status, crate::TaskStatus::Error);
        assert_eq!(model.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}
