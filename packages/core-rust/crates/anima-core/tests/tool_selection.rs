use anima_core::{
    AgentConfig, AgentRuntime, AgentSettings, Content, DataValue, ModelAdapter,
    ModelGenerateRequest, ModelGenerateResponse, ModelStopReason, ModelToolSelector, TaskResult,
    TaskStatus, ToolCall, ToolDescriptor,
};
use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Selection(Mutex<BTreeSet<String>>);
impl ModelToolSelector for Selection {
    fn visible_tool_names(&self) -> BTreeSet<String> {
        self.0.lock().unwrap().clone()
    }
}
#[derive(Default)]
struct Capture {
    configs: Mutex<Vec<AgentConfig>>,
    calls: Mutex<Vec<ToolCall>>,
}
#[async_trait]
impl ModelAdapter for Capture {
    fn provider(&self) -> &str {
        "capture"
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        _: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.configs.lock().unwrap().push(config.clone());
        let call = {
            let mut calls = self.calls.lock().unwrap();
            if calls.is_empty() {
                None
            } else {
                Some(calls.remove(0))
            }
        };
        let reason = if call.is_some() {
            ModelStopReason::ToolCall
        } else {
            ModelStopReason::End
        };
        Ok(ModelGenerateResponse {
            content: Content {
                text: "settled".into(),
                ..Default::default()
            },
            tool_calls: call.map(|call| vec![call]),
            usage: Default::default(),
            stop_reason: reason,
        })
    }
}
fn descriptor(name: &str) -> ToolDescriptor {
    ToolDescriptor {
        name: name.into(),
        description: format!("exact {name}"),
        parameters_schema: BTreeMap::from([(
            "fixture".into(),
            DataValue::String(name.repeat(100)),
        )]),
        examples: None,
    }
}
fn config() -> AgentConfig {
    serde_json::from_value(serde_json::json!({"name":"discovery","model":"capture","tools":[descriptor("essential"),descriptor("hidden")]})).unwrap()
}
fn call(name: &str) -> ToolCall {
    ToolCall {
        id: name.into(),
        name: name.into(),
        args: Default::default(),
    }
}
#[tokio::test]
async fn tool_selection_filters_only_each_provider_copy_and_hydrates_next_request() {
    let original = config();
    let adapter = Arc::new(Capture {
        calls: Mutex::new(vec![call("essential"), call("hidden")]),
        ..Default::default()
    });
    let selection = Arc::new(Selection(Mutex::new(BTreeSet::from([
        "essential".into(),
        "fabricated".into(),
    ]))));
    let mut runtime = AgentRuntime::new(original.clone(), adapter.clone());
    runtime.set_model_tool_selector(Some(selection.clone()));
    let executed = Arc::new(Mutex::new(Vec::new()));
    let result = runtime
        .run_with_tools(Content::default(), {
            let executed = executed.clone();
            move |agent, _, call| {
                let selection = selection.clone();
                let executed = executed.clone();
                let original = original.clone();
                async move {
                    assert_eq!(agent.config.tools, original.tools);
                    executed.lock().unwrap().push(call.name);
                    selection.0.lock().unwrap().insert("hidden".into());
                    TaskResult::success(Content::default(), 0)
                }
            }
        })
        .await;
    assert_eq!(result.status, TaskStatus::Success);
    let configs = adapter.configs.lock().unwrap();
    assert_eq!(configs[0].tools, Some(vec![descriptor("essential")]));
    assert_eq!(configs[1].tools, config().tools);
    assert_eq!(executed.lock().unwrap().as_slice(), ["essential", "hidden"]);
    assert_eq!(runtime.config(), &config());
    let restored_adapter = Arc::new(Capture::default());
    let mut restored = AgentRuntime::from_snapshot(runtime.snapshot(), restored_adapter.clone());
    restored.run(Content::default()).await;
    assert_eq!(
        restored_adapter.configs.lock().unwrap()[0].tools,
        config().tools
    );
}
#[tokio::test]
async fn tool_selection_hidden_exact_call_keeps_dispatch_and_wrapup_removes_all_tools() {
    let mut canonical = config();
    canonical.settings = Some(AgentSettings {
        max_tool_iterations: Some(1),
        max_retries: Some(4),
        ..Default::default()
    });
    let adapter = Arc::new(Capture {
        calls: Mutex::new(vec![call("hidden")]),
        ..Default::default()
    });
    let mut runtime = AgentRuntime::new(canonical.clone(), adapter.clone());
    runtime.set_model_tool_selector(Some(Arc::new(Selection::default())));
    let result = runtime
        .run_with_tools(Content::default(), |_, _, call| async move {
            assert_eq!(call.name, "hidden");
            TaskResult::success(Content::default(), 0)
        })
        .await;
    assert_eq!(result.status, TaskStatus::Success);
    let configs = adapter.configs.lock().unwrap();
    assert!(configs[0].tools.as_ref().unwrap().is_empty());
    assert!(configs[1].tools.as_ref().unwrap().is_empty());
    assert_eq!(configs[1].settings.as_ref().unwrap().max_retries, Some(0));
    assert_eq!(runtime.config(), &canonical);
}
#[tokio::test]
async fn tool_selection_default_sends_legacy_config_unchanged() {
    let adapter = Arc::new(Capture::default());
    let mut runtime = AgentRuntime::new(config(), adapter.clone());
    runtime.run(Content::default()).await;
    assert_eq!(adapter.configs.lock().unwrap()[0], config());
}

#[tokio::test]
async fn tool_selection_stop_precedes_the_next_provider_request() {
    let adapter = Arc::new(Capture::default());
    let mut runtime = AgentRuntime::new(config(), adapter.clone());
    runtime.set_model_tool_selector(Some(Arc::new(Selection::default())));
    let control = anima_core::RunControl::default();
    control.cancel.cancel();
    runtime.set_run_control(control);
    runtime.run(Content::default()).await;
    assert!(adapter.configs.lock().unwrap().is_empty());
}
