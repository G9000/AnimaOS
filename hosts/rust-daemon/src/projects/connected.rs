//! Explicit selected-account grants and native, generation-checked read integration.

#[cfg(test)]
#[path = "connected_tests.rs"]
mod tests;

use anima_document_store_sqlite::connected::{ConnectedSourceGrant, ConnectedSourceKind};
use serde::{Deserialize, Serialize};

use super::{ProjectError, ProjectResourceTarget};
use crate::app::SharedDaemonState;
use crate::state::DaemonState;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AddConnectedSourceRequest {
    pub(crate) expected_revision: u64,
    pub(crate) request_key: String,
    pub(crate) as_agent_id: String,
    pub(crate) owner_agent_id: String,
    pub(crate) connector_id: String,
    pub(crate) citation_id: String,
    pub(crate) share_with_project: bool,
    pub(crate) label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectedChoice {
    owner_agent_id: String,
    owner_name: String,
    connector_id: String,
    provider: String,
    citation_id: String,
    remote_id: String,
    title: String,
    account_label: Option<String>,
    source_kind: ConnectedSourceKind,
    coverage: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectedChoices {
    choices: Vec<ConnectedChoice>,
    omitted_count: usize,
}

pub(crate) struct ConnectedSnapshot {
    pub(crate) name: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) source_sha256: String,
    pub(crate) partial: bool,
}

/// Resolve an existing account citation without allowing remote identifiers to grant authority.
pub(crate) fn capture(
    state: &DaemonState,
    owner: &str,
    connector: &str,
    citation: &str,
) -> Result<ConnectedSourceGrant, ProjectError> {
    let authority = crate::connected_search::source_authority(state, owner, connector)
        .filter(|authority| authority.enabled && authority.read_granted)
        .ok_or(ProjectError::AccessUnavailable)?;
    let index = state
        .connected_search
        .sources
        .get(connector)
        .filter(|index| {
            index.agent_id == owner && index.generation == authority.generation && index.enabled
        })
        .ok_or(ProjectError::AccessUnavailable)?;
    let document = index
        .documents
        .iter()
        .chain(index.historical.values())
        .find(|document| document.citation_id == citation)
        .ok_or(ProjectError::NotFound)?;
    let (connection_generation, selection_revision, source_kind) = match authority.provider.as_str()
    {
        "gdrive" => {
            let record = state
                .drive_records
                .get(connector)
                .ok_or(ProjectError::AccessUnavailable)?;
            if !record
                .selected_file_ids
                .iter()
                .any(|id| id.as_str() == document.external_id)
            {
                return Err(ProjectError::AccessRevoked);
            }
            (record.generation.clone(), 0, ConnectedSourceKind::File)
        }
        "onedrive" | "sharepoint" => {
            let record = state
                .microsoft_file_records
                .get(connector)
                .ok_or(ProjectError::AccessUnavailable)?;
            if !record
                .selected_files
                .iter()
                .any(|file| file.key == document.external_id)
            {
                return Err(ProjectError::AccessRevoked);
            }
            (
                record.generation.clone(),
                record.selection_revision,
                ConnectedSourceKind::File,
            )
        }
        "gmail" | "outlook" | "email" => {
            let record = state
                .mail_records
                .get(connector)
                .ok_or(ProjectError::AccessUnavailable)?;
            (
                record.auth_generation.clone(),
                0,
                ConnectedSourceKind::EmailThread,
            )
        }
        _ => {
            return Err(ProjectError::InvalidRecord(
                "Choose a selected file or mail conversation",
            ))
        }
    };
    let grant = ConnectedSourceGrant {
        owner_agent_id: owner.into(),
        connector_id: connector.into(),
        provider: authority.provider,
        remote_id: document.external_id.clone(),
        citation_id: citation.into(),
        connection_generation,
        search_generation: authority.generation,
        grant_epoch: index.grant_epoch.clone(),
        selection_revision,
        stream_epoch: index.stream_epoch.clone(),
        source_kind,
    };
    let expected_kind = match grant.source_kind {
        ConnectedSourceKind::File => crate::connected_search::DocumentKind::File,
        ConnectedSourceKind::EmailThread => crate::connected_search::DocumentKind::Mail,
    };
    if !grant.is_valid() || document.kind != expected_kind {
        return Err(ProjectError::AccessUnavailable);
    }
    Ok(grant)
}

pub(crate) fn check(state: &DaemonState, grant: &ConnectedSourceGrant) -> Result<(), ProjectError> {
    state
        .ensure_profile_owned()
        .map_err(|_| ProjectError::AccessUnavailable)?;
    let current = capture(
        state,
        &grant.owner_agent_id,
        &grant.connector_id,
        &grant.citation_id,
    )?;
    if &current != grant {
        return Err(ProjectError::AccessRevoked);
    }
    Ok(())
}

pub(crate) async fn choices(
    state: &SharedDaemonState,
    project: &str,
    member: &str,
    query: &str,
) -> Result<ConnectedChoices, ProjectError> {
    if query.chars().count() > 256 || query.chars().any(char::is_control) {
        return Err(ProjectError::InvalidRecord(
            "Use a search of at most 256 characters",
        ));
    }
    let state = state.read().await;
    let project = state.projects.get(project).ok_or(ProjectError::NotFound)?;
    let lease = state.project_access.acquire_read(project, member)?;
    state
        .ensure_profile_owned()
        .map_err(|_| ProjectError::AccessUnavailable)?;
    let query = query.trim().to_lowercase();
    let mut choices = Vec::new();
    let mut omitted_count = 0;
    for (connector, index) in &state.connected_search.sources {
        if project.member(&index.agent_id).is_none() {
            continue;
        }
        for document in index.documents.iter().chain(index.historical.values()) {
            if !query.is_empty() && !document.title.to_lowercase().contains(&query) {
                continue;
            }
            let Ok(grant) = capture(&state, &index.agent_id, connector, &document.citation_id)
            else {
                continue;
            };
            if choices.len() >= 60 {
                omitted_count += 1;
                continue;
            }
            let account_label = state
                .drive_records
                .get(connector)
                .and_then(|r| r.account_label.clone())
                .or_else(|| {
                    state
                        .microsoft_file_records
                        .get(connector)
                        .and_then(|r| r.account_label.clone())
                })
                .or_else(|| {
                    state
                        .mail_records
                        .get(connector)
                        .and_then(|r| r.connector.account_label.clone())
                });
            choices.push(ConnectedChoice {
                owner_agent_id: index.agent_id.clone(),
                owner_name: state
                    .agents
                    .get(&index.agent_id)
                    .map(|agent| agent.config().name.clone())
                    .unwrap_or_else(|| "Account owner".into()),
                connector_id: connector.clone(),
                provider: grant.provider,
                citation_id: document.citation_id.clone(),
                remote_id: document.external_id.clone(),
                title: document.title.clone(),
                account_label,
                source_kind: grant.source_kind,
                coverage: if grant.source_kind == ConnectedSourceKind::File {
                    "One selected file; supported text only"
                } else if state.mail_records.get(connector).is_some_and(|r| {
                    r.connector.provider == crate::connectors::mail::Provider::Email
                }) {
                    "Selected retained IMAP message only; full thread unavailable"
                } else {
                    "Selected conversation; up to 25 messages and 1 MiB text; attachments omitted"
                },
            });
        }
    }
    choices.sort_by(|a, b| {
        (&a.owner_agent_id, &a.connector_id, &a.citation_id).cmp(&(
            &b.owner_agent_id,
            &b.connector_id,
            &b.citation_id,
        ))
    });
    lease.check()?;
    Ok(ConnectedChoices {
        choices,
        omitted_count,
    })
}

pub(crate) async fn read(
    state: &SharedDaemonState,
    grant: &ConnectedSourceGrant,
) -> Result<ConnectedSnapshot, ProjectError> {
    let (drive, microsoft, mail) = {
        let state = state.read().await;
        check(&state, grant)?;
        (
            state.drive_manager.as_ref().and_then(|m| m.upgrade()),
            state
                .microsoft_files_manager
                .as_ref()
                .and_then(|m| m.upgrade()),
            state.mail_manager.as_ref().and_then(|m| m.upgrade()),
        )
    };
    let (name, bytes, revision, partial) = match grant.provider.as_str() {
        "gdrive" => {
            let manager = drive.ok_or(ProjectError::AccessUnavailable)?;
            match manager
                .read_retained_for_project(
                    &grant.owner_agent_id,
                    &grant.remote_id,
                    &grant.connection_generation,
                )
                .await
                .map_err(|_| ProjectError::AccessUnavailable)?
            {
                crate::connectors::drive::model::DriveReadResult::Text(value) => (
                    format!("{}.txt", value.metadata.name),
                    value.text.into_bytes(),
                    value.metadata.version,
                    value.truncated,
                ),
                crate::connectors::drive::model::DriveReadResult::Unsupported { .. } => {
                    let value = manager
                        .read_binary_for_search(
                            &grant.owner_agent_id,
                            &grant.remote_id,
                            &grant.connection_generation,
                        )
                        .await
                        .map_err(|_| ProjectError::AccessUnavailable)?;
                    (
                        value.metadata.name,
                        value.bytes.ok_or(ProjectError::AccessUnavailable)?,
                        value.metadata.version,
                        false,
                    )
                }
            }
        }
        "onedrive" | "sharepoint" => {
            let value = microsoft
                .ok_or(ProjectError::AccessUnavailable)?
                .read_selected(
                    grant.owner_agent_id.clone(),
                    grant.connector_id.clone(),
                    grant.connection_generation.clone(),
                    grant.remote_id.clone(),
                )
                .await
                .map_err(|_| ProjectError::AccessUnavailable)?;
            if value.selection_revision != grant.selection_revision {
                return Err(ProjectError::AccessRevoked);
            }
            (
                value.metadata.name,
                value.bytes.ok_or(ProjectError::AccessUnavailable)?,
                value.metadata.revision.unwrap_or_default(),
                value.status != "ready",
            )
        }
        "gmail" | "outlook" | "email" => {
            let value = mail
                .ok_or(ProjectError::AccessUnavailable)?
                .read_project_thread(
                    &grant.owner_agent_id,
                    &grant.connector_id,
                    &grant.search_generation,
                    &grant.remote_id,
                    grant.stream_epoch.as_deref(),
                )
                .await
                .map_err(|_| ProjectError::AccessUnavailable)?;
            (
                format!("{}.txt", value.title),
                value.text.into_bytes(),
                value.revision,
                value.partial,
            )
        }
        _ => return Err(ProjectError::AccessUnavailable),
    };
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(ProjectError::AccessUnavailable);
    }
    let mut name = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':') {
                '_'
            } else {
                c
            }
        })
        .collect::<String>();
    while name.len() > 512 {
        name.pop();
    }
    if name.trim().is_empty() {
        name = "Selected document.txt".into();
    }
    let source_sha256 = super::records::digest(&(
        &grant.remote_id,
        &revision,
        anima_document_store_sqlite::source_sha256(&bytes),
    ))?;
    check(&*state.read().await, grant)?;
    Ok(ConnectedSnapshot {
        name,
        bytes,
        source_sha256,
        partial,
    })
}

pub(crate) fn is_knowledge(target: &ProjectResourceTarget) -> bool {
    matches!(
        target,
        ProjectResourceTarget::KnowledgeFolder { .. }
            | ProjectResourceTarget::ConnectedSource { .. }
    )
}

/// Called inside native account persistence before publishing a revoked account snapshot.
pub(crate) fn reconcile_access(state: &DaemonState) {
    for project in state.projects.records() {
        let revoked = project
            .resources
            .iter()
            .filter_map(|resource| {
                let ProjectResourceTarget::ConnectedSource { grant } = &resource.target else {
                    return None;
                };
                check(state, grant).is_err().then_some(resource.id.clone())
            })
            .collect();
        state
            .project_access
            .publish_source_revocations(&project.id, revoked);
    }
}
