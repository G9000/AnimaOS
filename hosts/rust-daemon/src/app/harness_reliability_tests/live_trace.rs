//! Ignored one-trial worker; the explicit Node driver owns the complete matrix.
use anima_core::execution_limits::{ExecutionContext, ExecutionLimits};
use anima_core::trace_evaluation::{
    compare_traces, fixture_set_digest, grade_fixture, GradedTrace, TaskFixture, TraceCapture,
    TraceSettings, TraceVariant,
};
use anima_core::{
    AgentConfig, AgentRuntime, Content, DataValue, TaskResult, ToolCall, ToolDescriptor,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn fixtures() -> Vec<TaskFixture> {
    serde_json::from_str(include_str!(
        "../../../../../packages/core-rust/acceptance/turn-quality/fixtures.json"
    ))
    .unwrap()
}

fn policy() -> Value {
    json!({"tools":"isolated-fixture-allowlist-v1","context":"fresh-runtime-and-workspace-per-trial","maxToolIterations":12,"maxModelAttempts":16,"maxToolCalls":48,"maxRetries":0,"maxInflightOperations":4,"temperature":0,"maxOutputTokensPerStep":2048,"maxInputTokens":4000000,"maxOutputTokens":262144,"maxTotalTokens":4262144,"executionDeadlineMs":40000})
}

fn descriptor(name: &str, description: &str, fields: &[&str]) -> ToolDescriptor {
    let properties = fields
        .iter()
        .map(|field| {
            (
                (*field).to_owned(),
                DataValue::Object(BTreeMap::from([(
                    "type".into(),
                    DataValue::String("string".into()),
                )])),
            )
        })
        .collect();
    ToolDescriptor {
        name: name.into(),
        description: description.into(),
        parameters_schema: BTreeMap::from([
            ("type".into(), DataValue::String("object".into())),
            ("properties".into(), DataValue::Object(properties)),
            (
                "required".into(),
                DataValue::Array(
                    fields
                        .iter()
                        .map(|field| DataValue::String((*field).into()))
                        .collect(),
                ),
            ),
            ("additionalProperties".into(), DataValue::Bool(false)),
        ]),
        examples: None,
    }
}

fn execute(
    root: &Path,
    fixture: &TaskFixture,
    call: ToolCall,
    stale_injected: &Mutex<bool>,
) -> TaskResult<Content> {
    let get = |key: &str| match call.args.get(key) {
        Some(DataValue::String(value)) => Ok(value.clone()),
        _ => Err("required string argument missing".to_owned()),
    };
    let operation = || -> Result<String, String> {
        if call.name == "unavailable_helper" {
            return Err("fixture helper is unavailable; use the permitted file tools".into());
        }
        if call.name == "check" {
            if call.args.len() != 1 {
                return Err("unexpected check arguments".into());
            }
            return match get("target")?.as_str() {
                "smoke" if fixture.id == "unrelated-green-check" => Ok("smoke:pass".into()),
                "target" if fixture.id == "unrelated-green-check" => {
                    if fs::read_to_string(root.join("target.txt"))
                        .map_err(|_| "target read failed")?
                        != "fixed\n"
                    {
                        return Err("target:fail".into());
                    }
                    fs::write(root.join("target-check.txt"), "target:pass")
                        .map_err(|_| "target check write failed")?;
                    Ok("target:pass".into())
                }
                _ => Err("check outside fixture permission".into()),
            };
        }
        let path = get("path")?;
        if path.contains(['/', '\\', ':', '\0'])
            || path == "."
            || path == ".."
            || (!fixture.initial_files.contains_key(&path)
                && !fixture.expected_files.contains_key(&path))
        {
            return Err("path outside fixture permission".into());
        }
        match call.name.as_str() {
            "read_file" if call.args.len() == 1 => {
                let value = fs::read_to_string(root.join(&path))
                    .map_err(|_| "fixture file is unavailable")?;
                if fixture.id == "stale-read" && path == "config.txt" {
                    let mut injected = stale_injected.lock().unwrap();
                    if !*injected {
                        fs::write(root.join(&path), "version=1\nowner=new\n")
                            .map_err(|_| "concurrent fixture edit failed")?;
                        *injected = true;
                    }
                }
                Ok(value)
            }
            "replace_file" if call.args.len() == 3 => {
                let old = get("old")?;
                let new = get("new")?;
                if old.is_empty() || old.len() > 65536 || new.len() > 65536 {
                    return Err("replacement is empty or too large".into());
                }
                let value =
                    fs::read_to_string(root.join(&path)).map_err(|_| "fixture read failed")?;
                if value.matches(&old).count() != 1 {
                    return Err("replacement requires exactly one current match; read again and disambiguate".into());
                }
                fs::write(root.join(path), value.replacen(&old, &new, 1))
                    .map_err(|_| "replacement failed")?;
                Ok("changed".into())
            }
            "write_file" if call.args.len() == 2 && path == "answer.txt" => {
                let content = get("content")?;
                if content.len() > 4096 {
                    return Err("answer exceeds fixture bound".into());
                }
                fs::write(root.join(path), content).map_err(|_| "answer write failed")?;
                Ok("written".into())
            }
            _ => Err("tool or arguments outside fixture permission".into()),
        }
    };
    match operation() {
        Ok(text) => TaskResult::success(
            Content {
                text,
                ..Content::default()
            },
            0,
        ),
        Err(error) => TaskResult::error(error, 0),
    }
}

#[tokio::test]
#[ignore = "explicit one-trial live worker; use the bounded matrix driver"]
async fn trace_live_worker() {
    assert_eq!(std::env::var("ANIMA_RUN_TRACE_LIVE").as_deref(), Ok("1"));
    let credential = zeroize::Zeroizing::new(
        std::env::var("ANTHROPIC_API_KEY").expect("explicit credential required"),
    );
    assert!(!credential.trim().is_empty());
    assert!(
        std::env::var("ANTHROPIC_BASE_URL").map_or(true, |url| matches!(
            url.as_str(),
            "https://api.anthropic.com" | "https://api.anthropic.com/"
        ))
    );
    let fixture_id = std::env::var("ANIMA_TRACE_FIXTURE").unwrap();
    let repetition: u32 = std::env::var("ANIMA_TRACE_REPETITION")
        .unwrap()
        .parse()
        .unwrap();
    assert!(repetition < 5);
    let variant = match std::env::var("ANIMA_TRACE_VARIANT").unwrap().as_str() {
        "baseline" => TraceVariant::Baseline,
        "candidate" => TraceVariant::Candidate,
        _ => panic!("explicit variant required"),
    };
    let model = std::env::var("ANIMA_TRACE_MODEL").expect("explicit matched model");
    assert!(!model.trim().is_empty() && model.len() <= 256);
    let output = std::path::PathBuf::from(std::env::var_os("ANIMA_TRACE_TRIAL_OUTPUT").unwrap());
    assert!(output.is_absolute() && output.is_dir());
    let fixtures = fixtures();
    let fixture = fixtures
        .iter()
        .find(|fixture| fixture.id == fixture_id)
        .unwrap();
    let workspace = tempfile::tempdir().unwrap();
    for (path, value) in &fixture.initial_files {
        assert!(!path.contains(['/', '\\', ':']) && path != ".." && path != ".");
        fs::write(workspace.path().join(path), value).unwrap();
    }
    let tools = vec![
        descriptor("read_file", "Read a permitted task file in full. Read again after any stale or ambiguous replacement error.", &["path"]),
        descriptor("replace_file", "Replace exactly one current occurrence of old with new in a permitted file. Ambiguous or stale old text is rejected.", &["path", "old", "new"]),
        descriptor("write_file", "Write the answer verbatim to answer.txt only.", &["path", "content"]),
        descriptor("check", "Run the fixture's smoke or target check. A successful target check writes its result artifact.", &["target"]),
        descriptor("unavailable_helper", "Attempt an optional fixture helper; an unavailable helper should not be repeated indefinitely.", &[]),
    ];
    let config: AgentConfig = serde_json::from_value(json!({
        "name":"matched-live-fixture","provider":"anthropic","model":model,
        "system":"Use the permitted tools to complete the user's isolated task. Preserve unrelated file contents and owner edits. Read task input files instead of guessing their contents. When the task asks for a derived answer, save it to answer.txt and reply with exactly that saved text. Return the requested final text without Markdown or added explanation.",
        "tools":tools,"fallbacks":[],"settings":{"temperature":0.0,"max_tokens":2048,"max_retries":0,"timeout_ms":30000,"max_tool_iterations":12,"additional":{}}
    })).unwrap();
    let mut settings =
        TraceSettings::from_config(&config, fixture_set_digest(&fixtures).unwrap()).unwrap();
    // Include the host policy/budget/context treatment omitted from AgentConfig.
    settings.configuration_id = hex::encode(Sha256::digest(
        serde_json::to_vec(
            &json!({"agentConfigId":settings.configuration_id,"hostPolicy":policy()}),
        )
        .unwrap(),
    ));
    let adapter = Arc::new(crate::runtime_model::RuntimeModelAdapter::from_env(
        crate::chatgpt_auth::ChatGptAuth::new(),
    ));
    let mut runtime = AgentRuntime::new(config, adapter);
    let mut limits = ExecutionLimits {
        max_model_attempts: 16,
        max_tool_calls: 48,
        max_retries: 0,
        max_inflight_operations: 4,
        ..ExecutionLimits::default()
    };
    limits.budget.max_wall_time_ms = Some(40000);
    limits.budget.max_input_tokens = Some(4_000_000);
    limits.budget.max_output_tokens = Some(262_144);
    limits.budget.max_total_tokens = Some(4_262_144);
    let context = ExecutionContext::ephemeral(
        format!("trace-{fixture_id}-{repetition}-{variant:?}"),
        limits,
        Default::default(),
    )
    .unwrap();
    runtime.set_execution_context(context).unwrap();
    runtime.set_default_tool_iterations(12);
    let capture = Arc::new(TraceCapture::new(settings, fixture_id, repetition, variant));
    runtime.set_run_observer(capture.clone());
    let stale = Mutex::new(false);
    // Provider and tool effects are confined to this disposable workspace. A
    // timeout produces no complete grade; the parent stops rather than retries.
    let result = tokio::time::timeout(
        Duration::from_secs(40),
        runtime.run_with_tools(
            Content {
                text: fixture.prompt.clone(),
                ..Content::default()
            },
            |_, _, call| std::future::ready(execute(workspace.path(), fixture, call, &stale)),
        ),
    )
    .await
    .expect("trial deadline reached; delivery/usage uncertain; do not retry");
    let trace = capture.finish(&result).unwrap();
    let graded = grade_fixture(fixture, trace, workspace.path()).unwrap();
    let artifact = json!({"traceComplete":true,"fixtureFileSha256":hex::encode(Sha256::digest(include_bytes!("../../../../../packages/core-rust/acceptance/turn-quality/fixtures.json"))),"hostPolicy":policy(),"graded":graded});
    let encoded = serde_json::to_string_pretty(&artifact)
        .unwrap()
        .replace(credential.as_str(), "[REDACTED]");
    fs::write(output.join("grade.json"), encoded).unwrap();
    // Wrong answers remain completed observations for the independent comparator.
    println!("TRACE_TRIAL_RECORDED");
}

#[test]
#[ignore = "matrix validator invoked by the explicit live driver"]
fn trace_live_compare() {
    assert_eq!(std::env::var("ANIMA_RUN_TRACE_LIVE").as_deref(), Ok("1"));
    let root = std::path::PathBuf::from(std::env::var_os("ANIMA_TRACE_MATRIX").unwrap());
    let repetitions: u32 = std::env::var("ANIMA_TRACE_REPETITIONS")
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=5).contains(&repetitions));
    let fixtures = fixtures();
    let mut grades: Vec<GradedTrace> = vec![];
    for fixture in &fixtures {
        for repetition in 0..repetitions {
            for variant in ["baseline", "candidate"] {
                let path = root.join(format!("{}-{repetition}-{variant}/grade.json", fixture.id));
                let artifact: Value = serde_json::from_slice(
                    &fs::read(path).expect("every planned matrix row is required"),
                )
                .unwrap();
                assert_eq!(artifact["traceComplete"], true);
                assert_eq!(artifact["hostPolicy"], policy());
                grades.push(serde_json::from_value(artifact["graded"].clone()).unwrap());
            }
        }
    }
    let report = compare_traces(&fixtures, repetitions, &grades)
        .expect("complete matched traces and independent oracle regrading required");
    fs::write(
        root.join("comparison.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("TRACE_MATRIX_VALIDATED");
}
