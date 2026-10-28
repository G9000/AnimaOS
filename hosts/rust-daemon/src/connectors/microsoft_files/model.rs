//! Persisted Microsoft file authority; tokens live only in the credential store.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MicrosoftFileMode {
    OneDrive,
    SharePoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MicrosoftFileStatus {
    Pairing,
    Active,
    ReauthRequired,
    Disconnected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MicrosoftSelectedFile {
    pub(crate) key: String,
    pub(crate) drive_id: String,
    pub(crate) item_id: String,
    pub(crate) name: String,
    pub(crate) site_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MicrosoftFileRecord {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) mode: MicrosoftFileMode,
    pub(crate) status: MicrosoftFileStatus,
    pub(crate) generation: String,
    pub(crate) selection_revision: u64,
    pub(crate) oauth_config_revision: u64,
    pub(crate) account_id: Option<String>,
    pub(crate) account_label: Option<String>,
    pub(crate) approved_site_id: Option<String>,
    pub(crate) approved_site_url: Option<String>,
    pub(crate) selected_files: Vec<MicrosoftSelectedFile>,
    pub(crate) created_at_ms: u64,
    pub(crate) updated_at_ms: u64,
    #[serde(default)]
    pub(crate) cleanup_generations: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MicrosoftFileError {
    Invalid,
    NotFound,
    Conflict,
    Unconfigured,
    Reauthorization,
    PermissionDenied,
    Unavailable,
    Capacity,
    Persistence,
    Credential,
}
impl std::fmt::Display for MicrosoftFileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Invalid => "Invalid Microsoft files request",
            Self::NotFound => "Microsoft file connection was not found",
            Self::Conflict => "Microsoft file connection changed; refresh and try again",
            Self::Unconfigured => "Microsoft account setup is unavailable",
            Self::Reauthorization => "Reconnect Microsoft files to renew access",
            Self::PermissionDenied => "This file or approved site is unavailable",
            Self::Unavailable => "Microsoft files are temporarily unavailable",
            Self::Capacity => "Microsoft file selection or storage limit reached",
            Self::Persistence => "Microsoft file connection could not be saved",
            Self::Credential => "Microsoft file credential storage is unavailable",
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MicrosoftFileMetadata {
    pub(crate) drive_id: String,
    pub(crate) item_id: String,
    pub(crate) name: String,
    pub(crate) mime: Option<String>,
    pub(crate) size: Option<u64>,
    pub(crate) modified_at: Option<String>,
    pub(crate) revision: Option<String>,
    pub(crate) source_url: Option<String>,
    pub(crate) is_folder: bool,
}

fn bounded(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

pub(crate) fn valid_site_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none()
        && url.query().is_none()
        && url
            .host_str()
            .is_some_and(|host| host.ends_with(".sharepoint.com") && host != ".sharepoint.com")
}

/// Validate restored connector metadata before publishing it to daemon state.
pub(crate) fn validate_records<'a>(
    records: &[MicrosoftFileRecord],
    agent_ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), String> {
    if records.len() > 128 {
        return Err("Microsoft file connector limit exceeded".into());
    }
    let agents = agent_ids.into_iter().collect::<HashSet<_>>();
    let mut ids = HashSet::new();
    let mut active = HashSet::new();
    for record in records {
        let mut keys = HashSet::new();
        let mut targets = HashSet::new();
        if !bounded(&record.id, 128)
            || !ids.insert(&record.id)
            || !bounded(&record.agent_id, 128)
            || !bounded(&record.generation, 128)
            || record.selection_revision == 0
            || record.created_at_ms == 0
            || record.updated_at_ms < record.created_at_ms
            || record.selected_files.len() > 16
            || record.cleanup_generations.len() > 16
            || record
                .cleanup_generations
                .iter()
                .any(|value| !bounded(value, 128))
            || record
                .cleanup_generations
                .iter()
                .collect::<HashSet<_>>()
                .len()
                != record.cleanup_generations.len()
            || record
                .account_label
                .as_ref()
                .is_some_and(|value| !bounded(value, 320))
            || record
                .account_id
                .as_ref()
                .is_some_and(|value| !bounded(value, 1024))
            || record.selected_files.iter().any(|file| {
                !bounded(&file.key, 128)
                    || !bounded(&file.drive_id, 1024)
                    || !bounded(&file.item_id, 1024)
                    || !bounded(&file.name, 2048)
                    || !keys.insert(&file.key)
                    || !targets.insert((&file.drive_id, &file.item_id))
                    || (record.mode == MicrosoftFileMode::SharePoint
                        && file.site_id != record.approved_site_id)
                    || (record.mode == MicrosoftFileMode::OneDrive && file.site_id.is_some())
            })
            || (record.status != MicrosoftFileStatus::Disconnected
                && (!agents.contains(record.agent_id.as_str())
                    || !active.insert((&record.agent_id, record.mode))))
            || (record.status == MicrosoftFileStatus::Active && record.account_id.is_none())
            || (record.mode == MicrosoftFileMode::SharePoint
                && record.status == MicrosoftFileStatus::Active
                && (record
                    .approved_site_id
                    .as_ref()
                    .is_none_or(|value| !bounded(value, 1024))
                    || record
                        .approved_site_url
                        .as_ref()
                        .is_none_or(|value| !valid_site_url(value))))
            || (record.mode == MicrosoftFileMode::OneDrive
                && (record.approved_site_id.is_some() || record.approved_site_url.is_some()))
        {
            return Err("Invalid Microsoft file connector snapshot".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_record() -> MicrosoftFileRecord {
        MicrosoftFileRecord {
            id: "connection".into(),
            agent_id: "agent".into(),
            mode: MicrosoftFileMode::OneDrive,
            status: MicrosoftFileStatus::Active,
            generation: "generation".into(),
            selection_revision: 1,
            oauth_config_revision: 0,
            account_id: Some("account".into()),
            account_label: None,
            approved_site_id: None,
            approved_site_url: None,
            selected_files: vec![],
            created_at_ms: 1,
            updated_at_ms: 1,
            cleanup_generations: vec![],
        }
    }

    #[test]
    fn restores_environment_configured_connections_and_rejects_invalid_authority() {
        let record = active_record();
        assert!(validate_records(&[record.clone()], ["agent"]).is_ok());
        assert!(validate_records(&[record.clone()], ["other-agent"]).is_err());
        let mut invalid = record;
        invalid.mode = MicrosoftFileMode::SharePoint;
        invalid.approved_site_id = Some("site".into());
        invalid.approved_site_url = Some("https://example.com/site".into());
        assert!(validate_records(&[invalid], ["agent"]).is_err());
    }
}
