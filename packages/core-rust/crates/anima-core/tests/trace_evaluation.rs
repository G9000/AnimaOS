use anima_core::trace_evaluation::*;
use anima_core::{Content, RunFrame, RunObserver, TaskResult, TaskStatus, TokenUsage, ToolCall};
use std::collections::BTreeMap;

fn fixtures() -> Vec<TaskFixture> {
    serde_json::from_str(include_str!(
        "../../../acceptance/turn-quality/fixtures.json"
    ))
    .unwrap()
}
fn settings() -> TraceSettings {
    TraceSettings {
        provider: "deterministic".into(),
        model: "fixture-model".into(),
        configuration_id: "temperature=0;maxTokens=128;toolRounds=12".into(),
        fixture_set_id: fixture_set_digest(&fixtures()).unwrap(),
    }
}
fn completed(variant: TraceVariant) -> TaskTrace {
    let capture = TraceCapture::new(settings(), "ambiguous-edit", 0, variant);
    capture.on_frame(RunFrame::StepStarted {
        step_id: "run:1".into(),
    });
    capture.on_frame(RunFrame::StepUsage {
        step_id: "run:1".into(),
        usage: TokenUsage::default(),
    });
    capture.on_frame(RunFrame::StepFinished {
        step_id: "run:1".into(),
        message_id: Some("reply".into()),
    });
    capture
        .finish(&TaskResult::success(
            Content {
                text: "Changed prod only.".into(),
                ..Content::default()
            },
            0,
        ))
        .unwrap()
}
#[test]
fn trace_evaluation_rejects_missing_incomplete_mismatched_and_duplicate_trials() {
    let f = fixtures();
    assert!(compare_traces(&f, 1, &[]).is_err());
    let mut t = completed(TraceVariant::Baseline);
    t.events.pop();
    assert!(t.metrics().is_err());
    let baseline = GradedTrace {
        trace: completed(TraceVariant::Baseline),
        oracle_failures: vec![],
        observed_files: BTreeMap::new(),
        observed_directories: Default::default(),
    };
    let mut candidate = baseline.clone();
    candidate.trace.variant = TraceVariant::Candidate;
    candidate.trace.settings.model = "other".into();
    assert!(compare_traces(&f[..1], 1, &[baseline.clone(), candidate]).is_err());
    assert!(compare_traces(&f[..1], 1, &[baseline.clone(), baseline]).is_err());
}
#[test]
fn trace_evaluation_counts_real_frames_and_preserves_unknown_usage() {
    let capture = TraceCapture::new(settings(), "ambiguous-edit", 0, TraceVariant::Candidate);
    capture.on_frame(RunFrame::StepStarted {
        step_id: "run:1".into(),
    });
    capture.on_frame(RunFrame::StepFinished {
        step_id: "run:1".into(),
        message_id: Some("reply".into()),
    });
    for id in ["a", "b"] {
        capture.on_frame(RunFrame::ToolStarted {
            step_id: "run:1".into(),
            tool_call: ToolCall {
                id: id.into(),
                name: "read".into(),
                args: BTreeMap::new(),
            },
        });
        capture.on_frame(RunFrame::ToolFinished {
            step_id: "run:1".into(),
            tool_call_id: id.into(),
            name: "read".into(),
            status: TaskStatus::Error,
            duration_ms: 1,
            result: "bad".into(),
            recovered: false,
        });
    }
    let t = capture.finish(&TaskResult::error("failed", 0)).unwrap();
    let m = t.metrics().unwrap();
    assert_eq!(
        (
            m.model_calls,
            m.tool_calls,
            m.tool_errors,
            m.exact_repeat_calls,
            m.result_bytes,
            m.run_errors
        ),
        (1, 2, 2, 1, 6, 1)
    );
    assert_eq!(m.unknown_usage_calls, 1);
    assert_eq!(m.known_usage, None);
    assert_eq!(
        completed(TraceVariant::Baseline)
            .metrics()
            .unwrap()
            .known_usage,
        None
    );
}

