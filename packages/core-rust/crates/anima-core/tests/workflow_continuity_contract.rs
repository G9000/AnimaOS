use anima_core::workflows::{
    WorkflowInput, WorkflowResumeRequest, WorkflowRun, WorkflowStatus, WorkflowStepKind,
    WorkflowStepSpec, WorkflowStepStatus,
};
use serde_json::json;

fn workflow() -> WorkflowRun {
    WorkflowRun::new(
        "workflow-1",
        "agent-1",
        WorkflowInput {
            title: "A saved mission".into(),
            request_key: "mission-1".into(),
            steps: vec![
                WorkflowStepSpec {
                    id: "draft".into(),
                    title: "Draft".into(),
                    kind: WorkflowStepKind::Task,
                    prompt: "Prepare a draft".into(),
                },
                WorkflowStepSpec {
                    id: "review".into(),
                    title: "Review".into(),
                    kind: WorkflowStepKind::Approval,
                    prompt: "Review the draft".into(),
                },
                WorkflowStepSpec {
                    id: "finish".into(),
                    title: "Finish".into(),
                    kind: WorkflowStepKind::Task,
                    prompt: "Finish the work".into(),
                },
            ],
        },
        10,
    )
    .expect("valid workflow")
}

fn decision(revision: u64, action: &str) -> WorkflowResumeRequest {
    serde_json::from_value(json!({"revision": revision, "action": action}))
        .expect("supported workflow decision")
}

#[test]
fn pause_preserves_progress_and_requires_a_separate_checkpoint_decision() {
    let mut run = workflow();
    let pause = decision(run.revision, "pause");
    assert!(run.resume(pause.clone(), 11).unwrap());
    assert_eq!(run.current_step, 0);
    assert_eq!(run.steps[0].status, WorkflowStepStatus::Pending);
    assert!(run.attach_job("job-before-continue", 12).is_err());
    assert!(
        !run.resume(pause, 12).unwrap(),
        "identical replay is a no-op"
    );

    assert!(run.resume(decision(run.revision, "continue"), 13).unwrap());
    run.attach_job("job-1", 14).unwrap();
    assert!(run.resume(decision(run.revision, "pause"), 15).unwrap());
    run.complete_task("Draft saved", 16).unwrap();
    assert_eq!(run.status, WorkflowStatus::AwaitingApproval);
    assert_eq!(run.steps[0].result.as_deref(), Some("Draft saved"));
    assert!(run.resume(decision(run.revision, "approve"), 17).is_err());

    let resume = decision(run.revision, "continue");
    assert!(run.resume(resume.clone(), 18).unwrap());
    assert_eq!(run.status, WorkflowStatus::AwaitingApproval);
    assert!(!run.resume(resume, 19).unwrap());
    assert_eq!(run.current_step, 1, "replay cannot approve the checkpoint");
    assert!(run.resume(decision(run.revision, "approve"), 20).unwrap());
    assert_eq!(run.current_step, 2);
    run.validate().expect("saved history is reachable");
}

#[test]
fn paused_snapshot_round_trips_and_cannot_be_forged() {
    let mut run = workflow();
    run.resume(decision(run.revision, "pause"), 11).unwrap();
    let saved = serde_json::to_value(&run).unwrap();
    assert_eq!(saved["paused"], true);
    let restored: WorkflowRun = serde_json::from_value(saved).unwrap();
    restored.validate().expect("the saved hold is reachable");

    let mut forged = serde_json::to_value(workflow()).unwrap();
    forged["paused"] = json!(true);
    let forged: WorkflowRun = serde_json::from_value(forged).unwrap();
    assert!(forged.validate().is_err(), "a hold needs a saved decision");

    let legacy = serde_json::to_value(workflow()).unwrap();
    let restored: WorkflowRun = serde_json::from_value(legacy).unwrap();
    restored
        .validate()
        .expect("legacy workflow remains readable");
}

#[test]
fn pause_reserves_room_for_continue_at_the_decision_limit() {
    let mut run = workflow();
    for index in 0..31 {
        let time = 11 + index * 2;
        run.resume(decision(run.revision, "pause"), time).unwrap();
        run.resume(decision(run.revision, "continue"), time + 1)
            .unwrap();
    }
    run.resume(decision(run.revision, "pause"), 73).unwrap();
    run.resume(decision(run.revision, "continue"), 74).unwrap();
    assert_eq!(run.decisions.len(), 64);
    assert!(run.resume(decision(run.revision, "pause"), 75).is_err());

    let mut run = workflow();
    run.attach_job("completed-job", 11).unwrap();
    run.complete_task("Done", 12).unwrap();
    run.resume(decision(run.revision, "approve"), 13).unwrap();
    for index in 0..31 {
        let time = 14 + index * 2;
        run.resume(decision(run.revision, "pause"), time).unwrap();
        run.resume(decision(run.revision, "continue"), time + 1)
            .unwrap();
    }
    assert_eq!(run.decisions.len(), 63);
    assert!(
        run.resume(decision(run.revision, "pause"), 77).is_err(),
        "one remaining decision slot cannot safely hold work"
    );
}

#[test]
fn a_held_review_preserves_the_failure_and_never_retries_by_continuing() {
    let mut run = workflow();
    run.attach_job("failed-job", 11).unwrap();
    run.needs_review("An effect is uncertain", 12).unwrap();
    run.resume(decision(run.revision, "pause"), 13).unwrap();
    let mut retry = decision(run.revision, "retry");
    retry.acknowledge_uncertain = true;
    assert!(run.resume(retry, 14).is_err());
    assert_eq!(run.error.as_deref(), Some("An effect is uncertain"));
    assert_eq!(run.steps[0].attempt, 1);
    run.resume(decision(run.revision, "continue"), 15).unwrap();
    assert_eq!(run.status, WorkflowStatus::NeedsReview);
    assert_eq!(run.error.as_deref(), Some("An effect is uncertain"));
    assert_eq!(run.steps[0].job_id.as_deref(), Some("failed-job"));
}

#[test]
fn cancellation_of_a_held_workflow_is_terminal_across_restore() {
    let mut run = workflow();
    let pause = decision(run.revision, "pause");
    run.resume(pause.clone(), 11).unwrap();
    run.cancel(run.revision, 12).unwrap();
    assert_eq!(run.status, WorkflowStatus::Cancelled);
    assert!(
        !run.resume(pause, 13).unwrap(),
        "the old pause is an inert replay"
    );
    assert!(run.resume(decision(run.revision, "continue"), 14).is_err());
    let restored: WorkflowRun =
        serde_json::from_value(serde_json::to_value(&run).unwrap()).unwrap();
    restored
        .validate()
        .expect("the held stop remains reachable");
    assert_eq!(restored.status, WorkflowStatus::Cancelled);
}
