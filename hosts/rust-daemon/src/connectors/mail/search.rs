//! Independent read snapshots; never advance the automatic-reply mailbox cursor.
use super::*;
use crate::connected_search::{DocumentKind, IndexedDocument};
impl MailManager {
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
        })
    }
}
