//! Bounded native action evidence, separate from the committed transcript.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionStatus {
    Started,
    Succeeded,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionDurability {
    Persistent,
    Memory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActionReceipt {
    pub(crate) tool_call_id: String,
    pub(crate) name: String,
    pub(crate) status: ActionStatus,
    pub(crate) started_at_ms: u64,
    pub(crate) finished_at_ms: Option<u64>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) arguments_preview: String,
    pub(crate) arguments_truncated: bool,
    pub(crate) result_preview: Option<String>,
    pub(crate) result_truncated: bool,
    pub(crate) durability: ActionDurability,
}

pub(crate) fn validate_receipts(receipts: &[ActionReceipt]) -> Result<(), String> {
    if receipts.len() > crate::live::MAX_LIVE_TOOL_CARDS {
        return Err("run action receipts exceed the call limit".into());
    }
    let mut ids = HashSet::new();
    for receipt in receipts {
        if receipt.tool_call_id.trim().is_empty()
            || receipt.name.trim().is_empty()
            || !ids.insert(&receipt.tool_call_id)
            || receipt.tool_call_id.len() > crate::live::MAX_PREVIEW_BYTES
            || receipt.name.len() > crate::live::MAX_PREVIEW_BYTES
            || receipt.arguments_preview.len() > crate::live::MAX_PREVIEW_BYTES
            || receipt
                .result_preview
                .as_ref()
                .is_some_and(|text| text.len() > crate::live::MAX_PREVIEW_BYTES)
            || receipt.started_at_ms == 0
        {
            return Err("invalid or duplicate run action receipt".into());
        }
        let terminal = receipt.status != ActionStatus::Started;
        if terminal != receipt.finished_at_ms.is_some()
            || terminal != receipt.duration_ms.is_some()
            || terminal != receipt.result_preview.is_some()
            || (!terminal && receipt.result_truncated)
            || receipt
                .finished_at_ms
                .is_some_and(|at| at < receipt.started_at_ms)
        {
            return Err("inconsistent run action receipt outcome".into());
        }
    }
    Ok(())
}
