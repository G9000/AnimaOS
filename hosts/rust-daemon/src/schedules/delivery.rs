//! Send evidence is independent of model execution and user receipt.
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{ScheduleOutcomeStatus, ScheduleTrigger, ScheduledPromptRecord};

const MAX_DELIVERY_EVENTS: usize = 100;

pub(crate) fn telegram_send_result(
    result: &Result<
        Vec<crate::connectors::telegram::TelegramSentMessage>,
        crate::connectors::telegram::TelegramTransportError,
    >,
    chat_id: &str,
    text: &str,
) -> (DeliveryResult, Vec<String>) {
    use crate::connectors::telegram::{chunk_telegram_text, TelegramTransportError as Error};
    match result {
        Ok(receipts) => {
            let ids = receipts
                .iter()
                .map(|receipt| receipt.message_id.clone())
                .collect();
            let complete = receipts.len() == chunk_telegram_text(text).len()
                && !receipts.is_empty()
                && receipts
                    .iter()
                    .all(|receipt| receipt.chat.id == chat_id && !receipt.message_id.is_empty());
            (
                if complete {
                    DeliveryResult::Sent
                } else {
                    DeliveryResult::Unknown
                },
                ids,
            )
        }
        Err(
            Error::HttpStatus { status: 400..=499 }
            | Error::UpstreamApi {
                code: Some(400..=499),
            }
            | Error::Configuration
            | Error::InvalidChatId
            | Error::InvalidOffset
            | Error::InvalidMessage
            | Error::InvalidRequestBounds
            | Error::PollConflict,
        ) => (
            if chunk_telegram_text(text).len() > 1 {
                DeliveryResult::Unknown
            } else {
                DeliveryResult::Failed
            },
            Vec::new(),
        ),
        Err(_) => (DeliveryResult::Unknown, Vec::new()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DeliveryChannel {
    Workspace,
    Telegram,
    Discord,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DeliveryResult {
    Sent,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReminderVerification {
    pub(crate) automation_id: String,
    pub(crate) scheduled_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeliveryEvent {
    pub(crate) occurrence_id: String,
    pub(crate) outbound_id: Option<String>,
    pub(crate) scheduled_at_ms: Option<u64>,
    pub(crate) channel: DeliveryChannel,
    pub(crate) attempted_at_ms: Option<u64>,
    pub(crate) completed_at_ms: Option<u64>,
    pub(crate) result: DeliveryResult,
    /// Provider send acknowledgements, never device-delivery or read receipts.
    pub(crate) provider_message_ids: Vec<String>,
}

impl ScheduledPromptRecord {
    pub(crate) fn record_delivery_event(&mut self, event: DeliveryEvent) {
        self.delivery_events.push(event);
        if self.delivery_events.len() > MAX_DELIVERY_EVENTS {
            self.delivery_events
                .drain(..self.delivery_events.len() - MAX_DELIVERY_EVENTS);
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReminderStatus {
    pub(crate) automation_id: String,
    pub(crate) scheduled_at_ms: Option<u64>,
    pub(crate) next_due_at_ms: Option<u64>,
    pub(crate) checked_at_ms: u64,
    pub(crate) status: String,
    pub(crate) explanation: String,
    /// Current native channels do not supply device-delivery/read receipts.
    pub(crate) receipt_verified: bool,
    pub(crate) delivery_events: Vec<DeliveryEvent>,
}

impl ReminderStatus {
    pub(crate) fn from_record(record: &ScheduledPromptRecord, now: u64) -> Self {
        let fired = record.last_fired.as_ref();
        let events: Vec<_> = record
            .delivery_events
            .iter()
            .filter(|event| fired.is_some_and(|f| event.occurrence_id == f.run_idempotency_key))
            .cloned()
            .collect();
        let scheduled_at_ms =
            fired
                .and_then(|f| f.scheduled_at_ms)
                .or_else(|| match record.trigger {
                    ScheduleTrigger::Once { at_ms } if fired.is_none() => Some(at_ms),
                    _ if fired.is_none()
                        && !matches!(record.trigger, ScheduleTrigger::Event { .. }) =>
                    {
                        Some(record.next_due_at_ms)
                    }
                    _ => None,
                });
        let (status, explanation) = match events.last() {
            Some(event) if event.result == DeliveryResult::Sent => (
                "sent",
                if event.channel == DeliveryChannel::Workspace {
                    "Posted in the console's main chat (saved there); receipt cannot be verified. This does not confirm device delivery or that the user saw it."
                } else {
                    "The channel acknowledged sending the message; receipt cannot be verified. This does not confirm device delivery or that the user saw it."
                },
            ),
            Some(event) if event.result == DeliveryResult::Failed => (
                "failed", "Sending failed or the reminder could not run. No successful delivery is confirmed.",
            ),
            _ if record.last_safe_outcome.as_ref().is_some_and(|outcome| outcome.status == ScheduleOutcomeStatus::Failed) => (
                "failed", "The reminder run failed. No successful delivery is confirmed.",
            ),
            _ if fired.is_none() && record.enabled && (scheduled_at_ms.is_some_and(|due| due > now) || matches!(record.trigger, ScheduleTrigger::Event { .. })) => (
                "scheduled", "The reminder is scheduled and has no send evidence yet.",
            ),
            _ => (
                "delivery_unconfirmed", "Delivery unconfirmed: running or producing a reply does not confirm sending, device delivery, or that the user saw it. No conclusive channel acknowledgement is available.",
            ),
        };
        Self {
            automation_id: record.id.clone(),
            scheduled_at_ms,
            next_due_at_ms: (record.enabled
                && !matches!(record.trigger, ScheduleTrigger::Event { .. }))
            .then_some(record.next_due_at_ms),
            checked_at_ms: now,
            status: status.into(),
            explanation: explanation.into(),
            receipt_verified: false,
            delivery_events: events,
        }
    }
}
