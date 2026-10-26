#![allow(dead_code)]
use anima_core::*;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct Definitions(pub AgentDefinition);
#[async_trait]
impl DefinitionResolver for Definitions {
    async fn resolve(&self, _: Uuid, pin: &DefinitionPin) -> Result<AgentDefinition, EngineError> {
        assert_eq!(pin, &DefinitionPin::from_definition(&self.0).unwrap());
        Ok(self.0.clone())
    }
}
pub struct Model(pub Arc<Mutex<Vec<ModelGenerateRequest>>>);
#[async_trait]
impl ModelAdapter for Model {
    fn provider(&self) -> &str {
        "fixture"
    }
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &ModelGenerateResponse) -> bool {
        true
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.0.lock().unwrap().push(request.clone());
        Ok(ModelGenerateResponse {
            content: Content {
                text: "hello".into(),
                ..Content::default()
            },
            tool_calls: None,
            usage: TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 2,
                total_tokens: 12,
                ..TokenUsage::default()
            },
            stop_reason: ModelStopReason::End,
        })
    }
}
pub struct Policy;
#[async_trait]
impl CurrentPolicyResolver for Policy {
    async fn resolve(
        &self,
        _: EnginePolicyRequest,
    ) -> Result<CurrentPolicyResolution, EngineError> {
        panic!("no tools")
    }
}
pub struct Live;
#[async_trait]
impl EngineLiveEventSink for Live {
    async fn emit(&self, _: EngineLiveEvent) -> Result<(), EngineError> {
        Ok(())
    }
}

pub fn definition() -> AgentDefinition {
    serde_json::from_value(serde_json::json!({
        "schema_version":1,"id":"assistant-definition","version":1,"name":"assistant","display_name":"Assistant","description":"test","persona":"Be gentle.","system":"Keep it concise.",
        "model":{"provider":"fixture","model":"fixture","credential_reference":null,"temperature":0.2},
        "source_profile":{"profile_id":"dialogue","profile_version":1},"resolved_capabilities":[],"memory":{"enabled":false,"namespace":"fixture","retention_days":null},
        "approval_policy_id":"fixture","approval_policy_revision":1,"approval_restrictions":[],
        "limits":{"max_turns":4,"timeout_ms":30000,"max_concurrent_tasks":1},"lifecycle":{"auto_start":false,"restart_on_failure":false,"max_restarts":0,"allows_concurrent_sessions":false},"host_requirements":[]
    })).unwrap()
}
