//! Accepted Telegram owner inputs are reconstructed from the scoped ledger.
use super::ConnectorManagerError;
use crate::app::SharedDaemonState;
use anima_core::{Content, DataValue};
use std::collections::BTreeMap;

pub(super) async fn owner_content(
    state: &SharedDaemonState,
    agent_id: &str,
    room_id: &str,
    run_id: Option<&str>,
    text: String,
    metadata: BTreeMap<String, DataValue>,
) -> Result<Content, ConnectorManagerError> {
    match run_id {
        Some(run_id) => crate::agent_runs::attachments::accepted_content(
            state, agent_id, room_id, run_id, metadata,
        )
        .await
        .map_err(|_| ConnectorManagerError::Persistence),
        None => Ok(Content {
            text,
            metadata: Some(metadata),
            attachments: None,
        }),
    }
}
