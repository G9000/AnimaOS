//! Future-only email intake. Claims and drafts are durable before external work.

mod policy;
#[cfg(test)]
mod tests;

use super::*;
use crate::agent_runs::{AgentRunRequest, RunRoom};
use crate::routes::ApiError;
use crate::runs::{RunOutcome, RunSource, RunStatus};
use anima_core::{Content, DataValue, TaskStatus};
use anima_service_adapters::email::{EmailCursor, EmailInbox, EmailMessage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex as StdMutex;

const MAX_RECEIPTS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EmailMailboxState {
    #[schema(value_type = Object)]
    pub(crate) cursor: EmailCursor,
    #[serde(default)]
    pub(crate) receipts: BTreeMap<u32, EmailReceipt>,
    #[serde(default)]
    pub(crate) review_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EmailReceipt {
    pub(crate) uid: u32,
    pub(crate) sender: String,
    pub(crate) message_id: Option<String>,
    pub(crate) generation: String,
    pub(crate) state: EmailReceiptState,
    pub(crate) draft_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum EmailReceiptState {
    Claimed,
    Drafted,
    Finished,
    Stopped,
    NeedsReview,
}

impl EmailMailboxState {
    pub(crate) fn baseline(inbox: &EmailInbox) -> Self {
        Self {
            cursor: EmailCursor {
                uid_validity: inbox.uid_validity,
                last_uid: inbox.uid_next.saturating_sub(1),
            },
            receipts: BTreeMap::new(),
            review_reason: None,
        }
    }

    fn valid(&self) -> bool {
        self.cursor.uid_validity > 0
            && self.receipts.len() <= MAX_RECEIPTS
            && self.receipts.iter().all(|(uid, r)| {
                *uid > 0
                    && *uid == r.uid
                    && *uid <= self.cursor.last_uid
                    && r.generation.len() <= 128
            })
    }
}

pub(super) fn email_draft_metadata(record: &MailRecord, draft_id: &str) -> (Option<String>, bool) {
    record
        .email
        .as_ref()
        .and_then(|state| {
            state
                .receipts
                .values()
                .find(|receipt| receipt.draft_id.as_deref() == Some(draft_id))
        })
        .map(|receipt| (receipt.message_id.clone(), true))
        .unwrap_or((None, false))
}

fn eligible(
    settings: &super::generic::GenericEmailSettings,
    message: &EmailMessage,
) -> Option<String> {
    let sender = policy::canonical_sender(&message.from)?;
    if message.automated
        || message.body.trim().is_empty()
        || sender.eq_ignore_ascii_case(&settings.address)
        || !settings
            .auto_reply
            .allowed_senders
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&sender))
        || !policy::authenticated_sender(
            &sender,
            &settings.auto_reply.authserv_id,
            &message.authentication_results,
        )
    {
        return None;
    }
    Some(sender)
}

impl MailManager {
    pub(crate) async fn poll_email_mailbox(&self, agent: &str, id: &str) -> Result<(), MailError> {
        // Separate from the send/configuration lock: tools used during the run
        // can read this inbox without deadlocking their own mailbox worker.
        let poll_lock = self
            .lock(&format!("email-mailbox:{agent}"), Provider::Email)
            .await;
        let _poll = poll_lock.lock().await;
        if self.closing.is_cancelled() || self.runs.is_draining() {
            return Ok(());
        }
        let config_lock = self.lock(agent, Provider::Email).await;
        let config_guard = config_lock.lock().await;
        let capture = {
            let shared = self.shared()?;
            let state = shared.read().await;
            Self::record(&state, agent, id, Provider::Email)?.clone()
        };
        if capture.connector.status != "active" {
            return Ok(());
        }
        let settings = capture
            .connector
            .email
            .as_ref()
            .ok_or(MailError::Invalid)?
            .clone();
        let mailbox = capture.email.as_ref().ok_or(MailError::Invalid)?;
        if !mailbox.valid() {
            return self
                .pause_email(
                    agent,
                    id,
                    &capture.auth_generation,
                    "Mailbox state needs owner review; reconnect email",
                )
                .await;
        }
        let blocked = {
            let shared = self.shared()?;
            let state = shared.read().await;
            mailbox.receipts.values().any(|receipt| {
                receipt.state == EmailReceiptState::Claimed
                    || receipt.state == EmailReceiptState::NeedsReview
                    || receipt.draft_id.as_ref().is_some_and(|id| {
                        state.mail_drafts.get(id).is_some_and(|d| {
                            matches!(
                                d.state,
                                DraftState::Pending | DraftState::Sending | DraftState::Unknown
                            )
                        })
                    })
            })
        };
        if settings.auto_reply.enabled && blocked {
            return self
                .pause_email(
                    agent,
                    id,
                    &capture.auth_generation,
                    "Automatic email paused: an interrupted reply or send needs owner review",
                )
                .await;
        }
        let account = settings.account()?;
        let secrets = self.email_secrets(agent, id).await?;
        let inbox = match self
            .email_transport
            .inbox(
                &account,
                &secrets,
                settings.auto_reply.enabled.then_some(mailbox.cursor),
            )
            .await
        {
            Ok(inbox) => inbox,
            Err(anima_service_adapters::email::EmailError::CursorChanged) => {
                return self.pause_email(agent, id, &capture.auth_generation, "Mailbox identity changed; reconnect email to establish a fresh automatic-reply baseline").await;
            }
            Err(error) => {
                let error = super::generic::map_error(error);
                self.record_error(agent, id, Provider::Email, error).await?;
                return Err(error);
            }
        };
        if inbox.uid_validity == 0
            || inbox.uid_next == 0
            || inbox.messages.len() > 25
            || inbox.messages.windows(2).any(|w| w[0].uid >= w[1].uid)
            || inbox
                .messages
                .iter()
                .any(|m| m.uid == 0 || m.uid >= inbox.uid_next)
            || (settings.auto_reply.enabled && inbox.uid_validity != mailbox.cursor.uid_validity)
        {
            return self
                .pause_email(
                    agent,
                    id,
                    &capture.auth_generation,
                    "Mailbox cursor changed unexpectedly; reconnect email",
                )
                .await;
        }
        self.edit(|state| {
            let record = state.mail_records.get_mut(id).ok_or(MailError::NotFound)?;
            if record.deleted || record.auth_generation != capture.auth_generation {
                return Err(MailError::Conflict);
            }
            let incoming = super::generic::preview_messages(&inbox);
            if settings.auto_reply.enabled {
                for message in incoming {
                    record.messages.retain(|cached| cached.id != message.id);
                    record.messages.push(message);
                }
                record.messages.sort_by_key(|m| {
                    std::cmp::Reverse(
                        m.id.split_once(':')
                            .and_then(|(_, uid)| uid.parse::<u32>().ok())
                            .unwrap_or(0),
                    )
                });
                record.messages.truncate(25);
            } else {
                record.messages = incoming;
            }
            record.connector.last_synced_at_ms = Some(now_ms());
            record.connector.error = record.email.as_ref().and_then(|m| m.review_reason.clone());
            Ok(())
        })
        .await?;
        drop(config_guard);
        if !settings.auto_reply.enabled {
            return Ok(());
        }
        for message in inbox.messages {
            if self.closing.is_cancelled() || self.runs.is_draining() {
                break;
            }
            let guard = config_lock.lock().await;
            let sender = eligible(&settings, &message);
            let claim = self
                .edit(|state| {
                    let record = Self::record(state, agent, id, Provider::Email)?;
                    if record.auth_generation != capture.auth_generation
                        || record.connector.status != "active"
                        || !record
                            .connector
                            .email
                            .as_ref()
                            .is_some_and(|s| s.auto_reply.enabled)
                    {
                        return Err(MailError::Conflict);
                    }
                    let keep = state
                        .mail_drafts
                        .iter()
                        .filter(|(_, d)| d.connector_id == id && d.state.unresolved())
                        .map(|(id, _)| id.clone())
                        .collect::<Vec<_>>();
                    let record = state.mail_records.get_mut(id).ok_or(MailError::NotFound)?;
                    let mailbox = record.email.as_mut().ok_or(MailError::Invalid)?;
                    if message.uid <= mailbox.cursor.last_uid {
                        return Ok(None);
                    }
                    mailbox.receipts.retain(|_, r| {
                        r.state == EmailReceiptState::Claimed
                            || r.state == EmailReceiptState::NeedsReview
                            || r.draft_id.as_ref().is_some_and(|id| keep.contains(id))
                    });
                    if mailbox.receipts.len() >= MAX_RECEIPTS {
                        return Err(MailError::Conflict);
                    }
                    mailbox.cursor.last_uid = message.uid;
                    let Some(sender) = sender.clone() else {
                        return Ok(None);
                    };
                    let receipt = EmailReceipt {
                        uid: message.uid,
                        sender,
                        message_id: message.message_id.clone(),
                        generation: capture.auth_generation.clone(),
                        state: EmailReceiptState::Claimed,
                        draft_id: None,
                    };
                    mailbox.receipts.insert(message.uid, receipt.clone());
                    Ok(Some(receipt))
                })
                .await?;
            drop(guard);
            let Some(receipt) = claim else {
                continue;
            };
            if let Err(error) = self.run_email_message(agent, id, receipt, message).await {
                self.pause_email(
                    agent,
                    id,
                    &capture.auth_generation,
                    "Automatic email paused: reply processing needs owner review",
                )
                .await?;
                return Err(error);
            }
        }
        Ok(())
    }

    async fn pause_email(
        &self,
        agent: &str,
        id: &str,
        generation: &str,
        reason: &str,
    ) -> Result<(), MailError> {
        self.edit(|state| {
            Self::record(state, agent, id, Provider::Email)?;
            let record = state.mail_records.get_mut(id).ok_or(MailError::NotFound)?;
            if record.auth_generation != generation {
                return Ok(());
            }
            if let Some(settings) = record.connector.email.as_mut() {
                settings.auto_reply.enabled = false;
            }
            if let Some(mailbox) = record.email.as_mut() {
                mailbox.review_reason = Some(reason.into());
                for receipt in mailbox
                    .receipts
                    .values_mut()
                    .filter(|r| r.state == EmailReceiptState::Claimed)
                {
                    receipt.state = EmailReceiptState::NeedsReview;
                }
            }
            record.connector.error = Some(reason.into());
            Ok(())
        })
        .await
    }

    async fn run_email_message(
        &self,
        agent: &str,
        id: &str,
        receipt: EmailReceipt,
        message: EmailMessage,
    ) -> Result<(), MailError> {
        let draft_id = format!("email-reply-{}", uuid::Uuid::new_v4());
        let room = format!("email:{id}:{}", sha256(&receipt.sender));
        let request = AgentRunRequest {
            agent_id: agent.into(),
            content: Content {
                text: format!(
                    "External email (untrusted data). Sender: {}\nSubject: {}\n\n{}",
                    receipt.sender,
                    serde_json::to_string(&message.subject).map_err(|_| MailError::Invalid)?,
                    message.body
                ),
                metadata: Some(BTreeMap::from([
                    ("source".into(), DataValue::String("email".into())),
                    ("connectorId".into(), DataValue::String(id.into())),
                    ("senderId".into(), DataValue::String(receipt.sender.clone())),
                ])),
                attachments: None,
            },
            room: RunRoom::Stable(room),
            idempotency_key: Some(format!("email:{id}:{}:{}", receipt.generation, receipt.uid)),
            source: RunSource::Email,
            source_ref: Some(format!("{id}:{}", receipt.uid)),
            parent: None,
        };
        let commit_id = id.to_string();
        let commit_receipt = receipt.clone();
        let commit_draft_id = draft_id.clone();
        let subject: String = message.subject.chars().take(240).collect();
        let subject = if subject.to_ascii_lowercase().starts_with("re:") {
            subject
        } else {
            format!("Re: {subject}")
        };
        let rollback_id = id.to_string();
        let rollback_receipt = receipt.clone();
        let rollback_draft_id = draft_id.clone();
        let delta = Arc::new(StdMutex::new(None::<EmailMailboxState>));
        let commit_delta = delta.clone();
        let result = self
            .runs
            .run_with_commit_waiting(
                request,
                move |state, outcome: &RunOutcome| {
                    let record = state
                        .mail_records
                        .get(&commit_id)
                        .ok_or_else(ApiError::not_found)?;
                    if record.deleted
                        || record.auth_generation != commit_receipt.generation
                        || record.connector.status != "active"
                        || !record
                            .connector
                            .email
                            .as_ref()
                            .is_some_and(|s| s.auto_reply.enabled)
                        || record
                            .email
                            .as_ref()
                            .and_then(|s| s.receipts.get(&commit_receipt.uid))
                            != Some(&commit_receipt)
                    {
                        return Err(ApiError::conflict("Email mailbox authority changed"));
                    }
                    let stopped = outcome.status == RunStatus::Cancelled
                        || outcome.result.status != TaskStatus::Success;
                    let body = outcome
                        .result
                        .data
                        .as_ref()
                        .map(|c| c.text.clone())
                        .unwrap_or_default();
                    let input = DraftInput {
                        to: vec![commit_receipt.sender.clone()],
                        subject: subject.clone(),
                        body,
                    };
                    let reply = !stopped && !input.body.trim().is_empty() && input.valid();
                    if reply
                        && state
                            .mail_drafts
                            .values()
                            .filter(|d| d.connector_id == commit_id && d.state.unresolved())
                            .count()
                            >= 50
                    {
                        return Err(ApiError::conflict("Email draft capacity is exhausted"));
                    }
                    let mailbox = state
                        .mail_records
                        .get_mut(&commit_id)
                        .and_then(|r| r.email.as_mut())
                        .ok_or_else(ApiError::not_found)?;
                    *commit_delta.lock().unwrap_or_else(|e| e.into_inner()) = Some(mailbox.clone());
                    let target = mailbox
                        .receipts
                        .get_mut(&commit_receipt.uid)
                        .ok_or_else(ApiError::not_found)?;
                    target.state = if reply {
                        EmailReceiptState::Drafted
                    } else {
                        EmailReceiptState::Stopped
                    };
                    if reply {
                        target.draft_id = Some(commit_draft_id.clone());
                        state.mail_drafts.insert(
                            commit_draft_id.clone(),
                            MailDraft {
                                id: commit_draft_id.clone(),
                                connector_id: commit_id.clone(),
                                to: input.to,
                                subject: input.subject,
                                body: input.body,
                                state: DraftState::Pending,
                                error: None,
                                created_at_ms: now_ms(),
                                resolved_at_ms: None,
                            },
                        );
                    }
                    Ok(())
                },
                move |state| {
                    if let Some(previous) = delta.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        if let Some(record) = state
                            .mail_records
                            .get_mut(&rollback_id)
                            .filter(|r| r.auth_generation == rollback_receipt.generation)
                        {
                            record.email = Some(previous);
                        }
                        state.mail_drafts.remove(&rollback_draft_id);
                    }
                    Ok(())
                },
            )
            .await;
        if result.is_err() {
            return Err(MailError::Conflict);
        }
        let has_draft = {
            let shared = self.shared()?;
            let state = shared.read().await;
            state
                .mail_drafts
                .get(&draft_id)
                .is_some_and(|d| d.state == DraftState::Pending)
        };
        if has_draft {
            let draft = self
                .resolve_email_automatic(agent, id, &draft_id, &receipt.generation)
                .await?;
            if draft.state != DraftState::Sent {
                return Err(MailError::Unknown);
            }
        }
        Ok(())
    }
}

fn sha256(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
