use std::collections::{BTreeMap, BTreeSet};

use anima_core::task_regressions::{
    compare_planned_tasks, evaluator_fingerprint, ActorFixture, ArtifactObservationRef,
    ComparisonPlan, FixtureOracle, MockEffect, RegressionFixture, RegressionSuite,
    TrialObservation, VariantPin,
};
use anima_core::trace_evaluation::{
    compare_traces, regrade_observation, GradedTrace, TaskTrace, TraceEvent, TraceEventKind,
    TraceSettings, TraceVariant, TRACE_VERSION,
};
use anima_core::{Content, RunFrame, RunObserver, TaskResult};

fn suite() -> RegressionSuite {
    RegressionSuite {
        name: "isolated file task".into(),
        fixtures: vec![RegressionFixture {
            id: "edit".into(),
            actor: ActorFixture {
                prompt: "Update result.txt using input.txt and reply done.".into(),
                initial_files: BTreeMap::from([("input.txt".into(), "public input".into())]),
                readable_files: vec!["input.txt".into()],
                writable_files: vec!["result.txt".into()],
                mock_effects: true,
            },
            oracle: FixtureOracle {
                expected_files: BTreeMap::from([
                    ("input.txt".into(), "public input".into()),
                    ("result.txt".into(), "private final-state canary".into()),
                ]),
                expected_output: "done".into(),
                expected_mock_effects: vec![MockEffect {
                    key: "send".into(),
                    payload: "private receipt canary".into(),
                }],
            },
        }],
    }
}

fn plan(suite: &RegressionSuite) -> ComparisonPlan {
    let settings = TraceSettings {
        provider: "scripted".into(),
        model: "baseline-model".into(),
        configuration_id: "baseline-config".into(),
        fixture_set_id: suite.digest().unwrap(),
    };
    let baseline = VariantPin {
        settings,
        source_digest: "a".repeat(64),
        tool_policy_digest: "b".repeat(64),
    };
    let mut candidate = baseline.clone();
    candidate.settings.model = "candidate-model".into();
    candidate.settings.configuration_id = "candidate-config".into();
    ComparisonPlan {
        suite_digest: suite.digest().unwrap(),
        repetitions: 1,
        baseline,
        candidate,
        evaluator_digest: evaluator_fingerprint(),
    }
}

fn observation(
    suite: &RegressionSuite,
    plan: &ComparisonPlan,
    variant: TraceVariant,
) -> TrialObservation {
    let fixture = &suite.fixtures[0];
    let pin = match variant {
        TraceVariant::Baseline => &plan.baseline,
        TraceVariant::Candidate => &plan.candidate,
    };
    let events = vec![
        TraceEventKind::ModelStarted {
            step_id: "step".into(),
        },
        TraceEventKind::Served {
            step_id: "step".into(),
            provider: pin.settings.provider.clone(),
            model: pin.settings.model.clone(),
        },
        TraceEventKind::ModelFinished {
            step_id: "step".into(),
        },
        TraceEventKind::RunFinished {
            success: true,
            output: Some(fixture.oracle.expected_output.clone()),
            error: None,
        },
    ];
    TrialObservation {
        variant_pin: pin.clone(),
        graded: GradedTrace {
            trace: TaskTrace {
                version: TRACE_VERSION,
                settings: pin.settings.clone(),
                fixture_id: fixture.id.clone(),
                repetition: 0,
                variant,
                events: events
                    .into_iter()
                    .enumerate()
                    .map(|(index, event)| TraceEvent {
                        sequence: index as u64 + 1,
                        elapsed_ms: index as u64,
                        event,
                    })
                    .collect(),
            },
            oracle_failures: Vec::new(),
            observed_files: fixture.oracle.expected_files.clone(),
            observed_directories: BTreeSet::new(),
        },
        mock_effects: fixture.oracle.expected_mock_effects.clone(),
        artifact_refs: Vec::new(),
    }
}

fn matrix(suite: &RegressionSuite, plan: &ComparisonPlan) -> Vec<TrialObservation> {
    [TraceVariant::Baseline, TraceVariant::Candidate]
        .into_iter()
        .map(|variant| observation(suite, plan, variant))
        .collect()
}

#[test]
fn changed_model_and_configuration_are_compared_against_their_own_pins() {
    let suite = suite();
    let plan = plan(&suite);
    let trials = matrix(&suite, &plan);
    let result = compare_planned_tasks(&suite, &plan, &trials).unwrap();
    assert!(result.matrix_complete);
    assert_eq!(result.baseline.correct, 1);
    assert_eq!(result.candidate.correct, 1);
    assert_eq!(result.candidate.metrics.model_calls, 1);
    assert_eq!(result.trials.len(), 2);
    // Existing comparator still requires the legacy fixture-set and identical settings.
    assert!(compare_traces(
        &[suite.fixtures[0].task_fixture()],
        1,
        &trials
            .into_iter()
            .map(|trial| trial.graded)
            .collect::<Vec<_>>()
    )
    .is_err());
}

