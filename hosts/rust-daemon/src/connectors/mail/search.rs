//! Independent read snapshots; never advance the automatic-reply mailbox cursor.
use super::*;
use crate::connected_search::{DocumentKind, IndexedDocument};

pub(crate) struct ProjectMailThread {
    pub(crate) title: String,
    pub(crate) text: String,
    pub(crate) revision: String,
    pub(crate) partial: bool,
}

impl MailManager {
    /// Read only the selected conversation; this never moves the automatic-reply cursor.
    pub(crate) async fn read_project_thread(
        &self,
        agent: &str,
        id: &str,
        expected_generation: &str,
        selected_message: &str,
        stream_epoch: Option<&str>,
    ) -> Result<ProjectMailThread, MailError> {
        let provider = {
            let shared = self.shared()?;
            let state = shared.read().await;
            let authority = crate::connected_search::source_authority(&state, agent, id)
                .filter(|a| a.enabled && a.generation == expected_generation)
                .ok_or(MailError::Conflict)?;
            let provider = Self::record(&state, agent, id, Provider::parse(&authority.provider)?)?
                .connector
                .provider;
            provider
        };
        if provider == Provider::Email {
            let snapshot = self
                .search_documents(agent, id, expected_generation)
                .await?;
            if snapshot.stream_epoch.as_deref() != stream_epoch {
                return Err(MailError::Conflict);
            }
            let message = snapshot
                .documents
                .into_iter()
                .find(|d| d.external_id == selected_message)
                .ok_or(MailError::NotFound)?;
            return super::client::project_thread(vec![message], true);
        }
        let lock = self.lock(agent, provider).await;
        let _guard = lock.lock().await;
        let auth_generation = {
            let shared = self.shared()?;
            let state = shared.read().await;
            if !crate::connected_search::source_authority(&state, agent, id)
                .is_some_and(|a| a.enabled && a.generation == expected_generation)
            {
                return Err(MailError::Conflict);
            }
            Self::record(&state, agent, id, provider)?
                .auth_generation
                .clone()
        };
        let lease = self
            .oauth_apps
            .locked_config(provider.oauth_provider().ok_or(MailError::Invalid)?)
            .await
            .map_err(|_| MailError::Credential)?;
        let config = lease
            .config()
            .map(|c| OAuthConfig::from_resolved(provider, c))
            .ok_or(MailError::Unconfigured)?;
        let token = self.token(agent, id, provider, &config).await?;
        let result = self
            .transport
            .project_thread(provider, token.access_token(), selected_message)
            .await;
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                if error == MailError::Unauthorized {
                    self.record_error(agent, id, provider, error).await?;
                }
                return Err(error);
            }
        };
        let shared = self.shared()?;
        let state = shared.read().await;
        if Self::record(&state, agent, id, provider)?.auth_generation != auth_generation
            || !crate::connected_search::source_authority(&state, agent, id)
                .is_some_and(|a| a.enabled && a.generation == expected_generation)
        {
            return Err(MailError::Conflict);
        }
        Ok(value)
    }
    pub(crate) async fn search_history_page(
        &self,
        agent: &str,
        id: &str,
        expected_generation: &str,
        request: &crate::connected_search::HistoryPageRequest,
    ) -> Result<crate::connected_search::HistoryPage, MailError> {
        let provider = {
            let shared = self.shared()?;
            let state = shared.read().await;
            state
                .mail_records
                .get(id)
                .filter(|record| {
                    !record.deleted
                        && record.connector.agent_id == agent
                        && record.connector.status == "active"
                })
                .map(|record| record.connector.provider)
                .ok_or(MailError::NotFound)?
        };
        if !matches!(provider, Provider::Gmail | Provider::Outlook) {
            return Err(MailError::Invalid);
        }
        let lock = self.lock(agent, provider).await;
        let _guard = lock.lock().await;
        let auth_generation = {
            let shared = self.shared()?;
            let state = shared.read().await;
            if !crate::connected_search::source_authority(&state, agent, id).is_some_and(
                |authority| authority.enabled && authority.generation == expected_generation,
            ) {
                return Err(MailError::Conflict);
            }
            Self::record(&state, agent, id, provider)?
                .auth_generation
                .clone()
        };
        let lease = self
            .oauth_apps
            .locked_config(provider.oauth_provider().ok_or(MailError::Invalid)?)
            .await
            .map_err(|_| MailError::Credential)?;
        let config = lease
            .config()
            .map(|config| OAuthConfig::from_resolved(provider, config))
            .ok_or(MailError::Unconfigured)?;
        let token = self.token(agent, id, provider, &config).await?;
        let page = match self
            .transport
            .search_history(provider, token.access_token(), request)
            .await
        {
            Ok(page) => page,
            Err(error) => {
                self.record_error(agent, id, provider, error).await?;
                return Err(error);
            }
        };
        let shared = self.shared()?;
        let state = shared.read().await;
        if !crate::connected_search::source_authority(&state, agent, id).is_some_and(|authority| {
            authority.enabled && authority.generation == expected_generation
        }) || Self::record(&state, agent, id, provider)?.auth_generation != auth_generation
        {
            return Err(MailError::Conflict);
        }
        Ok(page)
    }

    pub(crate) async fn search_inbox_membership(
        &self,
        agent: &str,
        id: &str,
        expected_generation: &str,
        messages: &[String],
    ) -> Result<Vec<bool>, MailError> {
        let provider = {
            let shared = self.shared()?;
            let state = shared.read().await;
            state
                .mail_records
                .get(id)
                .filter(|record| {
                    !record.deleted
                        && record.connector.agent_id == agent
                        && record.connector.status == "active"
                })
                .map(|record| record.connector.provider)
                .ok_or(MailError::NotFound)?
        };
        if !matches!(provider, Provider::Gmail | Provider::Outlook) {
            return Err(MailError::Invalid);
        }
        let lock = self.lock(agent, provider).await;
        let _guard = lock.lock().await;
        let auth_generation = {
            let shared = self.shared()?;
            let state = shared.read().await;
            if !crate::connected_search::source_authority(&state, agent, id).is_some_and(
                |authority| authority.enabled && authority.generation == expected_generation,
            ) {
                return Err(MailError::Conflict);
            }
            Self::record(&state, agent, id, provider)?
                .auth_generation
                .clone()
        };
        let lease = self
            .oauth_apps
            .locked_config(provider.oauth_provider().ok_or(MailError::Invalid)?)
            .await
            .map_err(|_| MailError::Credential)?;
        let config = lease
            .config()
            .map(|config| OAuthConfig::from_resolved(provider, config))
            .ok_or(MailError::Unconfigured)?;
        let token = self.token(agent, id, provider, &config).await?;
        let result = match self
            .transport
            .inbox_membership(provider, token.access_token(), messages)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                self.record_error(agent, id, provider, error).await?;
                return Err(error);
            }
        };
        let shared = self.shared()?;
        let state = shared.read().await;
        if !crate::connected_search::source_authority(&state, agent, id).is_some_and(|authority| {
            authority.enabled && authority.generation == expected_generation
        }) || Self::record(&state, agent, id, provider)?.auth_generation != auth_generation
            || result.len() != messages.len()
        {
            return Err(MailError::Conflict);
        }
        Ok(result)
    }

    pub(crate) async fn search_attachments(
        &self,
        agent: &str,
        id: &str,
        expected_generation: &str,
        message_id: &str,
    ) -> Result<crate::connected_search::AttachmentBatch, MailError> {
        let provider = {
            let shared = self.shared()?;
            let state = shared.read().await;
            state
                .mail_records
                .get(id)
                .filter(|record| {
                    !record.deleted
                        && record.connector.agent_id == agent
                        && record.connector.status == "active"
                })
                .map(|record| record.connector.provider)
                .ok_or(MailError::NotFound)?
        };
        if !matches!(provider, Provider::Gmail | Provider::Outlook) {
            return Err(MailError::Invalid);
        }
        let lock = self.lock(agent, provider).await;
        let _guard = lock.lock().await;
        let auth_generation = {
            let shared = self.shared()?;
            let state = shared.read().await;
            if !crate::connected_search::source_authority(&state, agent, id).is_some_and(
                |authority| authority.enabled && authority.generation == expected_generation,
            ) {
                return Err(MailError::Conflict);
            }
            Self::record(&state, agent, id, provider)?
                .auth_generation
                .clone()
        };
        let lease = self
            .oauth_apps
            .locked_config(provider.oauth_provider().ok_or(MailError::Invalid)?)
            .await
            .map_err(|_| MailError::Credential)?;
        let config = lease
            .config()
            .map(|config| OAuthConfig::from_resolved(provider, config))
            .ok_or(MailError::Unconfigured)?;
        let token = self.token(agent, id, provider, &config).await?;
        let result = match self
            .transport
            .attachment_batch(provider, token.access_token(), message_id)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                if error == MailError::Unauthorized {
                    self.record_error(agent, id, provider, error).await?;
                }
                return Err(error);
            }
        };
        let shared = self.shared()?;
        let state = shared.read().await;
        if !crate::connected_search::source_authority(&state, agent, id).is_some_and(|authority| {
            authority.enabled && authority.generation == expected_generation
        }) || Self::record(&state, agent, id, provider)?.auth_generation != auth_generation
        {
            return Err(MailError::Conflict);
        }
        Ok(result)
    }

    pub(crate) async fn search_documents(
        &self,
        agent: &str,
        id: &str,
        expected_generation: &str,
    ) -> Result<crate::connected_search::SourceSnapshot, MailError> {
        let capture = {
            let shared = self.shared()?;
            let s = shared.read().await;
            s.mail_records
                .get(id)
                .filter(|r| {
                    !r.deleted && r.connector.agent_id == agent && r.connector.status == "active"
                })
                .cloned()
                .ok_or(MailError::NotFound)?
        };
        let p = capture.connector.provider;
        let lock = self.lock(agent, p).await;
        let _guard = lock.lock().await;
        let capture = {
            let shared = self.shared()?;
            let s = shared.read().await;
            if !crate::connected_search::source_authority(&s, agent, id)
                .is_some_and(|a| a.enabled && a.generation == expected_generation)
            {
                return Err(MailError::Conflict);
            }
            Self::record(&s, agent, id, p)?.clone()
        };
        let mut stream_epoch = None;
        let result: Vec<IndexedDocument> = if p == Provider::Email {
            let account = capture
                .connector
                .email
                .as_ref()
                .ok_or(MailError::Invalid)?
                .account()?;
            let secrets = self.email_secrets(agent, id).await?;
            let inbox = match self
                .email_transport
                .inbox(&account, &secrets, None)
                .await
                .map_err(generic::map_error)
            {
                Ok(inbox) => inbox,
                Err(error) => {
                    self.record_error(agent, id, p, error).await?;
                    return Err(error);
                }
            };
            if inbox.messages.len() > 25 || inbox.uid_validity == 0 {
                return Err(MailError::Upstream);
            }
            stream_epoch = Some(inbox.uid_validity.to_string());
            inbox
                .messages
                .into_iter()
                .map(|m| {
                    let mut d = crate::connected_search::providers::document(
                        DocumentKind::Mail,
                        format!("{}:{}", inbox.uid_validity, m.uid),
                        "inbox".into(),
                        m.subject,
                        m.body,
                    );
                    d.sender = Some(m.from);
                    d.source_modified_at = Some(m.received_at);
                    d
                })
                .collect()
        } else {
            let lease = self
                .oauth_apps
                .locked_config(p.oauth_provider().ok_or(MailError::Invalid)?)
                .await
                .map_err(|_| MailError::Credential)?;
            let config = lease
                .config()
                .map(|c| OAuthConfig::from_resolved(p, c))
                .ok_or(MailError::Unconfigured)?;
            let token = self.token(agent, id, p, &config).await?;
            match self.transport.search_inbox(p, token.access_token()).await {
                Ok(d) => d,
                Err(e) => {
                    self.record_error(agent, id, p, e).await?;
                    return Err(e);
                }
            }
        };
        self.edit(|s| {
            let r = Self::record(s, agent, id, p)?;
            if r.auth_generation != capture.auth_generation || r.connector.status != "active" {
                return Err(MailError::Conflict);
            }
            let r = s.mail_records.get_mut(id).ok_or(MailError::NotFound)?;
            r.messages = result
                .iter()
                .take(25)
                .map(|d| MailMessage {
                    id: d.external_id.clone(),
                    from: d.sender.clone().unwrap_or_default(),
                    subject: d.title.clone(),
                    preview: d.text.chars().take(2000).collect(),
                    received_at: d.source_modified_at.clone().unwrap_or_default(),
                })
                .collect();
            r.connector.last_synced_at_ms = Some(now_ms());
            Ok(())
        })
        .await?;
        Ok(crate::connected_search::SourceSnapshot {
            documents: result,
            stream_epoch,
            selection_revision: None,
            incomplete_count: 0,
        })
    }
}
