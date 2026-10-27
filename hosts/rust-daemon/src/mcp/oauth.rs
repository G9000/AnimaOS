//! Owner initiated OAuth. Browser callbacks authenticate an account, never tools.
use super::*;
use anima_mcp_oauth::{OAuthCredentials, OAuthError, OAuthSession};
use futures::future::BoxFuture;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::Zeroizing;

const LOGIN_TIME: Duration = Duration::from_secs(600);

#[derive(Default)]
pub(super) struct OAuthState {
    connections: BTreeMap<String, OAuthConnection>,
}
#[derive(Default)]
struct OAuthConnection {
    agent: String,
    attempt: Option<Attempt>,
    needs_sign_in: bool,
    failed: bool,
    vault: Arc<Mutex<()>>,
}
struct Attempt {
    id: String,
    expires_at: u64,
    cancel: CancellationToken,
}
#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpOAuthStart {
    authorization_url: String,
    attempt_id: String,
    expires_at: u64,
}
impl std::fmt::Debug for McpOAuthStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("McpOAuthStart([REDACTED])")
    }
}
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpOAuthStatus {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<StaticMcpError>,
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl McpManager {
    /// Agent deletion holds the shared control-plane transaction and owns this
    /// future until every vault operation settles. Keep rows available for retry
    /// whenever cleanup is uncertain, including unpublished login anchors.
    pub(crate) async fn prepare_agent_oauth_deletion(
        &self,
        agent: &str,
    ) -> Result<(), McpServiceError> {
        let rows = {
            let state = self.state()?;
            let guard = state.read().await;
            Self::owner(&guard, agent)?;
            self.same_publication(&guard)?;
            guard
                .mcp_registry
                .connections
                .values()
                .filter(|row| row.agent_id == agent)
                .cloned()
                .collect::<Vec<_>>()
        };
        for row in rows {
            self.cancel_oauth(&row.id);
            if row.oauth_credential_ref.is_some() {
                self.retire_and_join(&row.id).await?;
            }
            if let Err(error) = self.delete_oauth_material(&row, false).await {
                self.block_credential(&row.id);
                return Err(error);
            }
        }
        Ok(())
    }
    pub(super) fn cancel_oauth_agent(&self, agent: Option<&str>) {
        if let Ok(mut state) = self.inner.oauth.lock() {
            for connection in state
                .connections
                .values_mut()
                .filter(|connection| agent.is_none_or(|agent| connection.agent == agent))
            {
                if let Some(attempt) = connection.attempt.take() {
                    attempt.cancel.cancel();
                }
            }
        }
    }
    fn oauth_lock(&self, id: &str) -> Result<Arc<Mutex<()>>, McpServiceError> {
        Ok(self
            .inner
            .oauth
            .lock()
            .map_err(|_| McpServiceError::Unavailable)?
            .connections
            .entry(id.into())
            .or_default()
            .vault
            .clone())
    }
    pub(super) fn cancel_oauth(&self, id: &str) {
        if let Ok(mut state) = self.inner.oauth.lock() {
            if let Some(connection) = state.connections.get_mut(id) {
                if let Some(attempt) = connection.attempt.take() {
                    attempt.cancel.cancel();
                }
                connection.failed = false;
            }
        }
    }
    fn oauth_current(&self, id: &str, attempt_id: &str) -> bool {
        !self.inner.stopping.load(Ordering::SeqCst)
            && self.inner.oauth.lock().is_ok_and(|state| {
                state
                    .connections
                    .get(id)
                    .and_then(|row| row.attempt.as_ref())
                    .is_some_and(|attempt| {
                        attempt.id == attempt_id
                            && !attempt.cancel.is_cancelled()
                            && attempt.expires_at > now()
                    })
            })
    }
    fn oauth_failed(&self, id: &str, attempt_id: &str) {
        if let Ok(mut state) = self.inner.oauth.lock() {
            if let Some(connection) = state.connections.get_mut(id) {
                if connection
                    .attempt
                    .as_ref()
                    .is_some_and(|attempt| attempt.id == attempt_id)
                {
                    connection.attempt.take();
                    connection.failed = true;
                }
            }
        }
    }
    fn oauth_needs_sign_in(&self, id: &str) {
        if let Ok(mut state) = self.inner.oauth.lock() {
            state
                .connections
                .entry(id.into())
                .or_default()
                .needs_sign_in = true;
        }
    }
    pub(crate) async fn oauth_status(
        &self,
        agent: &str,
        id: &str,
    ) -> Result<McpOAuthStatus, McpServiceError> {
        let row = self.record(agent, id, None).await?;
        if row.stdio.is_some() {
            return Err(McpServiceError::InvalidRequest);
        }
        let mut state = self
            .inner
            .oauth
            .lock()
            .map_err(|_| McpServiceError::Unavailable)?;
        let connection = state.connections.entry(id.into()).or_default();
        if connection
            .attempt
            .as_ref()
            .is_some_and(|attempt| attempt.expires_at <= now())
        {
            if let Some(attempt) = connection.attempt.take() {
                attempt.cancel.cancel();
            }
            connection.failed = true;
        }
        let status = if connection.attempt.is_some() {
            "pending"
        } else if connection.needs_sign_in {
            "needs_sign_in"
        } else if connection.failed {
            "failed"
        } else if row.oauth_credential_ref.is_some() {
            "connected"
        } else {
            "signed_out"
        };
        Ok(McpOAuthStatus {
            state: status,
            attempt_id: connection
                .attempt
                .as_ref()
                .map(|attempt| attempt.id.clone()),
            expires_at: connection
                .attempt
                .as_ref()
                .map(|attempt| attempt.expires_at),
            error: matches!(status, "needs_sign_in" | "failed").then_some(StaticMcpError {
                code: "oauth_sign_in_required",
                message: "Sign in again to connect this account",
            }),
        })
    }
    pub(crate) async fn start_oauth(
        &self,
        agent: &str,
        id: &str,
        expected: u64,
        client_id: Option<String>,
    ) -> Result<McpOAuthStart, McpServiceError> {
        if client_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 2048 || id.chars().any(char::is_control))
        {
            return Err(McpServiceError::InvalidRequest);
        }
        let manager = self.clone();
        let (agent, id) = (agent.to_owned(), id.to_owned());
        self.owned(async move {
            manager
                .start_oauth_owned(agent, id, expected, client_id)
                .await
        })
        .await
    }
    async fn start_oauth_owned(
        &self,
        agent: String,
        id: String,
        expected: u64,
        client_id: Option<String>,
    ) -> Result<McpOAuthStart, McpServiceError> {
        let transaction = self.inner.transactions.lock().await;
        let record = self.record(&agent, &id, Some(expected)).await?;
        let state = self.state()?;
        {
            let guard = state.read().await;
            self.management_scope(&guard, &agent)?;
            crate::community_bundles::check_mcp_edit(&guard, &id, true, false)?;
            if guard.control_plane_store.is_none() {
                return Err(McpServiceError::Unavailable);
            }
        }
        if record.stdio.is_some() || !credential_endpoint_valid(&record.endpoint) {
            return Err(McpServiceError::InvalidRequest);
        }
        self.cancel_oauth(&id);
        self.retire_and_join(&id).await?;
        {
            let vault = self.oauth_lock(&id)?;
            let _vault = vault.lock().await;
            if self
                .inner
                .credentials
                .recover_oauth(&id, record.oauth_credential_ref.as_deref())
                .await
                .is_err()
            {
                self.block_credential(&id);
                return Err(McpServiceError::Unavailable);
            }
        }
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| McpServiceError::Unavailable)?;
        let address = listener
            .local_addr()
            .map_err(|_| McpServiceError::Unavailable)?;
        let attempt_id = uuid::Uuid::new_v4().to_string();
        // Native pre-registered clients allow a varying loopback port, while
        // their registered redirect path must remain stable across attempts.
        let callback_path = "/oauth/mcp/callback".to_owned();
        let redirect_uri = format!("http://{address}{callback_path}");
        let expires_at = now() + LOGIN_TIME.as_secs();
        let cancel = CancellationToken::new();
        {
            let mut oauth = self
                .inner
                .oauth
                .lock()
                .map_err(|_| McpServiceError::Unavailable)?;
            // Linearize queued sign-in admission with shutdown's cancellation
            // pass under this same lock, before any provider discovery starts.
            self.running()?;
            let connection = oauth.connections.entry(id.clone()).or_default();
            connection.agent = agent.clone();
            connection.attempt = Some(Attempt {
                id: attempt_id.clone(),
                expires_at,
                cancel: cancel.clone(),
            });
        }
        drop(transaction);
        let session = tokio::select! { biased;
            _=cancel.cancelled()=>return Err(McpServiceError::Cancelled),
            result=tokio::time::timeout(Duration::from_secs(30), OAuthSession::begin(&record.endpoint, &redirect_uri, client_id.as_deref()))=>match result {
                Ok(Ok(session))=>session,
                _=>{ self.oauth_failed(&id, &attempt_id); return Err(McpServiceError::Unavailable); }
            }
        };
        if !self.oauth_current(&id, &attempt_id)
            || self.record(&agent, &id, Some(expected)).await.is_err()
        {
            return Err(McpServiceError::Conflict);
        }
        let authorization_url = session.authorization_url().to_owned();
        let expected_state = reqwest::Url::parse(&authorization_url)
            .ok()
            .and_then(|url| {
                url.query_pairs()
                    .find(|(key, _)| key == "state")
                    .map(|(_, value)| Zeroizing::new(value.into_owned()))
            })
            .ok_or(McpServiceError::Unavailable)?;
        let manager = self.clone();
        let owned_attempt = attempt_id.clone();
        // Register the listener before publishing the sign-in attempt. Shutdown
        // must join it even when the spawned future has not yet been polled.
        self.inner.pending.fetch_add(1, Ordering::SeqCst);
        let work = Work(self.inner.clone());
        tokio::spawn(async move {
            let _work = work;
            let result = manager
                .wait_oauth_callback(
                    listener,
                    callback_path,
                    expected_state,
                    cancel,
                    expires_at,
                    session,
                    &record,
                    &owned_attempt,
                )
                .await;
            if result.is_err() {
                manager.oauth_failed(&id, &owned_attempt);
            }
        });
        Ok(McpOAuthStart {
            authorization_url,
            attempt_id,
            expires_at,
        })
    }
    async fn wait_oauth_callback(
        &self,
        listener: tokio::net::TcpListener,
        path: String,
        expected_state: Zeroizing<String>,
        cancel: CancellationToken,
        expires_at: u64,
        session: OAuthSession,
        record: &McpConnectionRecord,
        attempt_id: &str,
    ) -> Result<(), McpServiceError> {
        let mut session = Some(session);
        loop {
            let accepted = tokio::select! { biased;
                _=cancel.cancelled()=>return Err(McpServiceError::Cancelled),
                _=tokio::time::sleep(Duration::from_secs(expires_at.saturating_sub(now())))=>return Err(McpServiceError::TimedOut),
                accepted=listener.accept()=>accepted.map_err(|_| McpServiceError::Unavailable)?,
            };
            let (mut stream, peer) = accepted;
            if !peer.ip().is_loopback() {
                continue;
            }
            let callback = tokio::time::timeout(
                Duration::from_secs(3),
                read_callback(&mut stream, &path, &expected_state),
            )
            .await;
            let Ok(Ok(callback)) = callback else {
                let _ = callback_response(&mut stream, false).await;
                continue;
            };
            if !self.oauth_current(&record.id, attempt_id) {
                let _ = callback_response(&mut stream, false).await;
                return Err(McpServiceError::Revoked);
            }
            if session
                .as_ref()
                .ok_or(McpServiceError::Revoked)?
                .validate_callback_issuer(callback.issuer.as_deref())
                .is_err()
            {
                // Validate issuer on both success and denial responses without
                // consuming a legitimate pending authorization attempt.
                let _ = callback_response(&mut stream, false).await;
                continue;
            }
            let result = if callback.denied {
                Err(McpServiceError::Cancelled)
            } else {
                let exchange = session.take().ok_or(McpServiceError::Revoked)?.complete(
                    &callback.code,
                    &callback.state,
                    callback.issuer.as_deref(),
                );
                tokio::select! { biased;
                    _=cancel.cancelled()=>Err(McpServiceError::Cancelled),
                    result=tokio::time::timeout(Duration::from_secs(30), exchange)=>match result {
                        Ok(Ok(credentials))=>self.save_oauth(record, attempt_id, credentials).await,
                        _=>Err(McpServiceError::Unavailable),
                    }
                }
            };
            let _ = callback_response(&mut stream, result.is_ok()).await;
            if let Ok(committed) = &result {
                // Explicit sign-in admits this one connection. Discovery still
                // publishes only descriptors already reviewed and granted.
                if committed.enabled {
                    let _ = self
                        .discover_owned_with_oauth(
                            &committed.agent_id,
                            &committed.id,
                            committed.version,
                            true,
                            Some(cancel.clone()),
                        )
                        .await;
                }
                // Keep the attempt cancel token until the exact continuation has
                // finished admission; owner disconnect/edit/sign-out can fence it.
                if let Ok(mut state) = self.inner.oauth.lock() {
                    if let Some(connection) = state.connections.get_mut(&record.id) {
                        if connection
                            .attempt
                            .as_ref()
                            .is_some_and(|attempt| attempt.id == attempt_id)
                        {
                            connection.attempt.take();
                        }
                    }
                }
            }
            return result.map(|_| ());
        }
    }
    async fn save_oauth(
        &self,
        old: &McpConnectionRecord,
        attempt_id: &str,
        credentials: OAuthCredentials,
    ) -> Result<McpConnectionRecord, McpServiceError> {
        let _transaction = self.inner.transactions.lock().await;
        self.record(&old.agent_id, &old.id, Some(old.version))
            .await?;
        if !self.oauth_current(&old.id, attempt_id) || credentials.resource() != old.endpoint {
            return Err(McpServiceError::Revoked);
        }
        self.retire_and_join(&old.id).await?;
        let vault = self.oauth_lock(&old.id)?;
        let _vault = vault.lock().await;
        self.inner
            .credentials
            .recover_oauth(&old.id, old.oauth_credential_ref.as_deref())
            .await
            .map_err(|_| McpServiceError::Unavailable)?;
        let reference = uuid::Uuid::new_v4().to_string();
        let payload = credentials
            .to_json()
            .map_err(|_| McpServiceError::Unavailable)?;
        if self
            .inner
            .credentials
            .put_oauth(&old.id, &old.endpoint, &reference, payload, false)
            .await
            .is_err()
        {
            let _ = self
                .inner
                .credentials
                .delete_oauth(&old.id, &reference)
                .await;
            self.block_credential(&old.id);
            return Err(McpServiceError::Unavailable);
        }
        let result = async {
            self.delete_oauth_material_locked(old, false).await?;
            if let Some(previous) = &old.bearer_credential_ref {
                self.inner
                    .credentials
                    .delete(&old.id, previous)
                    .await
                    .map_err(|_| McpServiceError::Unavailable)?;
            }
            let state = self.state()?;
            let mut guard = state.write().await;
            Self::owner(&guard, &old.agent_id)?;
            self.same_publication(&guard)?;
            if guard.mcp_registry.connections.get(&old.id) != Some(old)
                || !self.oauth_current(&old.id, attempt_id)
            {
                return Err(McpServiceError::Conflict);
            }
            let mut next = guard.mcp_registry.clone();
            let row = next
                .connections
                .get_mut(&old.id)
                .ok_or(McpServiceError::NotFound)?;
            row.version = next_version(row.version)?;
            row.bearer_credential_ref = None;
            row.oauth_credential_ref = Some(reference.clone());
            let persist = guard
                .control_plane_persist_request()
                .with_mcp_candidate(next.snapshot(), None)
                .ok_or(McpServiceError::Unavailable)?;
            drop(guard);
            let saved = persist.save_mcp_candidate().await;
            if saved
                .as_ref()
                .is_err_and(|error| !crate::control_plane_store::is_durability_uncertain(error))
            {
                return Err(McpServiceError::Unavailable);
            }
            let mut guard = state.write().await;
            self.same_publication(&guard)?;
            guard.mcp_registry = next;
            self.publish(&guard)?;
            if saved.is_err() {
                return Err(McpServiceError::Unavailable);
            }
            let committed = guard.mcp_registry.connections[&old.id].clone();
            drop(guard);
            self.inner
                .credentials
                .commit_oauth(&old.id, &reference)
                .await
                .map_err(|_| McpServiceError::Unavailable)?;
            let mut oauth = self
                .inner
                .oauth
                .lock()
                .map_err(|_| McpServiceError::Unavailable)?;
            let connection = oauth.connections.entry(old.id.clone()).or_default();
            connection.needs_sign_in = false;
            connection.failed = false;
            self.inner
                .live
                .lock()
                .map_err(|_| McpServiceError::Unavailable)?
                .credential_blocked
                .remove(&old.id);
            Ok(committed)
        }
        .await;
        if result.is_err() {
            let _ = self
                .inner
                .credentials
                .delete_oauth(&old.id, &reference)
                .await;
            self.block_credential(&old.id);
        }
        result
    }
    pub(crate) async fn cancel_oauth_attempt(
        &self,
        agent: &str,
        id: &str,
        expected: u64,
        attempt_id: &str,
    ) -> Result<McpConnectionView, McpServiceError> {
        let manager = self.clone();
        let (agent, id, attempt_id) = (agent.to_owned(), id.to_owned(), attempt_id.to_owned());
        self.owned(async move {
            let _transaction = manager.inner.transactions.lock().await;
            let row = manager.record(&agent, &id, Some(expected)).await?;
            if !manager
                .inner
                .oauth
                .lock()
                .map_err(|_| McpServiceError::Unavailable)?
                .connections
                .get(&id)
                .and_then(|row| row.attempt.as_ref())
                .is_some_and(|attempt| attempt.id == attempt_id)
            {
                return Err(McpServiceError::Conflict);
            }
            manager.cancel_oauth(&id);
            let state = manager.state()?;
            let guard = state.read().await;
            manager.view(row, &guard)
        })
        .await
    }
    async fn delete_oauth_material_locked(
        &self,
        row: &McpConnectionRecord,
        revoke: bool,
    ) -> Result<(), McpServiceError> {
        let Some(reference) = &row.oauth_credential_ref else {
            return Ok(());
        };
        // Remote revocation is best effort; local deletion remains authoritative.
        if revoke {
            if let Ok(Some(payload)) = self
                .inner
                .credentials
                .load_oauth(&row.id, &row.endpoint, reference)
                .await
            {
                if let Ok(credentials) = OAuthCredentials::from_json(&payload) {
                    let _ =
                        tokio::time::timeout(Duration::from_secs(5), credentials.revoke()).await;
                }
            }
        }
        self.inner
            .credentials
            .delete_oauth(&row.id, reference)
            .await
            .map_err(|_| McpServiceError::Unavailable)
    }
    pub(super) async fn delete_oauth_material(
        &self,
        row: &McpConnectionRecord,
        revoke: bool,
    ) -> Result<(), McpServiceError> {
        let vault = self.oauth_lock(&row.id)?;
        let _vault = vault.lock().await;
        self.inner
            .credentials
            .recover_oauth(&row.id, None)
            .await
            .map_err(|_| McpServiceError::Unavailable)?;
        self.delete_oauth_material_locked(row, revoke).await
    }
    pub(crate) async fn sign_out_oauth(
        &self,
        agent: &str,
        id: &str,
        expected: u64,
    ) -> Result<McpConnectionView, McpServiceError> {
        let manager = self.clone();
        let (agent, id) = (agent.to_owned(), id.to_owned());
        self.owned(async move {
            let _transaction = manager.inner.transactions.lock().await;
            let old = manager.record(&agent, &id, Some(expected)).await?;
            let state = manager.state()?;
            {
                let guard = state.read().await;
                manager.management_scope(&guard, &agent)?;
                crate::community_bundles::check_mcp_edit(&guard, &id, false, false)?;
            }
            manager.cancel_oauth(&id);
            manager.retire_and_join(&id).await?;
            if let Err(error) = manager.delete_oauth_material(&old, true).await {
                manager.block_credential(&id);
                return Err(error);
            }
            if old.oauth_credential_ref.is_none() {
                return manager.view(old, &*state.read().await);
            }
            let mut guard = state.write().await;
            let mut next = guard.mcp_registry.clone();
            let row = next
                .connections
                .get_mut(&id)
                .ok_or(McpServiceError::NotFound)?;
            row.version = next_version(row.version)?;
            row.oauth_credential_ref = None;
            let persist = guard
                .control_plane_persist_request()
                .with_mcp_candidate(next.snapshot(), None)
                .ok_or(McpServiceError::Unavailable)?;
            drop(guard);
            let result = persist.save_mcp_candidate().await;
            if result
                .as_ref()
                .is_err_and(|error| !crate::control_plane_store::is_durability_uncertain(error))
            {
                manager.block_credential(&id);
                return Err(McpServiceError::Unavailable);
            }
            let mut guard = state.write().await;
            manager.same_publication(&guard)?;
            guard.mcp_registry = next;
            manager.publish(&guard)?;
            if result.is_err() {
                manager.block_credential(&id);
                return Err(McpServiceError::Unavailable);
            }
            if let Ok(mut oauth) = manager.inner.oauth.lock() {
                let row = oauth.connections.entry(id.clone()).or_default();
                row.needs_sign_in = false;
                row.failed = false;
            }
            manager
                .inner
                .live
                .lock()
                .map_err(|_| McpServiceError::Unavailable)?
                .credential_blocked
                .remove(&id);
            manager.view(guard.mcp_registry.connections[&id].clone(), &guard)
        })
        .await
    }
    pub(super) fn oauth_provider(
        &self,
        record: &McpConnectionRecord,
        cancel: CancellationToken,
    ) -> Arc<dyn super::super::transport::McpAccessTokenProvider> {
        Arc::new(OAuthProvider {
            inner: Arc::downgrade(&self.inner),
            agent: record.agent_id.clone(),
            id: record.id.clone(),
            endpoint: record.endpoint.clone(),
            reference: record.oauth_credential_ref.clone().expect("OAuth binding"),
            cancel,
        })
    }
}