#[test]
fn served_identity_and_settings_substitution_are_rejected() {
    let suite = suite();
    let plan = plan(&suite);
    let mut trials = matrix(&suite, &plan);
    if let TraceEventKind::Served { model, .. } = &mut trials[1].graded.trace.events[1].event {
        *model = "wrong-served-model".into();
    }
    assert!(compare_planned_tasks(&suite, &plan, &trials).is_err());
    let mut trials = matrix(&suite, &plan);
    trials[1].graded.trace.settings = plan.baseline.settings.clone();
    assert!(compare_planned_tasks(&suite, &plan, &trials).is_err());
    let mut trials = matrix(&suite, &plan);
    trials[1].graded.trace.events.remove(1);
    for (index, event) in trials[1].graded.trace.events.iter_mut().enumerate() {
        event.sequence = index as u64 + 1;
    }
    // Legacy metrics permits absent adapter evidence; task comparisons require it.
    assert!(trials[1].graded.trace.metrics().is_ok());
    assert!(compare_planned_tasks(&suite, &plan, &trials).is_err());
}

#[test]
fn full_observed_variant_pin_rejects_source_and_tool_policy_substitution() {
    let suite = suite();
    let plan = plan(&suite);
    let trials = matrix(&suite, &plan);
    for (index, variant) in [TraceVariant::Baseline, TraceVariant::Candidate]
        .into_iter()
        .enumerate()
    {
        for substitute_source in [true, false] {
            let mut changed = plan.clone();
            let planned_pin = match variant {
                TraceVariant::Baseline => &mut changed.baseline,
                TraceVariant::Candidate => &mut changed.candidate,
            };
            if substitute_source {
                planned_pin.source_digest = "c".repeat(64);
            } else {
                planned_pin.tool_policy_digest = "c".repeat(64);
            }
            assert_eq!(planned_pin.settings, trials[index].graded.trace.settings);
            assert!(compare_planned_tasks(&suite, &changed, &trials).is_err());

            let mut changed = trials.clone();
            let observed_pin = &mut changed[index].variant_pin;
            if substitute_source {
                observed_pin.source_digest = "c".repeat(64);
            } else {
                observed_pin.tool_policy_digest = "c".repeat(64);
            }
            assert!(compare_planned_tasks(&suite, &plan, &changed).is_err());
        }
    }
    let mut serialized = serde_json::to_value(&trials[0]).unwrap();
    serialized.as_object_mut().unwrap().remove("variantPin");
    assert!(serde_json::from_value::<TrialObservation>(serialized).is_err());
}

#[test]
fn independent_regrading_detects_corrupted_extra_files_and_forged_verdicts() {
    let suite = suite();
    let plan = plan(&suite);
    let mut trials = matrix(&suite, &plan);
    trials[1]
        .graded
        .observed_files
        .insert("result.txt".into(), "I claim success".into());
    assert_eq!(
        regrade_observation(&suite.fixtures[0].task_fixture(), &trials[1].graded).unwrap(),
        vec!["file missing or differs: result.txt"]
    );
    assert!(compare_planned_tasks(&suite, &plan, &trials).is_err());
    trials[1].graded.oracle_failures = vec!["file missing or differs: result.txt".into()];
    let report = compare_planned_tasks(&suite, &plan, &trials).unwrap();
    assert_eq!(report.candidate.correct, 0);
    trials[1]
        .graded
        .observed_files
        .insert("extra.txt".into(), "unexpected".into());
    trials[1]
        .graded
        .oracle_failures
        .push("unrequested file: extra.txt".into());
    let report = compare_planned_tasks(&suite, &plan, &trials).unwrap();
    assert_eq!(report.candidate.correct, 0);
    assert_eq!(report.trials[1].failures.len(), 2);
    let mut trials = matrix(&suite, &plan);
    trials[0].graded.oracle_failures = vec!["forged failure".into()];
    assert!(compare_planned_tasks(&suite, &plan, &trials).is_err());
}

#[test]
fn hidden_oracle_never_defines_or_serializes_as_actor_capabilities() {
    let mut suite = suite();
    let actor = suite.fixtures[0].actor.clone();
    suite.fixtures[0]
        .oracle
        .expected_files
        .insert("hidden.txt".into(), "hidden oracle canary".into());
    assert!(suite.validate().is_ok());
    assert_eq!(suite.fixtures[0].actor, actor);
    let actor_json = serde_json::to_string(&suite.fixtures[0].actor).unwrap();
    for private in [
        "hidden.txt",
        "hidden oracle canary",
        "private receipt canary",
    ] {
        assert!(!actor_json.contains(private));
    }
    assert!(!suite.fixtures[0]
        .actor
        .writable_files
        .contains(&"hidden.txt".into()));
    assert!(suite.fixtures[0]
        .task_fixture()
        .expected_files
        .contains_key("hidden.txt"));
}

