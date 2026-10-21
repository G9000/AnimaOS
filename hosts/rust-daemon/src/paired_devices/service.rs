use crate::{
    mcp::{
        credentials::McpBearerToken, CreateMcpConnection, McpConnectionRecord, McpManager,
        McpServiceError, UpdateMcpConnection,
    },
    state::DaemonState,
};
use anima_service_adapters::paired_devices::*;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PairedDevicesSnapshot {
    #[serde(default)]
    pub(crate) records: Vec<PairedDeviceRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PairedDeviceRecord {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) mcp_connection_id: String,
    pub(crate) endpoint: String,
    pub(crate) node_identity: String,
    pub(crate) manifest_digest: String,
    pub(crate) generation: u64,
    pub(crate) revoked: bool,
    #[serde(default)]
    pub(crate) remote_revocation_confirmed: bool,
}
impl PairedDevicesSnapshot {
    /// Used by the existing MCP candidate transaction before saving and when
    /// publishing its row removal or endpoint replacement.
    pub(crate) fn reconcile_mcp(&mut self, rows: &[McpConnectionRecord]) {
        for record in &mut self.records {
            if !rows.iter().any(|row| {
                row.id == record.mcp_connection_id
                    && row.agent_id == record.agent_id
                    && row.endpoint == record.endpoint
                    && row.stdio.is_none()
            }) {
                record.revoked = true;
            }
        }
    }
    pub(crate) fn validate(
        &self,
        rows: &[McpConnectionRecord],
        owners: &HashSet<String>,
    ) -> Result<(), String> {
        if self.records.len() > 32 {
            return Err("paired device limit exceeded".into());
        }
        let mut ids = HashSet::new();
        let mut connections = HashSet::new();
        for record in &self.records {
            if uuid::Uuid::parse_str(&record.id).is_err()
                || uuid::Uuid::parse_str(&record.mcp_connection_id).is_err()
                || !ids.insert(&record.id)
                || !connections.insert(&record.mcp_connection_id)
                || !owners.contains(&record.agent_id)
                || !valid_text(&record.node_identity)
                || !valid_digest(&record.manifest_digest)
                || record.generation == 0
                || record.generation > 9_007_199_254_740_991
                || (record.remote_revocation_confirmed && !record.revoked)
                || !valid_node_endpoint(&record.endpoint)
                || (!record.revoked
                    && !rows.iter().any(|row| {
                        row.id == record.mcp_connection_id
                            && row.agent_id == record.agent_id
                            && row.endpoint == record.endpoint
                            && row.stdio.is_none()
                    }))
            {
                return Err("invalid paired device binding".into());
            }
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PairDeviceInput {
    pub(crate) label: String,
    pub(crate) endpoint: String,
    pub(crate) expected_identity: String,
    pub(crate) expected_manifest_digest: String,
    pub(crate) ticket: McpBearerToken,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PairDeviceOutcome {
    pub(crate) device: PairedDeviceRecord,
    pub(crate) discovery_ready: bool,
}
#[derive(Clone)]
pub(crate) struct PairedDevicesService {
    state: Weak<RwLock<DaemonState>>,
    mcp: McpManager,
    http: reqwest::Client,
}
fn valid_node_endpoint(endpoint: &str) -> bool {
    endpoint.starts_with("https://")
        && crate::mcp::normalized_endpoint(endpoint).is_ok_and(|value| value == endpoint)
        && reqwest::Url::parse(endpoint).is_ok_and(|url| url.path() == "/mcp")
}
fn peer_url(endpoint: &str, path: &str) -> Result<reqwest::Url, McpServiceError> {
    if !valid_node_endpoint(endpoint) {
        return Err(McpServiceError::InvalidRequest);
    }
    reqwest::Url::parse(endpoint)
        .and_then(|url| url.join(path))
        .map_err(|_| McpServiceError::InvalidRequest)
}
impl PairedDevicesService {
    pub(crate) fn new(state: Weak<RwLock<DaemonState>>, mcp: McpManager) -> Self {
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .expect("fixed HTTPS client configuration");
        Self { state, mcp, http }
    }
    #[cfg(test)]
    pub(crate) fn with_client(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }
    fn state(&self) -> Result<Arc<RwLock<DaemonState>>, McpServiceError> {
        self.state.upgrade().ok_or(McpServiceError::Unavailable)
    }
    async fn owner(&self, agent: &str) -> Result<(), McpServiceError> {
        let state = self.state()?;
        let guard = state.read().await;
        if guard.control_plane_store.is_none() {
            return Err(McpServiceError::Unavailable);
        }
        match guard.agents.get(agent) {
            Some(runtime) if !crate::agent_runs::is_helper_config(runtime.config()) => Ok(()),
            _ => Err(McpServiceError::NotFound),
        }
    }
    pub(crate) async fn list(
        &self,
        agent: &str,
    ) -> Result<Vec<PairedDeviceRecord>, McpServiceError> {
        self.owner(agent).await?;
        let state = self.state()?;
        let result = state
            .read()
            .await
            .paired_devices
            .records
            .iter()
            .filter(|record| record.agent_id == agent)
            .cloned()
            .collect();
        Ok(result)
    }
    async fn bounded_json(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Value, McpServiceError> {
        let response = request
            .send()
            .await
            .map_err(|_| McpServiceError::Unavailable)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|n| n > MAX_NODE_WIRE_BYTES as u64)
        {
            return Err(McpServiceError::Unavailable);
        }
        let mut chunks = response.bytes_stream();
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| McpServiceError::Unavailable)?;
            if bytes.len() + chunk.len() > MAX_NODE_WIRE_BYTES {
                return Err(McpServiceError::Unavailable);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| McpServiceError::Unavailable)
    }
    async fn remote_revoke(&self, endpoint: &str, token: &McpBearerToken) -> bool {
        let Ok(url) = peer_url(endpoint, "/revoke") else {
            return false;
        };
        self.http
            .post(url)
            .bearer_auth(token.expose())
            .json(&json!({}))
            .send()
            .await
            .is_ok_and(|response| response.status() == reqwest::StatusCode::NO_CONTENT)
    }
    async fn save_record(&self, mut record: PairedDeviceRecord) -> Result<(), McpServiceError> {
        let transactions = self.mcp.control_plane_transactions();
        let _transaction = transactions.lock().await;
        let state = self.state()?;
        let mut guard = state.write().await;
        let previous = guard.paired_devices.clone();
        if let Some(old) = guard
            .paired_devices
            .records
            .iter_mut()
            .find(|old| old.id == record.id)
        {
            if old.revoked && !record.revoked {
                return Err(McpServiceError::Revoked);
            }
            record.remote_revocation_confirmed |= old.remote_revocation_confirmed;
            *old = record;
        } else {
            if guard.paired_devices.records.len() >= 32 {
                return Err(McpServiceError::Busy);
            }
            guard.paired_devices.records.push(record);
        }
        // The state write guard prevents concurrent ordinary snapshots observing
        // a partial pairing patch. No network or manager call runs under it.
        let saved = guard.control_plane_persist_request().save().await;
        if let Err(error) = saved {
            if !crate::control_plane_store::is_durability_uncertain(&error) {
                guard.paired_devices = previous;
            }
            return Err(McpServiceError::Unavailable);
        }
        Ok(())
    }
    pub(crate) async fn pair(
        &self,
        agent: &str,
        input: PairDeviceInput,
    ) -> Result<PairDeviceOutcome, McpServiceError> {
        let service = self.clone();
        let agent = agent.to_owned();
        let manager = self.mcp.clone();
        manager
            .owned(async move { service.pair_owned(&agent, input).await })
            .await
    }
    async fn pair_owned(
        &self,
        agent: &str,
        input: PairDeviceInput,
    ) -> Result<PairDeviceOutcome, McpServiceError> {
        self.owner(agent).await?;
        if !valid_node_endpoint(&input.endpoint)
            || !valid_text(&input.expected_identity)
            || !valid_digest(&input.expected_manifest_digest)
            || input.ticket.expose().len() != 64
        {
            return Err(McpServiceError::InvalidRequest);
        }
        let manifest = self
            .bounded_json(self.http.get(peer_url(&input.endpoint, "/manifest")?))
            .await?;
        let manifest: NodeManifest = serde_json::from_value(manifest["manifest"].clone())
            .map_err(|_| McpServiceError::Conflict)?;
        if manifest.identity != input.expected_identity
            || manifest.digest().map_err(|_| McpServiceError::Conflict)?
                != input.expected_manifest_digest
        {
            return Err(McpServiceError::Conflict);
        }
        let mut issued=self.bounded_json(self.http.post(peer_url(&input.endpoint,"/pair")?).json(&json!({"ticket":input.ticket.expose(),"expectedIdentity":input.expected_identity,"expectedManifestDigest":input.expected_manifest_digest}))).await?;
        let token = McpBearerToken::parse(
            issued["bearerToken"]
                .take()
                .as_str()
                .ok_or(McpServiceError::Unavailable)?
                .to_owned(),
        )
        .map_err(|_| McpServiceError::Unavailable)?;
        let connection = match self
            .mcp
            .create(
                agent,
                CreateMcpConnection {
                    label: input.label,
                    endpoint: input.endpoint.clone(),
                },
            )
            .await
        {
            Ok(connection) => connection,
            Err(error) => {
                self.remote_revoke(&input.endpoint, &token).await;
                return Err(error);
            }
        };
        let mut record = PairedDeviceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent.into(),
            mcp_connection_id: connection.record.id.clone(),
            endpoint: input.endpoint,
            node_identity: manifest.identity,
            manifest_digest: input.expected_manifest_digest,
            generation: manifest.generation,
            revoked: false,
            remote_revocation_confirmed: false,
        };
        let setup = async {
            self.save_record(record.clone()).await?;
            let connection = self
                .mcp
                .set_bearer(
                    agent,
                    &record.mcp_connection_id,
                    connection.record.version,
                    token.clone(),
                )
                .await?;
            self.mcp
                .update(
                    agent,
                    &record.mcp_connection_id,
                    UpdateMcpConnection {
                        expected_version: connection.record.version,
                        label: None,
                        endpoint: None,
                        enabled: Some(true),
                    },
                )
                .await
        }
        .await;
        match setup {
            Ok(connection) => {
                let discovery_ready = self
                    .mcp
                    .connect(agent, &record.mcp_connection_id, connection.record.version)
                    .await
                    .is_ok();
                Ok(PairDeviceOutcome {
                    device: record,
                    discovery_ready,
                })
            }
            Err(error) => {
                record.revoked = true;
                record.remote_revocation_confirmed =
                    self.remote_revoke(&record.endpoint, &token).await;
                let _ = self.save_record(record.clone()).await;
                if let Ok(current) = self.mcp.get(agent, &record.mcp_connection_id).await {
                    let _ = self
                        .mcp
                        .delete(agent, &record.mcp_connection_id, current.record.version)
                        .await;
                }
                Err(error)
            }
        }
    }
    pub(crate) async fn revoke(
        &self,
        agent: &str,
        id: &str,
    ) -> Result<PairedDeviceRecord, McpServiceError> {
        let service = self.clone();
        let agent = agent.to_owned();
        let id = id.to_owned();
        let manager = self.mcp.clone();
        manager
            .owned(async move { service.revoke_owned(&agent, &id).await })
            .await
    }
    async fn revoke_owned(
        &self,
        agent: &str,
        id: &str,
    ) -> Result<PairedDeviceRecord, McpServiceError> {
        self.owner(agent).await?;
        let mut record = self
            .list(agent)
            .await?
            .into_iter()
            .find(|record| record.id == id)
            .ok_or(McpServiceError::NotFound)?;
        record.revoked = true;
        self.save_record(record.clone()).await?;
        let token = self
            .mcp
            .paired_bearer(agent, &record.mcp_connection_id)
            .await
            .ok()
            .flatten();
        // Local authority retires independently from reachability of the node.
        if let Ok(current) = self.mcp.get(agent, &record.mcp_connection_id).await {
            self.mcp
                .delete(agent, &record.mcp_connection_id, current.record.version)
                .await?;
        }
        if let Some(token) = token {
            record.remote_revocation_confirmed = self.remote_revoke(&record.endpoint, &token).await;
        }
        self.save_record(record.clone()).await?;
        Ok(record)
    }
}
