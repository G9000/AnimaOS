//! Bounded, advisory verification before a task's final answer.
//!
//! Adapted from Hermes's verification stop/evidence pattern; see HERMES-NOTICE.
//! The host supplies settled observations, not model claims. Even a recognized
//! check's success and a file excerpt are not a correctness proof.

use std::collections::{BTreeMap, BTreeSet};
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
    /// Recognition alone supplies no resolved scope and cannot verify edits.
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

/// Workspace identity and canonical relative path are supplied by the host.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompletionFile {
    pub workspace: String,
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionCheckOutcome {
    Passed,
    Failed,
    Unknown,
}

/// Captured before dispatch. Its fields cannot be replaced by a settled result.
#[derive(Clone, Debug)]
pub struct CompletionAttempt {
    id: u64,
    contract: String,
    versions: Vec<(CompletionFile, u64, Option<String>)>,
}

struct FileState {
    revision: u64,
    digest: Option<String>,
    unread: bool,
    requires_check: bool,
    required_contracts: BTreeSet<String>,
}

struct CheckState {
    attempt: CompletionAttempt,
    outcome: CompletionCheckOutcome,
    settled: bool,
}

#[derive(Default)]
struct ReviewState {
    files: BTreeMap<CompletionFile, FileState>,
    checks: BTreeMap<String, CheckState>,
    overflow: bool,
    ordinary_failure: bool,
    unfinished: usize,
    prompted: bool,
    revision: u64,
    next_attempt: u64,
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
                let digest = Some(format!(
                    "observed-edit-{}",
                    state.revision.saturating_add(1)
                ));
                record_edit(
                    &mut state,
                    CompletionFile {
                        workspace: String::new(),
                        path,
                    },
                    digest,
                    requires_check,
                    Vec::new(),
                );
            }
            CompletionObservation::ReadBack { path } => {
                if let Some(file) = state.files.get_mut(&CompletionFile {
                    workspace: String::new(),
                    path,
                }) {
                    file.unread = false;
                }
            }
            CompletionObservation::CommandFinished { passed }
            | CompletionObservation::VerificationFinished { passed } => {
                if !passed && state.files.values().any(|file| file.requires_check) {
                    state.ordinary_failure = true;
                }
            }
            CompletionObservation::TasksSaved { unfinished } => {
                state.unfinished = unfinished;
            }
        }
    }

    /// Record an acknowledged native edit and the bytes actually observed by
    /// the host. Missing identities remain gaps rather than success evidence.
    pub fn record_edit(
        &self,
        file: CompletionFile,
        digest: Option<String>,
        requires_check: bool,
        required_contracts: Vec<String>,
    ) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        record_edit(&mut state, file, digest, requires_check, required_contracts);
    }

    pub fn tracked_files(&self) -> Vec<CompletionFile> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .files
            .keys()
            .cloned()
            .collect()
    }

    /// Refresh an already tracked file from bounded real-byte host snapshots.
    /// External changes invalidate both read-back and verification versions.
    pub fn refresh_file(&self, file: &CompletionFile, digest: Option<String>) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let digest = bounded_digest(digest);
        if state
            .files
            .get(file)
            .is_some_and(|current| current.digest != digest)
        {
            state.revision = state.revision.saturating_add(1);
            let revision = state.revision;
            let current = state.files.get_mut(file).expect("tracked file");
            current.digest = digest;
            current.revision = revision;
            current.unread = true;
        }
    }

    pub fn read_back(
        &self,
        checkpoint: CompletionCheckpoint,
        file: &CompletionFile,
        digest: Option<&str>,
    ) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if checkpoint.0 != state.revision || digest.is_none() {
            return;
        }
        if let Some(current) = state.files.get_mut(file) {
            if current.digest.as_deref() == digest {
                current.unread = false;
            }
        }
    }

    pub fn note_gap(&self) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .overflow = true;
    }

    /// Starting a newer attempt immediately supersedes older success. Pending,
    /// failed and uncertain attempts all leave their requirements unresolved.
    pub fn begin_check(&self, contract: &str, files: Vec<CompletionFile>) -> CompletionAttempt {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.next_attempt = state.next_attempt.saturating_add(1);
        let valid = valid_id(contract)
            && files.len() <= 64
            && (state.checks.len() < 32 || state.checks.contains_key(contract));
        if !valid {
            state.overflow = true;
        }
        let versions = if valid {
            files
                .into_iter()
                .filter_map(|file| {
                    let current = state.files.get(&file)?;
                    Some((file, current.revision, current.digest.clone()))
                })
                .collect()
        } else {
            Vec::new()
        };
        let attempt = CompletionAttempt {
            id: state.next_attempt,
            contract: contract.into(),
            versions,
        };
        if valid {
            state.checks.insert(
                contract.into(),
                CheckState {
                    attempt: attempt.clone(),
                    outcome: CompletionCheckOutcome::Unknown,
                    settled: false,
                },
            );
        }
        attempt
    }

    pub fn finish_check(&self, attempt: CompletionAttempt, outcome: CompletionCheckOutcome) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if !state
            .checks
            .get(&attempt.contract)
            .is_some_and(|latest| latest.attempt.id == attempt.id && !latest.settled)
        {
            return;
        }
        let unchanged = !attempt.versions.is_empty()
            && attempt.versions.iter().all(|(file, revision, digest)| {
                digest.is_some()
                    && state.files.get(file).is_some_and(|current| {
                        current.revision == *revision && current.digest == *digest
                    })
            });
        let outcome = if outcome == CompletionCheckOutcome::Passed && !unchanged {
            CompletionCheckOutcome::Unknown
        } else {
            outcome
        };
        let latest = state
            .checks
            .get_mut(&attempt.contract)
            .expect("current attempt");
        latest.outcome = outcome;
        latest.settled = true;
    }

    fn take_feedback(&self) -> Option<String> {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let unread = state.files.values().any(|file| file.unread);
        let unchecked = state
            .files
            .iter()
            .filter(|(_, file)| file.requires_check)
            .filter(|(path, file)| {
                file.digest.is_none()
                    || file.required_contracts.is_empty()
                    || file.required_contracts.iter().any(|contract| {
                        !state.checks.get(contract).is_some_and(|check| {
                            check.outcome == CompletionCheckOutcome::Passed
                                && check.attempt.versions.iter().any(
                                    |(covered, revision, digest)| {
                                        covered == *path
                                            && *revision == file.revision
                                            && *digest == file.digest
                                    },
                                )
                        })
                    })
            })
            .map(|(path, _)| path.path.chars().take(160).collect::<String>())
            .take(5)
            .collect::<Vec<_>>();
        if state.prompted
            || (!unread
                && !state.overflow
                && unchecked.is_empty()
                && !state.ordinary_failure
                && state.unfinished == 0)
        {
            return None;
        }
        state.prompted = true;
        let mut gaps = Vec::new();
        if unread {
            gaps.push("some edited files have no later read-back observation".to_string());
        }
        if !unchecked.is_empty() {
            gaps.push(format!("changed files lack current successful command evidence for every matching owner verification contract (missing contracts, failed/unknown outcomes or stale versions): {unchecked:?}"));
        }
        if state.ordinary_failure {
            gaps.push(
                "a later unscoped command failed; inspect and report its actual outcome"
                    .to_string(),
            );
        }
        if state.overflow {
            gaps.push(
                "some evidence could not be captured within the path or snapshot bounds"
                    .to_string(),
            );
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

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}

fn bounded_digest(digest: Option<String>) -> Option<String> {
    digest.filter(|value| {
        !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
    })
}

fn record_edit(
    state: &mut ReviewState,
    file: CompletionFile,
    digest: Option<String>,
    requires_check: bool,
    required_contracts: Vec<String>,
) {
    if file.path.is_empty()
        || file.path.len() > 4096
        || file.workspace.len() > 4096
        || file.path.chars().any(char::is_control)
        || file.workspace.chars().any(char::is_control)
        || (state.files.len() >= 64 && !state.files.contains_key(&file))
        || required_contracts.len() > 32
        || required_contracts.iter().any(|id| !valid_id(id))
    {
        state.overflow = true;
        return;
    }
    state.revision = state.revision.saturating_add(1);
    state.files.insert(
        file,
        FileState {
            revision: state.revision,
            digest: bounded_digest(digest),
            unread: true,
            requires_check,
            required_contracts: required_contracts.into_iter().collect(),
        },
    );
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
        review.record_edit(
            CompletionFile {
                workspace: String::new(),
                path: path.into(),
            },
            Some(path.into()),
            requires_check,
            vec!["check".into()],
        );
    }
    fn read(review: &CompletionReview, path: &str) {
        review.observe(CompletionObservation::ReadBack { path: path.into() });
    }
    fn check(review: &CompletionReview, passed: bool) {
        let attempt = review.begin_check("check", review.tracked_files());
        review.finish_check(
            attempt,
            if passed {
                CompletionCheckOutcome::Passed
            } else {
                CompletionCheckOutcome::Failed
            },
        );
    }

    fn scoped_file(path: &str) -> CompletionFile {
        CompletionFile {
            workspace: "workspace".into(),
            path: path.into(),
        }
    }

    #[test]
    fn change_bound_completion_union_preserves_uncovered_paths() {
        for both in [false, true] {
            let review = CompletionReview::default();
            for (path, contract) in [("a/lib.rs", "a"), ("b/lib.rs", "b")] {
                let file = scoped_file(path);
                review.record_edit(
                    file.clone(),
                    Some("saved".into()),
                    true,
                    vec![contract.into()],
                );
            }
            for file in review.tracked_files() {
                review.read_back(review.checkpoint(), &file, Some("saved"));
            }
            let a = review.begin_check("a", vec![scoped_file("a/lib.rs")]);
            review.finish_check(a, CompletionCheckOutcome::Passed);
            if both {
                let b = review.begin_check("b", vec![scoped_file("b/lib.rs")]);
                review.finish_check(b, CompletionCheckOutcome::Passed);
            }
            assert_eq!(review.take_feedback().is_some(), !both);
        }
    }

    #[test]
    fn change_bound_completion_newer_failure_or_unknown_fences_old_pass() {
        for outcome in [
            CompletionCheckOutcome::Failed,
            CompletionCheckOutcome::Unknown,
        ] {
            let review = CompletionReview::default();
            edit(&review, "lib.rs", true);
            read(&review, "lib.rs");
            let old = review.begin_check("check", review.tracked_files());
            let newer = review.begin_check("check", review.tracked_files());
            review.finish_check(newer, outcome);
            review.finish_check(old, CompletionCheckOutcome::Passed);
            assert!(review.take_feedback().is_some());
        }
    }

    #[test]
    fn change_bound_completion_changed_digest_and_missing_bytes_remain_unverified() {
        for digest in [None, Some("different".into())] {
            let review = CompletionReview::default();
            let file = scoped_file("lib.rs");
            review.record_edit(
                file.clone(),
                Some("saved".into()),
                true,
                vec!["check".into()],
            );
            review.read_back(review.checkpoint(), &file, Some("saved"));
            let attempt = review.begin_check("check", vec![file.clone()]);
            review.refresh_file(&file, digest);
            review.finish_check(attempt, CompletionCheckOutcome::Passed);
            let feedback = review.take_feedback().unwrap();
            assert!(feedback.contains("read-back"));
            assert!(feedback.contains("command"));
        }
    }

    #[test]
    fn change_bound_completion_each_declared_requirement_needs_current_success() {
        let review = CompletionReview::default();
        let file = scoped_file("lib.rs");
        review.record_edit(
            file.clone(),
            Some("saved".into()),
            true,
            vec!["test".into(), "lint".into()],
        );
        review.read_back(review.checkpoint(), &file, Some("saved"));
        let attempt = review.begin_check("test", vec![file]);
        review.finish_check(attempt, CompletionCheckOutcome::Passed);
        assert!(review.take_feedback().is_some());
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
    fn change_bound_completion_unscoped_green_check_cannot_verify_changed_code() {
        let review = CompletionReview::default();
        edit(&review, "crate-a/src/lib.rs", true);
        read(&review, "crate-a/src/lib.rs");
        review.observe(CompletionObservation::VerificationFinished { passed: true });
        assert!(
            review.take_feedback().is_some(),
            "a green command without resolved scope cannot cover crate A"
        );
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