#[test]
fn trace_evaluation_counts_duplicate_tool_ids_as_distinct_runtime_invocations() {
    let capture = TraceCapture::new(settings(), "ambiguous-edit", 0, TraceVariant::Candidate);
    capture.on_frame(RunFrame::StepStarted {
        step_id: "run:1".into(),
    });
    capture.on_frame(RunFrame::StepFinished {
        step_id: "run:1".into(),
        message_id: Some("reply".into()),
    });
    for _ in 0..2 {
        capture.on_frame(RunFrame::ToolStarted {
            step_id: "run:1".into(),
            tool_call: ToolCall {
                id: "same-id".into(),
                name: "read".into(),
                args: BTreeMap::new(),
            },
        });
    }
    for _ in 0..2 {
        capture.on_frame(RunFrame::ToolFinished {
            step_id: "run:1".into(),
            tool_call_id: "same-id".into(),
            name: "read".into(),
            status: TaskStatus::Success,
            duration_ms: 0,
            result: "ok".into(),
            recovered: false,
        });
    }
    let trace = capture
        .finish(&TaskResult::success(Content::default(), 0))
        .unwrap();
    let metrics = trace.metrics().unwrap();
    assert_eq!(metrics.tool_calls, 2);
    assert_eq!(metrics.exact_repeat_calls, 1);
    let mut lost = trace;
    lost.events.remove(4);
    assert!(lost.metrics().is_err());
}

fn from_events(events: Vec<TraceEventKind>) -> TaskTrace {
    TaskTrace {
        version: TRACE_VERSION,
        settings: settings(),
        fixture_id: "ambiguous-edit".into(),
        repetition: 0,
        variant: TraceVariant::Candidate,
        events: events
            .into_iter()
            .enumerate()
            .map(|(index, event)| TraceEvent {
                sequence: index as u64 + 1,
                elapsed_ms: 0,
                event,
            })
            .collect(),
    }
}
fn model_start(step: &str) -> TraceEventKind {
    TraceEventKind::ModelStarted {
        step_id: step.into(),
    }
}
fn model_finish(step: &str) -> TraceEventKind {
    TraceEventKind::ModelFinished {
        step_id: step.into(),
    }
}
fn tool_start(step: &str) -> TraceEventKind {
    TraceEventKind::ToolStarted {
        step_id: step.into(),
        call_id: "call".into(),
        name: "read".into(),
        arguments: "{}".into(),
    }
}
fn tool_finish(step: &str) -> TraceEventKind {
    TraceEventKind::ToolFinished {
        step_id: step.into(),
        call_id: "call".into(),
        name: "read".into(),
        success: true,
        result: "ok".into(),
        recovered: false,
    }
}
fn terminal() -> TraceEventKind {
    TraceEventKind::RunFinished {
        success: true,
        output: Some("done".into()),
        error: None,
    }
}
fn served(step: &str) -> TraceEventKind {
    TraceEventKind::Served {
        step_id: step.into(),
        provider: "deterministic".into(),
        model: "fixture-model".into(),
    }
}

#[test]
fn trace_evaluation_rejects_tools_outside_current_finished_model_phase() {
    assert!(from_events(vec![
        model_start("1"),
        model_finish("1"),
        tool_start("1"),
        tool_finish("1"),
        model_start("2"),
        model_finish("2"),
        terminal()
    ])
    .metrics()
    .is_ok());
    for events in [
        vec![
            model_start("1"),
            tool_start("1"),
            tool_finish("1"),
            model_finish("1"),
            terminal(),
        ],
        vec![
            model_start("1"),
            model_finish("1"),
            model_start("2"),
            tool_start("1"),
            tool_finish("1"),
            model_finish("2"),
            terminal(),
        ],
        vec![
            model_start("1"),
            model_finish("1"),
            model_start("2"),
            model_finish("2"),
            tool_start("1"),
            tool_finish("1"),
            terminal(),
        ],
        vec![
            model_start("1"),
            model_finish("1"),
            tool_start("1"),
            model_start("2"),
            tool_finish("1"),
            model_finish("2"),
            terminal(),
        ],
    ] {
        assert!(from_events(events).metrics().is_err(), "tools must follow completion of their current model step and settle before the next starts");
    }
}