#[test]
fn mock_effects_require_exact_order_count_key_and_payload() {
    let suite = suite();
    let plan = plan(&suite);
    for effects in [
        Vec::new(),
        vec![MockEffect {
            key: "send".into(),
            payload: "wrong".into(),
        }],
        vec![MockEffect {
            key: "pay".into(),
            payload: "private receipt canary".into(),
        }],
        vec![suite.fixtures[0].oracle.expected_mock_effects[0].clone(); 2],
    ] {
        let mut trials = matrix(&suite, &plan);
        trials[1].mock_effects = effects;
        let report = compare_planned_tasks(&suite, &plan, &trials).unwrap();
        assert_eq!(report.candidate.correct, 0);
        assert_eq!(report.trials[1].failures, vec!["mock effects differ"]);
    }
    let mut suite = suite;
    suite.fixtures[0]
        .oracle
        .expected_mock_effects
        .push(MockEffect {
            key: "pay".into(),
            payload: "second effect".into(),
        });
    let plan = self::plan(&suite);
    let mut trials = matrix(&suite, &plan);
    trials[1].mock_effects.reverse();
    assert_eq!(
        compare_planned_tasks(&suite, &plan, &trials)
            .unwrap()
            .candidate
            .correct,
        0
    );
}

#[test]
fn unsafe_windows_paths_and_case_aliases_are_rejected_at_actor_admission() {
    for path in [
        "",
        ".",
        "..",
        "../escape",
        "a/b",
        "a\\b",
        "C:escape",
        "file:stream",
        "x.",
        ".hidden",
        "a..b",
        "NUL",
        "nul.txt",
        "CON",
        "PrN.log",
        "AUX",
        "COM1",
        "com9.txt",
        "LPT1.csv",
        "LPT9",
        "trailing ",
        "é.txt",
        "a?b",
    ] {
        let mut actor = suite().fixtures.remove(0).actor;
        actor.writable_files = vec![path.into()];
        assert!(actor.validate().is_err(), "accepted unsafe path {path:?}");
    }
    let mut actor = suite().fixtures.remove(0).actor;
    actor.writable_files.push("INPUT.TXT".into());
    assert!(actor.validate().is_err());
    let mut actor = suite().fixtures.remove(0).actor;
    actor.writable_files = vec!["COM10.txt".into(), "normal-file_1.txt".into()];
    assert!(actor.validate().is_ok());
}

#[test]
fn incomplete_duplicate_extra_and_out_of_range_matrix_rows_are_rejected() {
    let suite = suite();
    let plan = plan(&suite);
    let trials = matrix(&suite, &plan);
    assert!(compare_planned_tasks(&suite, &plan, &trials[..1]).is_err());
    assert!(compare_planned_tasks(&suite, &plan, &[trials[0].clone(), trials[0].clone()]).is_err());
    let mut extra = trials.clone();
    extra.push(trials[0].clone());
    assert!(compare_planned_tasks(&suite, &plan, &extra).is_err());
    for replacement in ["other", "edit"] {
        let mut invalid = trials.clone();
        invalid[1].graded.trace.fixture_id = replacement.into();
        invalid[1].graded.trace.repetition = 1;
        assert!(compare_planned_tasks(&suite, &plan, &invalid).is_err());
    }
    let mut incomplete = trials;
    incomplete[1].graded.trace.events.pop();
    assert!(compare_planned_tasks(&suite, &plan, &incomplete).is_err());
}

#[test]
fn steering_capture_cannot_become_comparison_evidence() {
    let suite = suite();
    let plan = plan(&suite);
    let capture = anima_core::trace_evaluation::TraceCapture::new(
        plan.baseline.settings,
        "edit",
        0,
        TraceVariant::Baseline,
    );
    capture.on_frame(RunFrame::Steered {
        message_id: "steer".into(),
        text: "change the fixture".into(),
        content: Content::default(),
    });
    assert!(capture
        .finish(&TaskResult::success(Content::default(), 0))
        .is_err());
}

#[test]
fn suite_digest_binds_private_oracle_public_capabilities_and_plan_evaluator() {
    let suite = suite();
    let original = suite.digest().unwrap();
    let mut changed = suite.clone();
    changed.fixtures[0]
        .actor
        .writable_files
        .push("another.txt".into());
    assert_ne!(original, changed.digest().unwrap());
    let mut changed = suite.clone();
    changed.fixtures[0].oracle.expected_output = "different".into();
    assert_ne!(original, changed.digest().unwrap());
    let mut plan = plan(&suite);
    let trials = matrix(&suite, &plan);
    plan.evaluator_digest = "0".repeat(64);
    assert!(compare_planned_tasks(&suite, &plan, &trials).is_err());
}