struct OAuthProvider {
    inner: Weak<Inner>,
    agent: String,
    id: String,
    endpoint: String,
    reference: String,
    cancel: CancellationToken,
}
impl std::fmt::Debug for OAuthProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("McpOAuthProvider([REDACTED])")
    }
}
impl super::super::transport::McpAccessTokenProvider for OAuthProvider {
    fn authentication_rejected(&self) {
        if let Some(inner) = self.inner.upgrade() {
            McpManager {
                inner,
                management_run: None,
            }
            .oauth_needs_sign_in(&self.id);
        }
    }
    fn token(
        &self,
    ) -> BoxFuture<'static, Result<McpBearerToken, super::super::transport::McpTransportError>>
    {
        let (inner, agent, id, endpoint, reference, cancel) = (
            self.inner.clone(),
            self.agent.clone(),
            self.id.clone(),
            self.endpoint.clone(),
            self.reference.clone(),
            self.cancel.clone(),
        );
        Box::pin(async move {
            let inner = inner
                .upgrade()
                .ok_or(super::super::transport::McpTransportError::Unavailable)?;
            let manager = McpManager {
                inner,
                management_run: None,
            };
            let owned = manager.clone();
            manager
                .owned(async move {
                    let vault = owned.oauth_lock(&id)?;
                    let _vault = vault.lock().await;
                    let row = owned.record(&agent, &id, None).await?;
                    if cancel.is_cancelled()
                        || row.endpoint != endpoint
                        || row.oauth_credential_ref.as_deref() != Some(reference.as_str())
                    {
                        return Err(McpServiceError::Revoked);
                    }
                    if owned
                        .inner
                        .oauth
                        .lock()
                        .map_err(|_| McpServiceError::Unavailable)?
                        .connections
                        .get(&id)
                        .is_some_and(|state| state.needs_sign_in)
                    {
                        return Err(McpServiceError::Unavailable);
                    }
                    let result = async {
                        let payload = owned
                            .inner
                            .credentials
                            .load_oauth(&id, &endpoint, &reference)
                            .await
                            .map_err(|_| McpServiceError::Unavailable)?
                            .ok_or_else(|| {
                                owned.oauth_needs_sign_in(&id);
                                McpServiceError::Unavailable
                            })?;
                        let mut credentials =
                            OAuthCredentials::from_json(&payload).map_err(|_| {
                                owned.oauth_needs_sign_in(&id);
                                McpServiceError::Unavailable
                            })?;
                        if credentials.resource() != endpoint {
                            owned.oauth_needs_sign_in(&id);
                            return Err(McpServiceError::Unavailable);
                        }
                        let token = credentials.access_token().await.map_err(|error| {
                            if matches!(
                                error,
                                OAuthError::AuthorizationRequired | OAuthError::Rejected
                            ) {
                                owned.oauth_needs_sign_in(&id);
                            }
                            McpServiceError::Unavailable
                        })?;
                        let refreshed = credentials
                            .to_json()
                            .map_err(|_| McpServiceError::Unavailable)?;
                        // Persist refresh rotation even if cancellation arrived during
                        // the exchange. Deletion waits on this same owned vault lock.
                        if refreshed.as_str() != payload.as_str() {
                            owned
                                .inner
                                .credentials
                                .put_oauth(&id, &endpoint, &reference, refreshed, true)
                                .await
                                .map_err(|_| {
                                    owned.block_credential(&id);
                                    owned.oauth_needs_sign_in(&id);
                                    McpServiceError::Unavailable
                                })?;
                        }
                        if cancel.is_cancelled() {
                            return Err(McpServiceError::Revoked);
                        }
                        McpBearerToken::parse(token.to_string())
                            .map_err(|_| McpServiceError::Unavailable)
                    }
                    .await;
                    result
                })
                .await
                .map_err(|_| super::super::transport::McpTransportError::Unavailable)
        })
    }
}