#[test]
fn trace_evaluation_rejects_late_duplicate_and_historical_served_events() {
    let usage = || TraceEventKind::Usage {
        step_id: "1".into(),
        usage: TokenUsage::default(),
    };
    assert!(from_events(vec![
        model_start("1"),
        served("1"),
        usage(),
        model_finish("1"),
        terminal()
    ])
    .metrics()
    .is_ok());
    for events in [
        vec![
            model_start("1"),
            served("1"),
            served("1"),
            model_finish("1"),
            terminal(),
        ],
        vec![
            model_start("1"),
            usage(),
            served("1"),
            model_finish("1"),
            terminal(),
        ],
        vec![model_start("1"), model_finish("1"), served("1"), terminal()],
        vec![
            model_start("1"),
            model_finish("1"),
            model_start("2"),
            served("1"),
            model_finish("2"),
            terminal(),
        ],
    ] {
        assert!(
            from_events(events).metrics().is_err(),
            "served identity must occur once while its current model is active, before usage"
        );
    }
}

#[test]
fn trace_evaluation_fixture_output_contract_is_visible_without_answer_leakage() {
    for fixture in fixtures() {
        match fixture.id.as_str() {
            "large-output" => {
                assert!(fixture.prompt.contains("last record identifier"));
                assert!(!fixture.prompt.contains(&fixture.expected_output));
                assert!(fixture.prompt.contains("reply only"));
            }
            "fresh-skill-reuse" => {
                assert!(fixture.initial_files["SKILL.txt"].contains("output only the mapped word"))
            }
            _ => assert!(
                fixture
                    .prompt
                    .contains(&format!("reply exactly: {}", fixture.expected_output)),
                "fixed acknowledgement must be an explicit task requirement: {}",
                fixture.id
            ),
        }
    }
}

use anima_core::{
    AgentConfig, AgentRuntime, DataValue, ModelAdapter, ModelGenerateRequest,
    ModelGenerateResponse, ModelStopReason, ToolDescriptor,
};
use async_trait::async_trait;
use std::path::Path;
use std::sync::{Arc, Mutex};

