//! Explicit, narrow provider evidence. Never part of the default test run.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anima_core::trace_evaluation::{
    fixture_set_digest, grade_fixture, TaskFixture, TraceCapture, TraceSettings, TraceVariant,
};
use anima_core::{
    AgentConfig, AgentRuntime, Content, DataValue, TaskResult, ToolCall, ToolDescriptor,
};

fn descriptor(name: &str, description: &str, paths: &[&str], writable: bool) -> ToolDescriptor {
    let string = |s: &str| DataValue::String(s.to_owned());
    let mut properties = BTreeMap::from([(
        "path".into(),
        DataValue::Object(BTreeMap::from([
            ("type".into(), string("string")),
            (
                "enum".into(),
                DataValue::Array(paths.iter().map(|p| string(p)).collect()),
            ),
        ])),
    )]);
    let mut required = vec![string("path")];
    if writable {
        properties.insert(
            "content".into(),
            DataValue::Object(BTreeMap::from([("type".into(), string("string"))])),
        );
        required.push(string("content"));
    }
    ToolDescriptor {
        name: name.into(),
        description: description.into(),
        parameters_schema: BTreeMap::from([
            ("type".into(), string("object")),
            ("properties".into(), DataValue::Object(properties)),
            ("required".into(), DataValue::Array(required)),
            ("additionalProperties".into(), DataValue::Bool(false)),
        ]),
        examples: None,
    }
}

/// Explicit allowlist before joining paths. There is no general filesystem,
/// shell, network, owner-profile or provider-credential tool in this test.
fn execute(
    root: &std::path::Path,
    call: ToolCall,
    audit: &Mutex<Vec<(String, String)>>,
) -> TaskResult<Content> {
    let get = |key: &str| match call.args.get(key) {
        Some(DataValue::String(value)) => Some(value.as_str()),
        _ => None,
    };
    let Some(path) = get("path") else {
        return TaskResult::error("required path missing", 0);
    };
    let operation = match (call.name.as_str(), path) {
        ("read_file", "SKILL.txt" | "input.txt") if call.args.len() == 1 => {
            std::fs::read_to_string(root.join(path)).map_err(|_| "fixture read failed")
        }
        ("write_file", "answer.txt") if call.args.len() == 2 => {
            let Some(content) = get("content").filter(|s| s.len() <= 4096) else {
                return TaskResult::error("required bounded content missing", 0);
            };
            std::fs::write(root.join(path), content)
                .map(|()| "written".to_owned())
                .map_err(|_| "fixture write failed")
        }
        _ => Err("tool or path outside fixture permission"),
    };
    match operation {
        Ok(text) => {
            audit
                .lock()
                .expect("fixture audit lock")
                .push((call.name, path.to_owned()));
            TaskResult::success(
                Content {
                    text,
                    ..Content::default()
                },
                0,
            )
        }
        Err(error) => TaskResult::error(error, 0),
    }
}

