use super::*;
impl DriveManager {
    pub(crate) async fn begin_connect(
        &self,
        agent: String,
        ids: Vec<String>,
    ) -> Result<(DriveRecord, String), DriveError> {
        let manager = self.clone();
        self.owned(async move { manager.begin_owned(&agent, ids).await })
            .await
    }
    async fn begin_owned(
        &self,
        agent: &str,
        ids: Vec<String>,
    ) -> Result<(DriveRecord, String), DriveError> {
        let selected = selected_ids(ids)?;
        let gate = self.gate(agent).await;
        let _gate = gate.lock().await;
        let lease = self
            .inner
            .oauth_apps
            .locked_config(OAuthProvider::Google)
            .await
            .map_err(|_| DriveError::Credential)?;
        let config =
            DriveOAuthConfig::from_resolved(lease.config().ok_or(DriveError::Unconfigured)?);
        let shared = self.shared()?;
        let previous = {
            let s = shared.read().await;
            if s.get_agent(agent).is_none() {
                return Err(DriveError::NotFound);
            }
            s.drive_records
                .values()
                .find(|r| r.agent_id == agent && r.status != DriveStatus::Disconnected)
                .cloned()
        };
        if previous
            .as_ref()
            .is_some_and(|r| r.selected_file_ids != selected)
        {
            return Err(DriveError::Conflict);
        }
        let now = now_ms();
        let mut next = DriveRecord {
            id: previous
                .as_ref()
                .map(|r| r.id.clone())
                .unwrap_or_else(|| format!("drive-{}", uuid::Uuid::new_v4())),
            agent_id: agent.into(),
            status: DriveStatus::Pairing,
            generation: uuid::Uuid::new_v4().to_string(),
            selected_file_ids: selected,
            account_permission_id: previous
                .as_ref()
                .and_then(|r| r.account_permission_id.clone()),
            account_label: previous.as_ref().and_then(|r| r.account_label.clone()),
            oauth_config_revision: lease.revision(),
            created_at_ms: previous.as_ref().map(|r| r.created_at_ms).unwrap_or(now),
            updated_at_ms: now,
            cleanup_generations: previous
                .as_ref()
                .map(|r| r.cleanup_generations.clone())
                .unwrap_or_default(),
        };
        if let Some(old) = &previous {
            queue_cleanup(&mut next, old.generation.clone())?;
        }
        let verifier = Zeroizing::new(format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ));
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let nonce = uuid::Uuid::new_v4().to_string();
        let url = config.consent_url(&nonce, &challenge);
        // Reserve bounded private admission before changing an active connection or its grants.
        let prior_pending = {
            let mut pending = self.inner.pending.lock().await;
            pending.retain(|_, p| p.expires_at_ms > now);
            if pending.values().filter(|p| p.agent_id != agent).count() >= 64 {
                return Err(DriveError::Conflict);
            }
            let keys = pending
                .iter()
                .filter(|(_, p)| p.agent_id == agent)
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            let mut prior = Vec::new();
            for key in keys {
                if let Some(p) = pending.remove(&key) {
                    prior.push((key, p));
                }
            }
            pending.insert(
                nonce.clone(),
                Pending {
                    connector_id: next.id.clone(),
                    agent_id: agent.into(),
                    generation: next.generation.clone(),
                    selected_digest: selection_digest(&next.selected_file_ids),
                    revision: lease.revision(),
                    expires_at_ms: now + AUTH_TTL_MS,
                    verifier,
                },
            );
            prior
        };
        if let Err(error) = self.edit_record(previous, next.clone(), Some(false)).await {
            let mut pending = self.inner.pending.lock().await;
            pending.remove(&nonce);
            for (key, p) in prior_pending {
                pending.entry(key).or_insert(p);
            }
            return Err(error);
        }
        if let Err(error) = self.current(&next, None).await {
            self.inner.pending.lock().await.remove(&nonce);
            return Err(error);
        }
        let _ = self.cleanup_record(&next.id).await;
        let next = shared
            .read()
            .await
            .drive_records
            .get(&next.id)
            .cloned()
            .ok_or(DriveError::Conflict)?;
        Ok((next, url))
    }
    pub(crate) async fn complete_connect(
        &self,
        nonce: String,
        code: Option<String>,
    ) -> Result<DriveRecord, DriveError> {
        let manager = self.clone();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);
        self.owned(async move { manager.complete_owned(nonce, code, deadline).await })
            .await
    }
    pub(super) async fn complete_owned(
        &self,
        nonce: String,
        code: Option<String>,
        deadline: tokio::time::Instant,
    ) -> Result<DriveRecord, DriveError> {
        if nonce.len() > 128 {
            return Err(DriveError::InvalidInput);
        }
        let pending = tokio::time::timeout_at(deadline, self.inner.pending.lock())
            .await
            .map_err(|_| DriveError::Deadline)?
            .remove(&nonce)
            .ok_or(DriveError::InvalidInput)?;
        let code = code
            .filter(|code| {
                !code.is_empty() && code.len() <= 8192 && !code.chars().any(char::is_control)
            })
            .ok_or(DriveError::InvalidInput)?;
        if pending.expires_at_ms <= self.callback_now_ms() {
            return Err(DriveError::InvalidInput);
        }
        let gate = tokio::time::timeout_at(deadline, self.gate(&pending.agent_id))
            .await
            .map_err(|_| DriveError::Deadline)?;
        let _gate = tokio::time::timeout_at(deadline, gate.lock())
            .await
            .map_err(|_| DriveError::Deadline)?;
        let lease = tokio::time::timeout_at(
            deadline,
            self.inner.oauth_apps.locked_config(OAuthProvider::Google),
        )
        .await
        .map_err(|_| DriveError::Deadline)?
        .map_err(|_| DriveError::Credential)?;
        if lease.revision() != pending.revision {
            return Err(DriveError::Conflict);
        }
        let config =
            DriveOAuthConfig::from_resolved(lease.config().ok_or(DriveError::Unconfigured)?);
        let shared = self.shared()?;
        let previous = shared
            .read()
            .await
            .drive_records
            .get(&pending.connector_id)
            .filter(|r| {
                r.status == DriveStatus::Pairing
                    && r.agent_id == pending.agent_id
                    && r.generation == pending.generation
                    && r.oauth_config_revision == pending.revision
                    && selection_digest(&r.selected_file_ids) == pending.selected_digest
            })
            .cloned()
            .ok_or(DriveError::Conflict)?;
        tokio::time::timeout_at(deadline, self.current(&previous, None))
            .await
            .map_err(|_| DriveError::Deadline)??;
        if pending.expires_at_ms <= self.callback_now_ms() {
            return Err(DriveError::InvalidInput);
        }
        let grant = tokio::time::timeout_at(
            deadline,
            self.inner
                .transport
                .exchange_code(&config, &code, &pending.verifier),
        )
        .await
        .map_err(|_| DriveError::Deadline)??;
        tokio::time::timeout_at(deadline, self.current(&previous, None))
            .await
            .map_err(|_| DriveError::Deadline)??;
        if !grant
            .granted_scopes
            .iter()
            .any(|scope| scope == DRIVE_SCOPE)
        {
            return Err(DriveError::ReauthRequired);
        }
        let identity = tokio::time::timeout_at(
            deadline,
            self.inner.transport.identify(grant.tokens.access_token()),
        )
        .await
        .map_err(|_| DriveError::Deadline)??;
        tokio::time::timeout_at(deadline, self.current(&previous, None))
            .await
            .map_err(|_| DriveError::Deadline)??;
        if previous
            .account_permission_id
            .as_ref()
            .is_some_and(|id| id != &identity.permission_id)
        {
            return Err(DriveError::Conflict);
        }
        for id in &previous.selected_file_ids {
            let metadata = tokio::time::timeout_at(
                deadline,
                self.inner
                    .transport
                    .metadata(grant.tokens.access_token(), id),
            )
            .await
            .map_err(|_| DriveError::Deadline)??;
            tokio::time::timeout_at(deadline, self.current(&previous, None))
                .await
                .map_err(|_| DriveError::Deadline)??;
            if metadata.id != *id || metadata.trashed {
                return Err(DriveError::NotFound);
            }
        }
        let mut intent = previous.clone();
        queue_cleanup(&mut intent, previous.generation.clone())?;
        self.edit_record(Some(previous), intent.clone(), None)
            .await?;
        if tokio::time::Instant::now() >= deadline {
            let _ = self.compensate_put(&intent).await;
            return Err(DriveError::Deadline);
        }
        if let Err(error) = self
            .inner
            .credentials
            .put(&intent.id, &intent.generation, grant)
            .await
        {
            let _ = self.compensate_put(&intent).await;
            return Err(error);
        }
        if let Err(error) = self.current(&intent, None).await {
            let _ = self.compensate_put(&intent).await;
            return Err(error);
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = self.compensate_put(&intent).await;
            return Err(DriveError::Deadline);
        }
        let mut next = intent.clone();
        next.status = DriveStatus::Active;
        next.account_permission_id = Some(identity.permission_id);
        next.account_label = identity.label;
        next.updated_at_ms = now_ms();
        next.cleanup_generations.retain(|g| g != &next.generation);
        if let Err(error) = self
            .edit_record(Some(intent.clone()), next.clone(), Some(true))
            .await
        {
            let _ = self.compensate_put(&intent).await;
            return Err(error);
        }
        let _ = self.cleanup_record(&next.id).await;
        let next = shared
            .read()
            .await
            .drive_records
            .get(&next.id)
            .cloned()
            .ok_or(DriveError::Conflict)?;
        Ok(next)
    }
    async fn compensate_put(&self, intent: &DriveRecord) -> Result<(), DriveError> {
        let mut failed = intent.clone();
        failed.status = DriveStatus::ReauthRequired;
        failed.updated_at_ms = now_ms();
        let _ = self
            .edit_record(Some(intent.clone()), failed, Some(false))
            .await;
        // Never rely on compensation persistence succeeding to clean the captured generation.
        self.inner
            .credentials
            .delete(&intent.id, &intent.generation)
            .await?;
        self.cleanup_record(&intent.id).await
    }
    pub(super) async fn access_grant(
        &self,
        capture: DriveRecord,
        tool: String,
    ) -> Result<DriveTokenGrant, DriveError> {
        let manager = self.clone();
        self.owned(async move {
            let result = async {
                let gate = manager.gate(&capture.agent_id).await;
                let _gate = gate.lock().await;
                let lease = manager
                    .inner
                    .oauth_apps
                    .locked_config(OAuthProvider::Google)
                    .await
                    .map_err(|_| DriveError::Credential)?;
                if lease.revision() != capture.oauth_config_revision {
                    return Err(DriveError::ReauthRequired);
                }
                let config = DriveOAuthConfig::from_resolved(
                    lease.config().ok_or(DriveError::Unconfigured)?,
                );
                manager.current(&capture, Some(&tool)).await?;
                let previous = manager
                    .inner
                    .credentials
                    .load(&capture.id, &capture.generation)
                    .await?
                    .ok_or(DriveError::ReauthRequired)?;
                manager.current(&capture, Some(&tool)).await?;
                if !previous
                    .granted_scopes
                    .iter()
                    .any(|scope| scope == DRIVE_SCOPE)
                {
                    return Err(DriveError::ReauthRequired);
                }
                if previous.tokens.expires_at_ms() > now_ms().saturating_add(60_000) {
                    return Ok(previous);
                }
                if previous.tokens.refresh_token().is_empty() {
                    return Err(DriveError::ReauthRequired);
                }
                let mut grant = manager
                    .inner
                    .transport
                    .refresh_tokens(&config, previous.tokens.refresh_token())
                    .await?;
                manager.current(&capture, Some(&tool)).await?;
                if grant.granted_scopes.is_empty() {
                    grant.granted_scopes = previous.granted_scopes.clone();
                }
                if !grant
                    .granted_scopes
                    .iter()
                    .any(|scope| scope == DRIVE_SCOPE)
                {
                    return Err(DriveError::ReauthRequired);
                }
                if grant.tokens.refresh_token().is_empty() {
                    grant.tokens = crate::connectors::gcalendar::store::GoogleOAuthTokens::new(
                        grant.tokens.access_token().to_string().into(),
                        previous.tokens.refresh_token().to_string().into(),
                        grant.tokens.expires_at_ms(),
                    );
                }
                let identity = manager
                    .inner
                    .transport
                    .identify(grant.tokens.access_token())
                    .await?;
                manager.current(&capture, Some(&tool)).await?;
                if capture.account_permission_id.as_deref() != Some(identity.permission_id.as_str())
                {
                    return Err(DriveError::Conflict);
                }
                manager
                    .inner
                    .credentials
                    .put(&capture.id, &capture.generation, grant.clone())
                    .await?;
                manager.current(&capture, Some(&tool)).await?;
                Ok(grant)
            }
            .await;
            if matches!(
                result,
                Err(DriveError::ReauthRequired | DriveError::Credential)
            ) {
                let mut next = capture.clone();
                next.status = DriveStatus::ReauthRequired;
                next.updated_at_ms = now_ms();
                manager
                    .edit_record(Some(capture), next, Some(false))
                    .await?;
            }
            result
        })
        .await
    }
}