struct ScriptedModel {
    calls: Mutex<std::collections::VecDeque<ToolCall>>,
    output: String,
}
#[async_trait]
impl ModelAdapter for ScriptedModel {
    fn provider(&self) -> &str {
        "deterministic"
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        _: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let call = self.calls.lock().unwrap().pop_front();
        let is_tool = call.is_some();
        Ok(ModelGenerateResponse {
            content: Content {
                text: if is_tool {
                    String::new()
                } else {
                    self.output.clone()
                },
                ..Content::default()
            },
            tool_calls: call.map(|c| vec![c]),
            usage: TokenUsage {
                prompt_tokens: 2,
                completion_tokens: 1,
                total_tokens: 3,
                ..TokenUsage::default()
            },
            stop_reason: if is_tool {
                ModelStopReason::ToolCall
            } else {
                ModelStopReason::End
            },
        })
    }
}
fn call(name: &str, pairs: &[(&str, &str)]) -> ToolCall {
    ToolCall {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        args: pairs
            .iter()
            .map(|(k, v)| ((*k).into(), DataValue::String((*v).into())))
            .collect(),
    }
}
fn script(id: &str, candidate: bool) -> Vec<ToolCall> {
    let read = |path: &str| call("read", &[("path", path)]);
    let replace = |path: &str, old: &str, new: &str| {
        call("replace", &[("path", path), ("old", old), ("new", new)])
    };
    let write = |path: &str, value: &str| call("write", &[("path", path), ("value", value)]);
    match id {
        "ambiguous-edit" => {
            let mut v = vec![read("config.txt"), replace("config.txt", "red", "blue")];
            if candidate {
                v.push(replace("config.txt", "prod.color=red", "prod.color=blue"));
            }
            v
        }
        "stale-read" => {
            let mut v = vec![
                read("config.txt"),
                replace(
                    "config.txt",
                    "version=1\nowner=old\n",
                    "version=2\nowner=old\n",
                ),
            ];
            if candidate {
                v.extend([
                    read("config.txt"),
                    replace("config.txt", "version=1", "version=2"),
                ]);
            }
            v
        }
        "large-output" => {
            let mut v = vec![read("log.txt")];
            if candidate {
                v.push(write("answer.txt", "record-4096"));
            }
            v
        }
        "repeated-failure" => {
            let mut v = vec![call("unavailable", &[]), call("unavailable", &[])];
            if candidate {
                v.push(replace("config.txt", "false", "true"));
            }
            v
        }
        "unrelated-green-check" => {
            let mut v = vec![call("check", &[("target", "smoke")])];
            if candidate {
                v.extend([
                    replace("target.txt", "broken", "fixed"),
                    call("check", &[("target", "target")]),
                ]);
            }
            v
        }
        "fresh-skill-reuse" => {
            let mut v = vec![read("SKILL.txt"), read("input.txt")];
            if candidate {
                v.push(write("answer.txt", "omega"));
            }
            v
        }
        _ => panic!("unknown fixture"),
    }
}
fn native_like_tool(
    root: &Path,
    fixture: &str,
    call: ToolCall,
    stale_injected: &Mutex<bool>,
) -> TaskResult<Content> {
    let get = |key: &str| match call.args.get(key) {
        Some(DataValue::String(v)) => v.clone(),
        _ => String::new(),
    };
    let path = get("path");
    let run = || -> Result<String, String> {
        if path.contains('/') || path.contains('\\') || path.contains(':') || path == ".." {
            return Err("invalid fixture path".into());
        }
        match call.name.as_str() {
            "read" => {
                let value = std::fs::read_to_string(root.join(&path)).map_err(|e| e.to_string())?;
                if fixture == "stale-read" && path == "config.txt" {
                    let mut injected = stale_injected.lock().unwrap();
                    if !*injected {
                        std::fs::write(root.join(&path), "version=1\nowner=new\n").unwrap();
                        *injected = true;
                    }
                }
                Ok(value)
            }
            "replace" => {
                let value = std::fs::read_to_string(root.join(&path)).map_err(|e| e.to_string())?;
                let old = get("old");
                if old.is_empty() || value.matches(&old).count() != 1 {
                    return Err("expected exactly one current match".into());
                }
                std::fs::write(root.join(&path), value.replacen(&old, &get("new"), 1))
                    .map_err(|e| e.to_string())?;
                Ok("changed".into())
            }
            "write" => {
                std::fs::write(root.join(&path), get("value")).map_err(|e| e.to_string())?;
                Ok("written".into())
            }
            "check" if get("target") == "smoke" => Ok("smoke:pass".into()),
            "check" => {
                if std::fs::read_to_string(root.join("target.txt")).unwrap() != "fixed\n" {
                    return Err("target:fail".into());
                }
                std::fs::write(root.join("target-check.txt"), "target:pass").unwrap();
                Ok("target:pass".into())
            }
            "unavailable" => Err("helper unavailable".into()),
            _ => Err("tool not permitted".into()),
        }
    };
    match run() {
        Ok(text) => TaskResult::success(
            Content {
                text,
                ..Content::default()
            },
            0,
        ),
        Err(e) => TaskResult::error(e, 0),
    }
}
struct Workspace(std::path::PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("anima-trace-eval-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn trace_evaluation_ordinary_runtime_battery_and_mutation_oracles() {
    let fixtures = fixtures();
    let mut grades = Vec::new();
    for f in &fixtures {
        for repetition in 0..2 {
            for variant in [TraceVariant::Baseline, TraceVariant::Candidate] {
                let candidate = variant == TraceVariant::Candidate;
                let workspace = Workspace::new();
                for (path, text) in &f.initial_files {
                    std::fs::write(workspace.0.join(path), text).unwrap();
                }
                let tools = ["read", "replace", "write", "check", "unavailable"]
                    .iter()
                    .map(|name| ToolDescriptor {
                        name: (*name).into(),
                        description: "permitted isolated test tool".into(),
                        parameters_schema: BTreeMap::new(),
                        examples: None,
                    })
                    .collect::<Vec<_>>();
                let config: AgentConfig = serde_json::from_value(serde_json::json!({"name":"trace-fixture","model":"fixture-model","provider":"deterministic","tools":tools,"settings":{"temperature":0.0,"max_tokens":128,"max_tool_iterations":12,"max_retries":0,"additional":{}}})).unwrap();
                let model = Arc::new(ScriptedModel {
                    calls: Mutex::new(script(&f.id, candidate).into()),
                    output: if candidate {
                        f.expected_output.clone()
                    } else {
                        "Done.".into()
                    },
                });
                let mut runtime = AgentRuntime::new(config, model);
                runtime.set_default_tool_iterations(12);
                runtime.set_run_id(format!("{}-{repetition}-{variant:?}", f.id));
                let experiment = TraceSettings::from_config(
                    runtime.config(),
                    fixture_set_digest(&fixtures).unwrap(),
                )
                .unwrap();
                let capture = Arc::new(TraceCapture::new(experiment, &f.id, repetition, variant));
                runtime.set_run_observer(capture.clone());
                let stale = Mutex::new(false);
                let result = futures::executor::block_on(runtime.run_with_tools(
                    Content {
                        text: f.prompt.clone(),
                        ..Content::default()
                    },
                    |_, _, call| {
                        std::future::ready(native_like_tool(&workspace.0, &f.id, call, &stale))
                    },
                ));
                assert_eq!(
                    result.status,
                    TaskStatus::Success,
                    "{}: {:?}",
                    f.id,
                    result.error
                );
                let trace = capture.finish(&result).unwrap();
                let metrics = trace.metrics().unwrap();
                assert!(metrics.model_calls > 1);
                assert!(metrics.tool_calls > 0);
                assert_eq!(metrics.unknown_usage_calls, 0);
                if f.id == "large-output" {
                    assert!(metrics.result_bytes > 32_000);
                }
                if f.id == "repeated-failure" {
                    assert_eq!(metrics.exact_repeat_calls, 1);
                    assert_eq!(metrics.tool_errors, 2);
                }
                let grade = grade_fixture(f, trace.clone(), &workspace.0).unwrap();
                assert_eq!(
                    grade.oracle_failures.is_empty(),
                    candidate,
                    "{} {:?}",
                    f.id,
                    grade.oracle_failures
                );
                if candidate {
                    // A correct spoken claim cannot hide an incorrect result or an extra mutation.
                    let first = f.expected_files.keys().next().unwrap();
                    std::fs::write(workspace.0.join(first), "corrupted").unwrap();
                    assert!(!grade_fixture(f, trace.clone(), &workspace.0)
                        .unwrap()
                        .oracle_failures
                        .is_empty());
                    std::fs::write(workspace.0.join(first), &f.expected_files[first]).unwrap();
                    std::fs::write(workspace.0.join("unrequested.txt"), "mutation").unwrap();
                    assert!(!grade_fixture(f, trace, &workspace.0)
                        .unwrap()
                        .oracle_failures
                        .is_empty());
                }
                grades.push(grade);
            }
        }
    }
    let report = compare_traces(&fixtures, 2, &grades).unwrap();
    assert_eq!(report.baseline.correct, 0);
    assert_eq!(report.candidate.correct, 12);
    assert_eq!(report.baseline.trials, 12);
    let encoded = serde_json::to_string(&report).unwrap();
    assert!(!encoded.contains("promotion"));
    assert!(!encoded.contains("grant"));
    let mut forged = grades.clone();
    forged[0].oracle_failures.clear();
    assert!(
        compare_traces(&fixtures, 2, &forged).is_err(),
        "a serialized verdict cannot override recorded filesystem/output observations"
    );
    let mut mismatched = grades.clone();
    mismatched[0].trace.settings.model = "other".into();
    assert!(compare_traces(&fixtures, 2, &mismatched).is_err());
    let mut missing = grades.clone();
    missing.pop();
    assert!(compare_traces(&fixtures, 2, &missing).is_err());
    let mut duplicate = grades.clone();
    duplicate[1] = duplicate[0].clone();
    assert!(compare_traces(&fixtures, 2, &duplicate).is_err());
    if let Some(path) = std::env::var_os("ANIMA_TRACE_EVALUATION_OUTPUT_DIR") {
        let path = std::path::PathBuf::from(path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("deterministic-runtime-traces.json"),
            serde_json::to_vec_pretty(&grades).unwrap(),
        )
        .unwrap();
        std::fs::write(
            path.join("deterministic-runtime-report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn trace_evaluation_refuses_unfinished_capture_and_unsafe_oracles() {
    let capture = TraceCapture::new(settings(), "ambiguous-edit", 0, TraceVariant::Baseline);
    assert!(capture
        .finish(&TaskResult::success(Content::default(), 0))
        .is_err());
    let mut f = fixtures().remove(0);
    f.expected_files
        .insert("../outside.txt".into(), "escape".into());
    let w = Workspace::new();
    assert!(grade_fixture(&f, completed(TraceVariant::Baseline), &w.0).is_err());
}
