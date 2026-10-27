mod support;

use std::sync::Arc;

use anima_core::{
    AgentRuntime, CapabilityKind, CapabilityManifest, CapabilityReferenceId, Content, GrantScope,
    GrantStatus, LogicalInvocation, PolicyContext, PolicyEngine, PolicyRestrictions, RiskLevel,
    RuntimeCompatibility,
};
use serde_json::json;
use uuid::Uuid;

use support::{config, FinalAnswerModelAdapter};

#[tokio::test]
async fn dialogue_requires_no_workspace() {
    let mut definition = config();
    definition.tools = Some(Vec::new());

    let mut runtime = AgentRuntime::new(definition, Arc::new(FinalAnswerModelAdapter));
    runtime.init();

    let result = runtime
        .run(Content {
            text: "What is the status?".into(),
            ..Content::default()
        })
        .await;

    assert_eq!(result.status, anima_core::TaskStatus::Success);
    assert_eq!(result.data.unwrap().text, "ready to ship");
}

fn filesystem_manifest() -> CapabilityManifest {
    CapabilityManifest::new(anima_core::CapabilityManifestInput {
        id: "workspace.write".into(),
        version: 1,
        kind: CapabilityKind::Workspace,
        label: "Write a workspace file".into(),
        description: "Writes a selected workspace resource.".into(),
        input_schema: json!({ "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }),
        output_schema: json!({ "type": "object" }),
        side_effects: true,
        risk_level: RiskLevel::Critical,
        host_permissions: vec!["filesystem.write".into()],
        secret_references: vec![],
        environment_requirements: vec!["filesystem.workspace".into()],
        timeout_ms: 1_000,
        cancellation_supported: true,
        max_retries: 0,
        idempotent: false,
        recovery_mode: anima_core::RecoveryMode::NonRetryable,
        supports_streaming: false,
        supports_artifacts: false,
        supports_citations: false,
        compatibility: RuntimeCompatibility {
            minimum_runtime_schema_version: 1,
            maximum_runtime_schema_version: 1,
            manifest_schema_version: 1,
        },
    })
    .unwrap()
}

fn invocation() -> LogicalInvocation {
    LogicalInvocation::new(
        Uuid::from_u128(1),
        "step-1",
        "workspace.write",
        1,
        json!({ "path": "report.md" }),
    )
    .unwrap()
}

fn policy_context(
    owner: &str,
    application_scope: &str,
    manifest: &CapabilityManifest,
    invocation: &LogicalInvocation,
) -> PolicyContext {
    PolicyContext::new(
        owner,
        "actor-1",
        "writer-agent",
        1,
        application_scope,
        CapabilityReferenceId::new(Uuid::from_u128(10)),
        manifest,
        invocation,
        1,
        PolicyRestrictions::default(),
        1_000,
    )
    .unwrap()
}

#[test]
fn application_scope_cannot_alias_another_owners_grant() {
    let manifest = filesystem_manifest();
    let invocation = invocation();
    let owner_context = policy_context("owner-1", "app-scope-1", &manifest, &invocation);
    let grant = anima_core::AutonomyGrant::new(
        "grant-1",
        1,
        GrantStatus::Active,
        GrantScope::new(
            owner_context.owner_id.clone(),
            owner_context.actor_id.clone(),
            owner_context.agent_definition_id.clone(),
            owner_context.agent_definition_version,
            owner_context.workspace_id.clone(),
            owner_context.resource_boundary.clone(),
            owner_context.capability_id.clone(),
            owner_context.manifest_version,
            Some(owner_context.canonical_argument_digest),
        )
        .unwrap(),
        RiskLevel::Critical,
        500,
        Some(2_000),
        Some(1),
    )
    .unwrap();

    let other_scope = policy_context("owner-1", "app-scope-2", &manifest, &invocation);
    let other_owner = policy_context("owner-2", "app-scope-1", &manifest, &invocation);

    assert!(PolicyEngine::grant_matches(&grant, &owner_context));
    assert!(!PolicyEngine::grant_matches(&grant, &other_scope));
    assert!(!PolicyEngine::grant_matches(&grant, &other_owner));
    assert!(matches!(
        PolicyEngine::evaluate(&other_scope, std::slice::from_ref(&grant))
            .unwrap()
            .decision,
        anima_core::PolicyDecision::Deny(_)
    ));
    assert!(matches!(
        PolicyEngine::evaluate(&other_owner, &[grant])
            .unwrap()
            .decision,
        anima_core::PolicyDecision::Deny(_)
    ));
}
