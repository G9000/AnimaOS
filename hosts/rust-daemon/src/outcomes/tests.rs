use super::*;
use anima_core::outcomes::{
    OutcomeAcceptance, OutcomeCriterion, OutcomeDefinition, OutcomeVerdict,
};

fn definition(criterion: OutcomeCriterion) -> OutcomeDefinition {
    OutcomeDefinition {
        criteria: vec![criterion],
        max_corrections: 2,
        acceptance: OutcomeAcceptance::ReportOnly,
    }
}

#[test]
fn outcome_file_text_grades_actual_saved_bytes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("report.txt"), "wrong total").unwrap();
    let plan = definition(OutcomeCriterion::FileText {
        id: "total".into(),
        path: "report.txt".into(),
        contains: "Total: 42".into(),
    });
    let before = verification::observe(&plan, Some(root.path()), "producer says done");
    assert_eq!(before[0].verdict, OutcomeVerdict::NotMet);
    std::fs::write(root.path().join("report.txt"), "Total: 42").unwrap();
    let after = verification::observe(&plan, Some(root.path()), "producer says done");
    assert_eq!(after[0].verdict, OutcomeVerdict::Met);
    assert_ne!(before[0].subject_sha256, after[0].subject_sha256);
}

#[test]
fn outcome_json_checks_public_value_and_missing_engine_is_inconclusive() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("result.json"), r#"{"total":41}"#).unwrap();
    let plan = definition(OutcomeCriterion::FileJson {
        id: "total".into(),
        path: "result.json".into(),
        pointer: "/total".into(),
        expected: serde_json::json!(42),
    });
    assert_eq!(
        verification::observe(&plan, Some(root.path()), "")[0].verdict,
        OutcomeVerdict::NotMet
    );
    let quality = definition(OutcomeCriterion::Quality {
        id: "visual".into(),
        rubric: "Readable pages".into(),
    });
    assert_eq!(
        verification::observe(&quality, Some(root.path()), "looks good")[0].verdict,
        OutcomeVerdict::Inconclusive
    );
}

#[test]
fn outcome_repair_fence_blocks_confirmed_send_repetition_and_opaque_commands() {
    let paths = vec!["report.txt".to_string()];
    for name in [
        "send_email",
        "bash",
        "spawn_helper",
        "browser",
        "mcp_send",
        "discover_tools",
        "write_document",
    ] {
        let call = ToolCall {
            id: "different-key".into(),
            name: name.into(),
            args: Default::default(),
        };
        assert!(repair::check_call(&paths, &call).is_err(), "blocked {name}");
    }
    let mut call = ToolCall {
        id: "repair".into(),
        name: "edit_file".into(),
        args: Default::default(),
    };
    call.args.insert(
        "file_path".into(),
        anima_core::DataValue::String("report.txt".into()),
    );
    assert!(repair::check_call(&paths, &call).is_ok());
    call.args.insert(
        "file_path".into(),
        anima_core::DataValue::String("unrelated.txt".into()),
    );
    assert!(repair::check_call(&paths, &call).is_err());
}

#[test]
fn outcome_read_is_bounded_and_cannot_follow_a_parent_path() {
    let call = ToolCall {
        id: "read".into(),
        name: "read_file".into(),
        args: [(
            "file_path".into(),
            anima_core::DataValue::String("../private.txt".into()),
        )]
        .into(),
    };
    assert!(repair::check_call(&["report.txt".into()], &call).is_err());
}

#[test]
fn outcome_freshness_rejects_report_substitution_with_identical_source_bytes() {
    use anima_core::outcomes::OutcomeObservation;
    let observed = OutcomeObservation {
        criterion_id: "pages".into(),
        verdict: OutcomeVerdict::Met,
        subject_sha256: Some("a".repeat(64)),
        report_sha256: Some("b".repeat(64)),
        locator: Some("complete_rendering".into()),
        detail: "Complete exported view".into(),
    };
    let mut substituted = observed.clone();
    substituted.report_sha256 = Some("c".repeat(64));
    assert!(!verification::same_subjects(
        &[observed.clone()],
        &[substituted]
    ));
    let mut substituted = observed.clone();
    substituted.locator = Some("recalculated_without_errors".into());
    assert!(!verification::same_subjects(&[observed], &[substituted]));
}

