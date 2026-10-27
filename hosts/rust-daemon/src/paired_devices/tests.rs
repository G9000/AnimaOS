use super::*;
use crate::mcp::McpConnectionRecord;
use std::collections::HashSet;
#[test]
fn paired_devices_snapshot_restores_only_exact_owned_https_mcp_linkage() {
    let record = PairedDeviceRecord {
        id: uuid::Uuid::new_v4().to_string(),
        agent_id: "owner".into(),
        mcp_connection_id: uuid::Uuid::new_v4().to_string(),
        endpoint: "https://node.example/mcp".into(),
        node_identity: "test-node".into(),
        manifest_digest: anima_service_adapters::paired_devices::digest_bytes(b"manifest"),
        generation: 1,
        revoked: false,
        remote_revocation_confirmed: false,
    };
    let row = McpConnectionRecord {
        id: record.mcp_connection_id.clone(),
        agent_id: "owner".into(),
        label: "node".into(),
        endpoint: record.endpoint.clone(),
        stdio: None,
        bearer_credential_ref: None,
        oauth_credential_ref: None,
        enabled: false,
        version: 1,
        reviewed_tools: vec![],
    };
    let owners = HashSet::from(["owner".into()]);
    let mut snapshot = PairedDevicesSnapshot {
        records: vec![record],
    };
    assert!(snapshot.validate(&[row.clone()], &owners).is_ok());
    snapshot.records[0].endpoint = "https://changed.example/mcp".into();
    assert!(snapshot.validate(&[row], &owners).is_err());
    snapshot.records[0].revoked = true;
    assert!(snapshot.validate(&[], &owners).is_ok());
    assert!(snapshot.validate(&[], &HashSet::new()).is_err());
}

#[test]
fn paired_devices_mcp_removal_or_endpoint_replacement_revokes_link() {
    let mut snapshot = PairedDevicesSnapshot {
        records: vec![PairedDeviceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: "owner".into(),
            mcp_connection_id: uuid::Uuid::new_v4().to_string(),
            endpoint: "https://node.example/mcp".into(),
            node_identity: "node".into(),
            manifest_digest: anima_service_adapters::paired_devices::digest_bytes(b"manifest"),
            generation: 1,
            revoked: false,
            remote_revocation_confirmed: false,
        }],
    };
    snapshot.reconcile_mcp(&[]);
    assert!(snapshot.records[0].revoked);
    assert!(snapshot
        .validate(&[], &HashSet::from(["owner".into()]))
        .is_ok());
}
