//! Explicit limits and durable progress for owner-enabled account search.
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub(super) const MAX_IMPORTED_ITEMS: usize = 2_000;
pub(super) const MAX_HISTORY_PAGE_ITEMS: usize = 20;
pub(super) const MAX_CURSOR_BYTES: usize = 8 * 1024;
pub(super) const MAX_HISTORY_CURSORS: usize = 512;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum HistoryDepth {
    #[default]
    Recent,
    Days30,
    Days90,
    Days365,
}

impl HistoryDepth {
    pub(super) fn days(self) -> Option<u64> {
        match self {
            Self::Recent => None,
            Self::Days30 => Some(30),
            Self::Days90 => Some(90),
            Self::Days365 => Some(365),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AttachmentTextMode {
    #[default]
    MetadataOnly,
    SupportedText,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SearchSettings {
    pub(crate) history_depth: HistoryDepth,
    pub(crate) retained_item_limit: u16,
    pub(crate) attachment_text: AttachmentTextMode,
    pub(crate) history_paused: bool,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            history_depth: HistoryDepth::Recent,
            retained_item_limit: 100,
            attachment_text: AttachmentTextMode::MetadataOnly,
            history_paused: false,
        }
    }
}

impl SearchSettings {
    pub(super) fn valid_for(&self, provider: &str) -> bool {
        matches!(self.retained_item_limit, 100 | 500 | 2_000)
            && (self.history_depth == HistoryDepth::Recent
                || matches!(provider, "gmail" | "outlook"))
            && (self.attachment_text == AttachmentTextMode::MetadataOnly
                || matches!(
                    provider,
                    "gmail" | "outlook" | "gdrive" | "onedrive" | "sharepoint"
                ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum ImportCursor {
    Gmail { page_token: String },
    Outlook { next_link: String },
}

impl ImportCursor {
    pub(super) fn valid_for(&self, provider: &str) -> bool {
        let value = match (self, provider) {
            (Self::Gmail { page_token }, "gmail") => page_token,
            (Self::Outlook { next_link }, "outlook") => next_link,
            _ => return false,
        };
        !value.is_empty() && value.len() <= MAX_CURSOR_BYTES && !value.chars().any(char::is_control)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum HistoryStatus {
    #[default]
    NotRequested,
    Pending,
    Running,
    Paused,
    Complete,
    Partial,
    Unsupported,
    Error,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HistoryCoverage {
    pub(crate) state: HistoryStatus,
    pub(crate) requested_from_ms: Option<u64>,
    pub(crate) requested_before_ms: Option<u64>,
    pub(crate) reason: Option<String>,
    pub(crate) enumerated_count: usize,
    pub(crate) retained_count: usize,
    pub(crate) oldest_retained_at: Option<String>,
    pub(crate) last_attempt_at_ms: Option<u64>,
    pub(crate) last_success_at_ms: Option<u64>,
    pub(crate) verified_at_ms: Option<u64>,
    pub(crate) retry_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HistoryProgress {
    pub(crate) coverage: HistoryCoverage,
    pub(crate) cursor: Option<ImportCursor>,
    pub(crate) pass_id: String,
    #[serde(default)]
    pub(crate) verification_cursor: Option<String>,
    #[serde(default)]
    pub(crate) cursor_fingerprints: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryPageRequest {
    pub(crate) from_ms: u64,
    pub(crate) before_ms: u64,
    pub(crate) cursor: Option<ImportCursor>,
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryPage {
    pub(crate) documents: Vec<super::IndexedDocument>,
    pub(crate) next_cursor: Option<ImportCursor>,
    pub(crate) exhausted: bool,
}

pub(super) fn default_settings_revision() -> u64 {
    1
}