#[test]
fn identical_pins_are_a_valid_aa_control() {
    let suite = suite();
    let mut plan = plan(&suite);
    plan.candidate = plan.baseline.clone();
    let report = compare_planned_tasks(&suite, &plan, &matrix(&suite, &plan)).unwrap();
    assert_eq!(report.baseline, report.candidate);
}

#[test]
fn declared_repetitions_and_every_fixture_remain_in_the_complete_matrix() {
    let mut suite = suite();
    let mut second = suite.fixtures[0].clone();
    second.id = "second".into();
    suite.fixtures.push(second);
    let mut plan = plan(&suite);
    plan.repetitions = 3;
    let mut trials = Vec::new();
    for fixture in &suite.fixtures {
        for repetition in 0..plan.repetitions {
            for variant in [TraceVariant::Baseline, TraceVariant::Candidate] {
                let mut trial = observation(&suite, &plan, variant);
                trial.graded.trace.fixture_id = fixture.id.clone();
                trial.graded.trace.repetition = repetition;
                trials.push(trial);
            }
        }
    }
    trials.reverse();
    let report = compare_planned_tasks(&suite, &plan, &trials).unwrap();
    assert_eq!(report.baseline.trials, 6);
    assert_eq!(report.candidate.correct, 6);
    assert_eq!(report.trials[0].fixture_id, "edit");
    assert_eq!(report.trials[0].repetition, 0);
    assert_eq!(report.trials[0].variant, TraceVariant::Baseline);
    trials.pop();
    assert!(compare_planned_tasks(&suite, &plan, &trials).is_err());
}

#[test]
fn artifact_fact_references_have_bounded_syntax_and_add_no_quality_verdict() {
    let suite = suite();
    let plan = plan(&suite);
    let mut trials = matrix(&suite, &plan);
    trials[1].artifact_refs.push(ArtifactObservationRef {
        report_sha256: "c".repeat(64),
        observation_locator: "observations/0/page-1".into(),
    });
    let report = compare_planned_tasks(&suite, &plan, &trials).unwrap();
    assert!(report.trials[1].failures.is_empty());
    let serialized = serde_json::to_value(report).unwrap();
    assert!(serialized.get("rendered").is_none());
    for (hash, locator) in [
        ("bad".into(), "observations/0".into()),
        ("c".repeat(64), "".into()),
        ("c".repeat(64), "x".repeat(257)),
        ("c".repeat(64), "line\nbreak".into()),
    ] {
        trials[1].artifact_refs[0] = ArtifactObservationRef {
            report_sha256: hash,
            observation_locator: locator,
        };
        assert!(compare_planned_tasks(&suite, &plan, &trials).is_err());
    }
}

#[test]
fn suite_bounds_and_unknown_actor_fields_fail_closed() {
    let mut suite = suite();
    suite.fixtures[0].actor.prompt = "x".repeat(16 * 1024 + 1);
    assert!(suite.validate().is_err());
    suite.fixtures[0].actor.prompt = "x".into();
    suite.fixtures[0]
        .actor
        .initial_files
        .insert("large.txt".into(), "x".repeat(64 * 1024 + 1));
    assert!(suite.validate().is_err());
    let value = serde_json::json!({
        "prompt": "task", "readableFiles": [], "writableFiles": [],
        "mockEffects": false, "expectedOutput": "hidden"
    });
    assert!(serde_json::from_value::<ActorFixture>(value).is_err());
}

#[test]
fn suite_count_total_bytes_and_per_side_plan_pins_are_bounded() {
    let mut suite = suite();
    suite.fixtures = (0..17)
        .map(|index| {
            let mut fixture = suite.fixtures[0].clone();
            fixture.id = format!("fixture-{index}");
            fixture
        })
        .collect();
    assert!(suite.validate().is_err());
    suite.fixtures.truncate(2);
    for fixture in &mut suite.fixtures {
        fixture.oracle.expected_files = (0..33)
            .map(|index| (format!("file-{index}.txt"), "x".repeat(64 * 1024)))
            .collect();
    }
    assert!(suite.validate().is_err());
    let suite = self::suite();
    let plan = plan(&suite);
    for repetitions in [0, 4, u32::MAX] {
        let mut changed = plan.clone();
        changed.repetitions = repetitions;
        assert!(changed.validate(&suite).is_err());
    }
    for variant in [TraceVariant::Baseline, TraceVariant::Candidate] {
        let mut changed = plan.clone();
        let pin = match variant {
            TraceVariant::Baseline => &mut changed.baseline,
            TraceVariant::Candidate => &mut changed.candidate,
        };
        pin.settings.fixture_set_id = "wrong-suite".into();
        assert!(changed.validate(&suite).is_err());
    }
}
