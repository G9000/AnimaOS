//! Generic email setup and one-attempt delivery, independent of OAuth configuration.
use super::*;
use anima_service_adapters::email::{
    EmailAccount, EmailEndpoint, EmailError, EmailOutgoing, EmailSecrets, EmailSecurity,
};
use zeroize::Zeroize;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum GenericEmailSecurity {
    Tls,
    Starttls,
}
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GenericEmailEndpoint {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) security: GenericEmailSecurity,
    pub(crate) username: String,
}
impl GenericEmailEndpoint {
    fn endpoint(&self) -> EmailEndpoint {
        EmailEndpoint {
            host: self.host.clone(),
            port: self.port,
            security: match self.security {
                GenericEmailSecurity::Tls => EmailSecurity::Tls,
                GenericEmailSecurity::Starttls => EmailSecurity::Starttls,
            },
            username: self.username.clone(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AutoReplySettings {
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) allowed_senders: Vec<String>,
    #[serde(default)]
    pub(crate) authserv_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenericEmailSettings {
    pub(crate) address: String,
    pub(crate) imap: GenericEmailEndpoint,
    pub(crate) smtp: GenericEmailEndpoint,
    pub(crate) auto_reply: AutoReplySettings,
}
impl GenericEmailSettings {
    pub(crate) fn account(&self) -> Result<EmailAccount, MailError> {
        let account = EmailAccount {
            address: self.address.clone(),
            imap: self.imap.endpoint(),
            smtp: self.smtp.endpoint(),
        };
        account.validate().map_err(map_error)?;
        Ok(account)
    }
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GenericEmailSetup {
    pub(crate) address: String,
    pub(crate) imap: GenericEmailEndpoint,
    pub(crate) smtp: GenericEmailEndpoint,
    pub(crate) imap_password: String,
    pub(crate) smtp_password: String,
    #[serde(default)]
    pub(crate) auto_reply: AutoReplySettings,
}
impl std::fmt::Debug for GenericEmailSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenericEmailSetup")
            .field("passwords", &"[REDACTED]")
            .finish()
    }
}
impl Drop for GenericEmailSetup {
    fn drop(&mut self) {
        self.imap_password.zeroize();
        self.smtp_password.zeroize()
    }
}
impl GenericEmailSetup {
    fn settings(&self) -> GenericEmailSettings {
        GenericEmailSettings {
            address: self.address.trim().to_lowercase(),
            imap: self.imap.clone(),
            smtp: self.smtp.clone(),
            auto_reply: AutoReplySettings {
                enabled: self.auto_reply.enabled,
                allowed_senders: self
                    .auto_reply
                    .allowed_senders
                    .iter()
                    .map(|s| s.trim().to_lowercase())
                    .collect(),
                authserv_id: self.auto_reply.authserv_id.trim().to_owned(),
            },
        }
    }
    pub(crate) fn validate(&self) -> Result<(), MailError> {
        self.settings().account()?;
        for value in [&self.imap_password, &self.smtp_password] {
            if value.is_empty() || value.len() > 4096 || value.contains(['\0', '\r', '\n']) {
                return Err(MailError::Invalid);
            }
        }
        let auto = &self.auto_reply;
        if auto.allowed_senders.len() > 100
            || auto.allowed_senders.iter().any(|address| {
                !DraftInput {
                    to: vec![address.trim().to_lowercase()],
                    subject: "validation".into(),
                    body: String::new(),
                }
                .valid()
            })
        {
            return Err(MailError::Invalid);
        }
        if auto.enabled
            && (auto.allowed_senders.is_empty()
                || auto.authserv_id.is_empty()
                || auto.authserv_id.len() > 253
                || !auto
                    .authserv_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b)))
        {
            return Err(MailError::Invalid);
        }
        Ok(())
    }
}
pub(super) fn map_error(error: EmailError) -> MailError {
    match error {
        EmailError::Invalid => MailError::Invalid,
        EmailError::Unauthorized => MailError::Unauthorized,
        EmailError::Unavailable => MailError::Upstream,
        EmailError::CursorChanged => MailError::Conflict,
        EmailError::Unknown => MailError::Unknown,
    }
}
pub(super) fn preview_messages(
    inbox: &anima_service_adapters::email::EmailInbox,
) -> Vec<MailMessage> {
    inbox
        .messages
        .iter()
        .take(25)
        .map(|m| MailMessage {
            id: format!("{}:{}", inbox.uid_validity, m.uid),
            from: m.from.chars().take(998).collect(),
            subject: m.subject.chars().take(998).collect(),
            preview: m.body.chars().take(2000).collect(),
            received_at: m.received_at.chars().take(64).collect(),
        })
        .collect()
}
impl MailManager {
    #[allow(dead_code)]
    pub(crate) fn with_email_backends(
        mut self,
        transport: Arc<dyn anima_service_adapters::email::EmailTransport>,
        credentials: Arc<dyn credentials::EmailCredentialStore>,
    ) -> Self {
        let inner =
            Arc::get_mut(&mut self.0).expect("configure mail backends before sharing the manager");
        inner.email_transport = transport;
        inner.email_credentials = credentials;
        self
    }
    pub(crate) async fn connect_email(
        &self,
        agent: &str,
        input: GenericEmailSetup,
    ) -> Result<MailConnector, MailError> {
        input.validate()?;
        let manager = self.clone();
        let agent = agent.to_owned();
        tokio::spawn(async move { manager.connect_email_owned(&agent, input).await })
            .await
            .map_err(|_| MailError::Persistence)?
    }
    async fn connect_email_owned(
        &self,
        agent: &str,
        input: GenericEmailSetup,
    ) -> Result<MailConnector, MailError> {
        let lock = self.lock(agent, Provider::Email).await;
        let _guard = lock.lock().await;
        if self.closing.is_cancelled() || self.runs.is_draining() {
            return Err(MailError::Conflict);
        }
        let old = self.connector_for_agent(agent, Provider::Email).await?;
        let id = old
            .as_ref()
            .map(|c| c.id.clone())
            .unwrap_or_else(|| format!("mail-{}", uuid::Uuid::new_v4()));
        {
            let shared = self.shared()?;
            let state = shared.read().await;
            if state.mail_drafts.values().any(|d| {
                d.connector_id == id && matches!(d.state, DraftState::Sending | DraftState::Unknown)
            }) {
                return Err(MailError::Conflict);
            }
        }
        let settings = input.settings();
        let account = settings.account()?;
        let secrets = EmailSecrets::new(input.imap_password.clone(), input.smtp_password.clone());
        let inbox = self
            .email_transport
            .verify(&account, &secrets)
            .await
            .map_err(map_error)?;
        if inbox.uid_validity == 0 || inbox.uid_next == 0 {
            return Err(MailError::Upstream);
        }
        let previous = self
            .email_credentials
            .load(&id)
            .await
            .map_err(|_| MailError::Credential)?;
        if self.email_credentials.put(&id, secrets).await.is_err() {
            self.restore_email_credentials(agent, &id, previous).await?;
            return Err(MailError::Credential);
        }
        let now = now_ms();
        let connector = MailConnector {
            id: id.clone(),
            agent_id: agent.into(),
            provider: Provider::Email,
            account_label: Some(settings.address.clone()),
            status: "active".into(),
            created_at_ms: old.map(|c| c.created_at_ms).unwrap_or(now),
            updated_at_ms: now,
            last_synced_at_ms: Some(now),
            error: None,
            email: Some(settings),
        };
        let result = self
            .edit(|state| {
                if self.closing.is_cancelled() || self.runs.is_draining() {
                    return Err(MailError::Conflict);
                }
                state.get_agent(agent).ok_or(MailError::NotFound)?;
                for draft in state
                    .mail_drafts
                    .values_mut()
                    .filter(|d| d.connector_id == id && d.state == DraftState::Pending)
                {
                    draft.state = DraftState::Rejected;
                    draft.error = Some(
                        "Mail configuration changed; create a new draft for this account".into(),
                    );
                    draft.resolved_at_ms = Some(now)
                }
                state.mail_records.insert(
                    id.clone(),
                    MailRecord {
                        connector: connector.clone(),
                        messages: preview_messages(&inbox),
                        deleted: false,
                        auth_generation: uuid::Uuid::new_v4().to_string(),
                        email: Some(mailbox::EmailMailboxState::baseline(&inbox)),
                    },
                );
                update_tools(state, agent, true)?;
                Ok(connector)
            })
            .await;
        if result.is_err() {
            self.restore_email_credentials(agent, &id, previous).await?;
        }
        result
    }
    async fn restore_email_credentials(
        &self,
        agent: &str,
        id: &str,
        previous: Option<EmailSecrets>,
    ) -> Result<(), MailError> {
        let restored = match previous {
            Some(previous) => self.email_credentials.put(id, previous).await,
            None => self.email_credentials.delete(id).await,
        };
        if restored.is_err() {
            // Unknown vault contents cannot be used with a previously active configuration.
            let _ = self
                .record_error(agent, id, Provider::Email, MailError::Unauthorized)
                .await;
            return Err(MailError::Credential);
        }
        Ok(())
    }
    pub(super) async fn email_secrets(
        &self,
        agent: &str,
        id: &str,
    ) -> Result<EmailSecrets, MailError> {
        let secrets = self
            .email_credentials
            .load(id)
            .await
            .map_err(|_| MailError::Credential)?;
        if let Some(secrets) = secrets {
            return Ok(secrets);
        }
        self.record_error(agent, id, Provider::Email, MailError::Unauthorized)
            .await?;
        Err(MailError::Unauthorized)
    }
    pub(super) async fn email_messages(
        &self,
        agent: &str,
        id: &str,
        refresh: bool,
    ) -> Result<Vec<MailMessage>, MailError> {
        let lock = self.lock(agent, Provider::Email).await;
        let _guard = lock.lock().await;
        if refresh {
            let (account, generation) = {
                let shared = self.shared()?;
                let state = shared.read().await;
                let r = Self::record(&state, agent, id, Provider::Email)?;
                if r.connector.status != "active" {
                    return Err(MailError::Unauthorized);
                }
                (
                    r.connector
                        .email
                        .as_ref()
                        .ok_or(MailError::Invalid)?
                        .account()?,
                    r.auth_generation.clone(),
                )
            };
            let secrets = self.email_secrets(agent, id).await?;
            match self.email_transport.inbox(&account, &secrets, None).await {
                Ok(inbox) => {
                    self.edit(|state| {
                        let r = Self::record(state, agent, id, Provider::Email)?;
                        if r.auth_generation != generation {
                            return Err(MailError::Conflict);
                        }
                        let r = state.mail_records.get_mut(id).ok_or(MailError::NotFound)?;
                        r.messages = preview_messages(&inbox);
                        r.connector.last_synced_at_ms = Some(now_ms());
                        r.connector.error = r
                            .email
                            .as_ref()
                            .and_then(|mailbox| mailbox.review_reason.clone());
                        Ok(())
                    })
                    .await?
                }
                Err(error) => {
                    let error = map_error(error);
                    self.record_error(agent, id, Provider::Email, error).await?;
                    return Err(error);
                }
            }
        }
        let shared = self.shared()?;
        let state = shared.read().await;
        Ok(Self::record(&state, agent, id, Provider::Email)?
            .messages
            .clone())
    }
    pub(super) async fn resolve_email_automatic(
        &self,
        agent: &str,
        id: &str,
        draft: &str,
        generation: &str,
    ) -> Result<MailDraft, MailError> {
        let manager = self.clone();
        let agent = agent.to_owned();
        let id = id.to_owned();
        let draft = draft.to_owned();
        let generation = generation.to_owned();
        tokio::spawn(async move {
            manager
                .resolve_email_owned(&agent, &id, &draft, true, Some(&generation))
                .await
        })
        .await
        .map_err(|_| MailError::Unknown)?
    }
    fn email_send_allowed(&self, r: &MailRecord, expected: Option<&str>) -> Result<(), MailError> {
        if r.connector.status != "active" {
            return Err(MailError::Unauthorized);
        }
        if let Some(expected) = expected {
            if self.closing.is_cancelled()
                || self.runs.is_draining()
                || r.auth_generation != expected
                || !r
                    .connector
                    .email
                    .as_ref()
                    .is_some_and(|e| e.auto_reply.enabled)
            {
                return Err(MailError::Conflict);
            }
        }
        Ok(())
    }
    pub(super) async fn resolve_email_owned(
        &self,
        agent: &str,
        id: &str,
        draft_id: &str,
        approve: bool,
        expected: Option<&str>,
    ) -> Result<MailDraft, MailError> {
        let lock = self.lock(agent, Provider::Email).await;
        let _guard = lock.lock().await;
        let (account, generation, metadata) = {
            let shared = self.shared()?;
            let state = shared.read().await;
            let r = Self::record(&state, agent, id, Provider::Email)?;
            let draft = state
                .mail_drafts
                .get(draft_id)
                .filter(|d| d.connector_id == id)
                .ok_or(MailError::NotFound)?;
            if draft.state != DraftState::Pending {
                return Err(MailError::Conflict);
            }
            if approve {
                self.email_send_allowed(r, expected)?;
            }
            (
                r.connector
                    .email
                    .as_ref()
                    .ok_or(MailError::Invalid)?
                    .account()?,
                r.auth_generation.clone(),
                mailbox::email_draft_metadata(r, draft_id),
            )
        };
        let secrets = if approve {
            Some(self.email_secrets(agent, id).await?)
        } else {
            None
        };
        let claimed = self
            .edit(|state| {
                let r = Self::record(state, agent, id, Provider::Email)?;
                if r.auth_generation != generation {
                    return Err(MailError::Conflict);
                }
                if approve {
                    self.email_send_allowed(r, expected)?;
                }
                let d = state
                    .mail_drafts
                    .get_mut(draft_id)
                    .ok_or(MailError::NotFound)?;
                if d.state != DraftState::Pending {
                    return Err(MailError::Conflict);
                }
                d.state = if approve {
                    DraftState::Sending
                } else {
                    DraftState::Rejected
                };
                if !approve {
                    d.resolved_at_ms = Some(now_ms())
                }
                Ok(d.clone())
            })
            .await?;
        let Some(secrets) = secrets else {
            return Ok(claimed);
        };
        let outgoing = EmailOutgoing {
            id: claimed.id.clone(),
            to: claimed.to.clone(),
            subject: claimed.subject.clone(),
            body: claimed.body.clone(),
            in_reply_to: metadata.0,
            automatic: metadata.1,
        };
        // Hold the lifecycle read guard through SMTP; deletion cannot cross the last fence.
        let result = {
            let shared = self.shared()?;
            let state = shared.read().await;
            let r = Self::record(&state, agent, id, Provider::Email)?;
            if r.auth_generation != generation {
                Err(MailError::Conflict)
            } else {
                match self.email_send_allowed(r, expected) {
                    Ok(()) => self
                        .email_transport
                        .send(&account, &secrets, &outgoing)
                        .await
                        .map_err(map_error),
                    Err(error) => Err(error),
                }
            }
        };
        let reauth = matches!(result, Err(MailError::Unauthorized));
        let (status, error) = match result {
            Ok(()) => (DraftState::Sent, None),
            Err(MailError::Unknown) => (DraftState::Unknown, Some(MailError::Unknown.to_string())),
            Err(error) => (DraftState::Failed, Some(error.to_string())),
        };
        self.edit(|state| {
            Self::record(state, agent, id, Provider::Email)?;
            if reauth {
                let r = state.mail_records.get_mut(id).ok_or(MailError::NotFound)?;
                r.connector.status = "reauthRequired".into();
                r.connector.error = Some(MailError::Unauthorized.to_string())
            }
            let draft = state
                .mail_drafts
                .get_mut(draft_id)
                .ok_or(MailError::NotFound)?;
            draft.state = status;
            draft.error = error;
            draft.resolved_at_ms = Some(now_ms());
            Ok(draft.clone())
        })
        .await
    }
}
