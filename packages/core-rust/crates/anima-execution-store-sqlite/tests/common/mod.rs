use anima_core::*;

// Inventories are constructed from the same public manifests that the conformance
// client registers; the adapter never bypasses protection for tests.
pub fn protection() -> PersistenceProtection {
    let inventories = [
        ("workspace.write", RecoveryMode::KeyedIdempotent),
        (
            "anima.conformance.workspace.keyed-alternate",
            RecoveryMode::KeyedIdempotent,
        ),
        ("anima.conformance.workspace.retry", RecoveryMode::Retry),
        ("anima.conformance.workspace.manual", RecoveryMode::Manual),
        (
            "anima.conformance.workspace.non-retryable",
            RecoveryMode::NonRetryable,
        ),
        (
            "anima.conformance.workspace.compensate",
            RecoveryMode::Compensate,
        ),
    ]
    .into_iter()
    .map(|(id, recovery_mode)| {
        let manifest = CapabilityManifest::new(CapabilityManifestInput {
            id: id.into(),
            version: 1,
            kind: CapabilityKind::Workspace,
            label: "Write".into(),
            description: "Writes a workspace file".into(),
            input_schema: serde_json::json!({"type":"object"}),
            output_schema: serde_json::json!({"type":"object"}),
            side_effects: true,
            risk_level: RiskLevel::High,
            host_permissions: vec![],
            secret_references: vec![],
            environment_requirements: vec![],
            timeout_ms: 1_000,
            cancellation_supported: true,
            max_retries: 0,
            idempotent: false,
            recovery_mode,
            supports_streaming: false,
            supports_artifacts: false,
            supports_citations: false,
            compatibility: RuntimeCompatibility {
                minimum_runtime_schema_version: 1,
                maximum_runtime_schema_version: 1,
                manifest_schema_version: 1,
            },
        })
        .unwrap();
        PersistenceCapabilitySecretInventory::new(&manifest, vec![]).unwrap()
    })
    .collect();
    PersistenceProtection::allow_model_payloads(
        PersistenceSnapshotSealKey::new([71; 32]).unwrap(),
        inventories,
    )
    .unwrap()
}