#[test]
fn outcome_raw_file_checks_do_not_inherit_generated_document_read_authority() {
    let mut config = crate::agent_runs::test_support::companion_config("scope");
    config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["read_document_source"])
            .unwrap(),
    );
    let criterion = OutcomeCriterion::FileText {
        id: "text".into(),
        path: "notes.txt".into(),
        contains: "public".into(),
    };
    assert!(!observation_allowed(&criterion, &config, &config));
}

#[test]
fn outcome_failed_owner_reply_preserves_history_through_review_and_close_but_rejects_unmet_or_forged_evidence(
) {
    use crate::runs::{RunLedger, RunSource, RunStart, RunStatus};

    fn failed_owner_report(response: &str) -> RunRecord {
        let mut record = RunRecord::running(
            RunStart {
                agent_id: "agent-owner".into(),
                session_id: "chat:owner".into(),
                source: RunSource::Web,
                source_ref: None,
                idempotency_key: Some("owner-result".into()),
                text: "Prepare an owner-reviewed result".into(),
                model: "gpt-5.4".into(),
                provider: Some("openai".into()),
                parent_run_id: None,
            },
            1,
        );
        attach(
            &mut record,
            OutcomeDefinition {
                criteria: vec![OutcomeCriterion::ResponseText {
                    id: "result".into(),
                    contains: "Ready".into(),
                }],
                max_corrections: 0,
                acceptance: OutcomeAcceptance::OwnerRequired,
            },
            None,
        )
        .unwrap();
        let outcome = record.outcome.as_mut().unwrap();
        let iteration = OutcomeIteration {
            ordinal: 1,
            binding_sha256: outcome.binding_sha256(),
            semantic_sha256: outcome.semantic_sha256(),
            reply_sha256: digest(response.as_bytes()),
            observations: verification::observe(&outcome.definition, None, response),
        };
        outcome.settle(iteration).unwrap();
        record.finish(RunStatus::Failed, None, 2);
        record.reply_message_id = Some("msg-owner-result".into());
        record.reply_sha256 = Some(digest(response.as_bytes()));
        record
    }

    let ready = failed_owner_report("Ready");
    let historical = ready.outcome.as_ref().unwrap().clone();
    for status in [
        OutcomeStatus::AwaitingOwner,
        OutcomeStatus::NeedsReview,
        OutcomeStatus::Stopped,
    ] {
        let mut record = ready.clone();
        if status != OutcomeStatus::AwaitingOwner {
            record.outcome.as_mut().unwrap().hold(status).unwrap();
        }
        assert_eq!(
            record.allows_outcome_review_delivery(),
            status == OutcomeStatus::AwaitingOwner
        );
        let restored: RunRecord =
            serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
        RunLedger::validate(std::slice::from_ref(&restored)).unwrap();
        assert_eq!(restored.reply_message_id, ready.reply_message_id);
        assert_eq!(restored.reply_sha256, ready.reply_sha256);
        let outcome = restored.outcome.as_ref().unwrap();
        assert_eq!(outcome.iterations, historical.iterations);
        assert_eq!(outcome.report_sha256, historical.report_sha256);
        assert_eq!(
            outcome.consumed_corrections,
            historical.consumed_corrections
        );
    }

    let mut ordinary_failure = ready.clone();
    ordinary_failure.outcome = None;
    assert!(RunLedger::validate(&[ordinary_failure]).is_err());
    let mut exhausted = ready.clone();
    exhausted
        .outcome
        .as_mut()
        .unwrap()
        .hold(OutcomeStatus::Exhausted)
        .unwrap();
    assert!(
        RunLedger::validate(&[exhausted]).is_err(),
        "Exhausted cannot acquire a committed reply exception"
    );
    let mut closed_unmet = failed_owner_report("Wrong result");
    assert_eq!(
        closed_unmet.outcome.as_ref().unwrap().status,
        OutcomeStatus::Exhausted
    );
    closed_unmet
        .outcome
        .as_mut()
        .unwrap()
        .hold(OutcomeStatus::Stopped)
        .unwrap();
    assert!(
        RunLedger::validate(&[closed_unmet]).is_err(),
        "closing an unmet report cannot validate its reply"
    );
    let mut substituted = ready.clone();
    substituted.reply_sha256 = Some(digest(b"substituted reply"));
    assert!(RunLedger::validate(&[substituted]).is_err());
    let mut missing_reply = ready.clone();
    missing_reply.reply_message_id = None;
    assert!(RunLedger::validate(&[missing_reply]).is_err());
    let mut changed_task = ready;
    changed_task.input.text = "Changed task".into();
    assert!(RunLedger::validate(&[changed_task]).is_err());
}
