//! What an automation leaves in the owner's main chat when it speaks: a card
//! (shown in full there), or a note when the output went to Discord. The
//! main chat always hears about it, so its agent can answer "where did my
//! reminder go?" and the console can link to it.
//!
//! `content.text` is written for the model and names the automation's id, so
//! the agent can mark a reminder done when the owner replies "done". The
//! console renders from `content.metadata` instead (`kind`, `body`, ...).

use std::collections::BTreeMap;

use anima_core::{Content, DataValue, Message, MessageRole};

use super::{automations::display_name, ScheduleTrigger, ScheduledPromptRecord};

/// Where the automation's output went besides the main chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MainChatDelivery {
    /// Only the main chat.
    Main,
    /// The main chat, and a copy to the owner's Telegram chat.
    Telegram,
    /// Posted to a Discord channel; the main chat gets a note.
    Discord {
        /// `#channel`, as the owner knows it.
        label: String,
        /// The web session of that channel, when the console has one.
        session_id: Option<String>,
        /// The copy saved in that session, for the console's anchor.
        message_id: Option<String>,
    },
}

/// Whether this automation is a one-time reminder.
pub(crate) fn is_reminder(record: &ScheduledPromptRecord) -> bool {
    matches!(record.trigger, ScheduleTrigger::Once { .. })
}

fn string(value: impl Into<String>) -> DataValue {
    DataValue::String(value.into())
}

/// The main chat's message for one spoken run of `record`.
pub(crate) fn main_chat_message(
    record: &ScheduledPromptRecord,
    room_id: &str,
    run_id: &str,
    reply_id: &str,
    reply: &str,
    delivery: &MainChatDelivery,
    now: u64,
) -> Message {
    let name = display_name(record);
    let reminder = is_reminder(record);
    let mut metadata = BTreeMap::from([
        ("automationId".to_string(), string(&record.id)),
        ("automationName".to_string(), string(&name)),
        ("reminder".to_string(), DataValue::Bool(reminder)),
        ("body".to_string(), string(reply)),
        ("runId".to_string(), string(run_id)),
        ("firedAtMs".to_string(), DataValue::Number(now as f64)),
    ]);
    let what = if reminder { "Reminder" } else { "Automation" };
    let text = match delivery {
        MainChatDelivery::Main | MainChatDelivery::Telegram => {
            metadata.insert("kind".into(), string("automation"));
            let telegram = matches!(delivery, MainChatDelivery::Telegram);
            metadata.insert(
                "delivery".into(),
                string(if telegram { "telegram" } else { "main" }),
            );
            let copy = if telegram {
                " A copy was sent to the owner's Telegram chat."
            } else {
                ""
            };
            let done = if reminder {
                " When the owner says it's done, mark it done; they can also snooze it."
            } else {
                ""
            };
            format!(
                "[{what} \"{name}\" (automation {id}) delivered here.{copy}{done}]\n\n{reply}",
                id = record.id
            )
        }
        MainChatDelivery::Discord {
            label,
            session_id,
            message_id,
        } => {
            metadata.insert("kind".into(), string("automationNote"));
            metadata.insert("delivery".into(), string("discord"));
            metadata.insert("destinationLabel".into(), string(label));
            if let Some(session_id) = session_id {
                metadata.insert("destinationSessionId".into(), string(session_id));
            }
            if let Some(message_id) = message_id {
                metadata.insert("destinationMessageId".into(), string(message_id));
            }
            format!(
                "[{what} \"{name}\" (automation {id}) was posted to Discord {label}, not here. Discord accepting it does not show the owner read it. What was posted:]\n\n{reply}",
                id = record.id
            )
        }
    };
    Message {
        id: format!("automation:{}:{reply_id}", record.id),
        agent_id: record.agent_id.clone(),
        room_id: room_id.to_string(),
        content: Content {
            text,
            attachments: None,
            metadata: Some(metadata),
        },
        role: MessageRole::Assistant,
        created_at_ms: now,
    }
}

/// The copy of a Discord post saved in that channel's web session, so the
/// console's "Open thread" lands on it.
pub(crate) fn destination_copy(
    record: &ScheduledPromptRecord,
    room_id: &str,
    reply_id: &str,
    reply: &str,
    now: u64,
) -> Message {
    Message {
        id: format!("automation-post:{}:{reply_id}", record.id),
        agent_id: record.agent_id.clone(),
        room_id: room_id.to_string(),
        content: Content {
            text: reply.to_string(),
            attachments: None,
            metadata: Some(BTreeMap::from([
                ("kind".to_string(), string("automationPost")),
                ("automationId".to_string(), string(&record.id)),
                ("automationName".to_string(), string(display_name(record))),
            ])),
        },
        role: MessageRole::Assistant,
        created_at_ms: now,
    }
}