struct Callback {
    code: Zeroizing<String>,
    state: Zeroizing<String>,
    issuer: Option<String>,
    denied: bool,
}
async fn read_callback(
    stream: &mut tokio::net::TcpStream,
    path: &str,
    expected_state: &str,
) -> Result<Callback, ()> {
    let mut bytes = Zeroizing::new(Vec::new());
    loop {
        let mut chunk = [0u8; 1024];
        let count = stream.read(&mut chunk).await.map_err(|_| ())?;
        if count == 0 {
            return Err(());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > 8192 {
            return Err(());
        }
        if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&bytes).map_err(|_| ())?;
    let mut line = request.lines().next().ok_or(())?.split_whitespace();
    if line.next() != Some("GET") {
        return Err(());
    }
    let target = line.next().ok_or(())?;
    if !target.starts_with('/') || line.next() != Some("HTTP/1.1") || line.next().is_some() {
        return Err(());
    }
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| ())?;
    if url.path() != path || url.fragment().is_some() {
        return Err(());
    }
    let mut params = BTreeMap::new();
    for (key, value) in url.query_pairs() {
        if params
            .insert(key.into_owned(), Zeroizing::new(value.into_owned()))
            .is_some()
        {
            return Err(());
        }
    }
    let state = params.remove("state").ok_or(())?;
    if state.as_str() != expected_state {
        return Err(());
    }
    let denied = params.contains_key("error");
    let code = params.remove("code").unwrap_or_default();
    if !denied && (code.is_empty() || code.len() > 4096) {
        return Err(());
    }
    let issuer = params.remove("iss").map(|value| value.to_string());
    Ok(Callback {
        code,
        state,
        issuer,
        denied,
    })
}
async fn callback_response(
    stream: &mut tokio::net::TcpStream,
    success: bool,
) -> Result<(), std::io::Error> {
    let body = if success {
        "<!doctype html><html><head><title>Account connected</title></head><body><p>Account connected. You can close this window and return to AnimaOS.</p></body></html>"
    } else {
        "<!doctype html><html><head><title>Sign-in incomplete</title></head><body><p>Sign-in did not complete. Return to AnimaOS and try again.</p></body></html>"
    };
    let response=format!("HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",if success { "200 OK" } else { "400 Bad Request" },body.len());
    tokio::time::timeout(
        Duration::from_secs(2),
        stream.write_all(response.as_bytes()),
    )
    .await
    .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))??;
    Ok(())
}