#[tokio::test]
#[ignore = "requires ANIMA_RUN_TRACE_LIVE=1 and explicit ANTHROPIC_API_KEY; one bounded provider task"]
async fn trace_evaluation_live_fresh_skill_reuse() {
    assert!(
        std::env::var("ANIMA_RUN_TRACE_LIVE").is_ok_and(|v| v == "1"),
        "explicit ANIMA_RUN_TRACE_LIVE=1 opt-in required"
    );
    // Read only the explicitly forwarded environment credential; never .env,
    // an owner profile, keychain content, or a credential-bearing debug value.
    let credential = zeroize::Zeroizing::new(
        std::env::var("ANTHROPIC_API_KEY")
            .unwrap_or_else(|_| panic!("explicit UTF-8 ANTHROPIC_API_KEY required")),
    );
    assert!(!credential.trim().is_empty(), "nonempty API key required");
    assert!(
        std::env::var("ANTHROPIC_BASE_URL").map_or(true, |url| {
            matches!(
                url.as_str(),
                "https://api.anthropic.com" | "https://api.anthropic.com/"
            )
        }),
        "live test only permits the canonical Anthropic endpoint"
    );
    let fixtures: Vec<TaskFixture> = serde_json::from_str(include_str!(
        "../../../packages/core-rust/acceptance/turn-quality/fixtures.json"
    ))
    .expect("task fixture JSON");
    let fixture = fixtures
        .into_iter()
        .find(|f| f.id == "fresh-skill-reuse")
        .expect("fresh skill fixture");
    let workspace = tempfile::TempDir::new().expect("isolated fixture directory");
    for (path, content) in &fixture.initial_files {
        assert!(matches!(path.as_str(), "SKILL.txt" | "input.txt"));
        std::fs::write(workspace.path().join(path), content).expect("write fixture setup");
    }
    let tools = vec![
        descriptor(
            "read_file",
            "Read a permitted task input file.",
            &["SKILL.txt", "input.txt"],
            false,
        ),
        descriptor(
            "write_file",
            "Write the task answer verbatim to answer.txt.",
            &["answer.txt"],
            true,
        ),
    ];
    let config: AgentConfig = serde_json::from_value(serde_json::json!({
        "name":"isolated-live-trace", "provider":"anthropic", "model":"claude-sonnet-4-6",
        "system":"Use the permitted tools to perform the task. After writing answer.txt, your final reply must be exactly the literal text you saved. Return plain text only: no Markdown, bolding, quotation marks, explanation, prefix or suffix.",
        "tools":tools, "fallbacks":[],
        "settings":{"temperature":0.0,"max_tokens":512,"max_retries":0,
                    "timeout_ms":30000,"max_tool_iterations":6,"additional":{}}
    }))
    .expect("explicit bounded agent config");
    let settings = TraceSettings::from_config(
        &config,
        fixture_set_digest(std::slice::from_ref(&fixture)).expect("fixture digest"),
    )
    .expect("actual configuration identity");
    let adapter = Arc::new(crate::runtime_model::RuntimeModelAdapter::from_env(
        crate::chatgpt_auth::ChatGptAuth::new(),
    ));
    let mut runtime = AgentRuntime::new(config, adapter);
    runtime.set_default_tool_iterations(6);
    runtime.set_run_id("trace-live-fresh-skill-0");
    let capture = Arc::new(TraceCapture::new(
        settings,
        &fixture.id,
        0,
        TraceVariant::Candidate,
    ));
    runtime.set_run_observer(capture.clone());
    let audit = Mutex::new(Vec::new());
    // The expected word/file contents never enter the request or tool schemas.
    // The model must learn the mapping through the actual fixture read tools.
    let input = Content {
        text: format!(
            "{} Save the mapped word to answer.txt, then reply with exactly the saved word.",
            fixture.prompt
        ),
        ..Content::default()
    };
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        runtime.run_with_tools(input, |_, _, call| {
            std::future::ready(execute(workspace.path(), call, &audit))
        }),
    )
    .await
    .expect("live task exceeded its total 30-second bound");
    let trace = capture
        .finish(&result)
        .expect("complete ordinary runtime trace");
    let graded = grade_fixture(&fixture, trace, workspace.path()).expect("independent task oracle");
    // Archive before asserting correctness, preserving failures for diagnosis.
    // The synthetic workspace has no secrets; also remove any credential echo
    // from external error text before serializing an optional evidence artifact.
    if let Some(output) = std::env::var_os("ANIMA_TRACE_EVALUATION_OUTPUT_DIR") {
        let output = std::path::PathBuf::from(output);
        std::fs::create_dir_all(&output).expect("isolated evidence directory");
        let encoded = serde_json::to_string_pretty(&graded)
            .expect("graded trace serialization")
            .replace(credential.as_str(), "[REDACTED]");
        std::fs::write(output.join("anthropic-fresh-skill-trace.json"), encoded)
            .expect("sanitized trace evidence");
    }
    assert!(
        graded.oracle_failures.is_empty(),
        "independent fixture oracle failed"
    );
    let audit = audit.lock().expect("fixture audit lock");
    let write = audit
        .iter()
        .position(|(name, path)| name == "write_file" && path == "answer.txt")
        .expect("actual answer write required");
    for required in ["SKILL.txt", "input.txt"] {
        assert!(
            audit[..write]
                .iter()
                .any(|(name, path)| name == "read_file" && path == required),
            "both actual input reads must precede the answer write"
        );
    }
    let metrics = graded.trace.metrics().expect("validated live metrics");
    assert!(metrics.tool_calls >= 3);
    assert!(metrics.model_calls <= 7);
    assert_eq!(metrics.recovered_tool_results, 0);
    // No raw provider error, model text, tool payload, endpoint or credential.
    println!(
        "TRACE_LIVE_ACCEPTED {}",
        serde_json::json!({
            "fixture":"fresh-skill-reuse", "provider":"anthropic", "model":"claude-sonnet-4-6",
            "modelCalls":metrics.model_calls,"toolCalls":metrics.tool_calls,"toolErrors":metrics.tool_errors,
            "resultBytes":metrics.result_bytes,"knownUsageCalls":metrics.known_usage_calls,
            "unknownUsageCalls":metrics.unknown_usage_calls,"knownUsage":metrics.known_usage,
            "elapsedMs":metrics.elapsed_ms,"evidence":"one observed live task; no baseline comparison"
        })
    );
}
