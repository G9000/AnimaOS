use super::*;
use async_trait::async_trait;
use std::sync::Weak;
use tokio::sync::RwLock;

pub(super) struct NativeReader {
    pub(super) state: Weak<RwLock<DaemonState>>,
    pub(super) mail: crate::connectors::mail::MailManager,
    pub(super) drive: crate::connectors::drive::DriveManager,
    pub(super) calendar: crate::connectors::gcalendar::CalendarManager,
    pub(super) microsoft_files: crate::connectors::microsoft_files::MicrosoftFileManager,
}
#[async_trait]
impl super::service::SourceReader for NativeReader {
    async fn attachments(
        &self,
        authority: &SourceAuthority,
        parent: &IndexedDocument,
    ) -> Result<AttachmentBatch, SearchError> {
        if matches!(authority.provider.as_str(), "onedrive" | "sharepoint") {
            let state = self.state.upgrade().ok_or(SearchError::Unavailable)?;
            let generation = {
                let state = state.read().await;
                if !source_authority(&state, &authority.agent_id, &authority.connector_id)
                    .is_some_and(|current| {
                        current.enabled && current.generation == authority.generation
                    })
                {
                    return Err(SearchError::Conflict);
                }
                state
                    .microsoft_file_records
                    .get(&authority.connector_id)
                    .ok_or(SearchError::Conflict)?
                    .generation
                    .clone()
            };
            let value = self
                .microsoft_files
                .read_selected(
                    authority.agent_id.clone(),
                    authority.connector_id.clone(),
                    generation,
                    parent.external_id.clone(),
                )
                .await
                .map_err(|_| SearchError::Unavailable)?;
            let mut current = document(
                DocumentKind::File,
                parent.external_id.clone(),
                "selected".into(),
                value.metadata.name.clone(),
                String::new(),
            );
            current.revision = value.metadata.revision.unwrap_or_default();
            current.source_modified_at = value.metadata.modified_at;
            if normalize(current, authority, 0)?.revision != parent.revision {
                return Err(SearchError::Conflict);
            }
            return Ok(AttachmentBatch {
                items: vec![RawAttachment {
                    id: "selected-document".into(),
                    name: value.metadata.name,
                    mime: value.metadata.mime.unwrap_or_default(),
                    bytes: value.bytes,
                    status: value.status,
                }],
                omitted: 0,
            });
        }
        if authority.provider == "gdrive" {
            let state = self.state.upgrade().ok_or(SearchError::Unavailable)?;
            let generation = {
                let state = state.read().await;
                if !source_authority(&state, &authority.agent_id, &authority.connector_id)
                    .is_some_and(|current| {
                        current.enabled && current.generation == authority.generation
                    })
                {
                    return Err(SearchError::Conflict);
                }
                state
                    .drive_records
                    .get(&authority.connector_id)
                    .ok_or(SearchError::Conflict)?
                    .generation
                    .clone()
            };
            let value = self
                .drive
                .read_binary_for_search(&authority.agent_id, &parent.external_id, &generation)
                .await
                .map_err(|_| SearchError::Unavailable)?;
            let mut current = document(
                DocumentKind::File,
                parent.external_id.clone(),
                "selected".into(),
                value.metadata.name.clone(),
                String::new(),
            );
            current.revision = value.metadata.version.clone();
            current.source_modified_at = value.metadata.modified_time.clone();
            if normalize(current, authority, 0)?.revision != parent.revision {
                return Err(SearchError::Conflict);
            }
            let status = if value.bytes.is_some() {
                "ready"
            } else if value
                .metadata
                .size_bytes
                .is_some_and(|size| size > 2 * 1024 * 1024)
            {
                "tooLarge"
            } else {
                "unsupported"
            };
            return Ok(AttachmentBatch {
                items: vec![RawAttachment {
                    id: "selected-document".into(),
                    name: value.metadata.name,
                    mime: value.metadata.mime_type,
                    bytes: value.bytes,
                    status: status.into(),
                }],
                omitted: 0,
            });
        }
        self.mail
            .search_attachments(
                &authority.agent_id,
                &authority.connector_id,
                &authority.generation,
                &parent.external_id,
            )
            .await
            .map_err(|_| SearchError::Unavailable)
    }
    async fn inbox_membership(
        &self,
        authority: &SourceAuthority,
        ids: &[String],
    ) -> Result<Vec<bool>, SearchError> {
        self.mail
            .search_inbox_membership(
                &authority.agent_id,
                &authority.connector_id,
                &authority.generation,
                ids,
            )
            .await
            .map_err(|_| SearchError::Unavailable)
    }
    async fn read_history(
        &self,
        authority: &SourceAuthority,
        request: &HistoryPageRequest,
    ) -> Result<HistoryPage, SearchError> {
        self.mail
            .search_history_page(
                &authority.agent_id,
                &authority.connector_id,
                &authority.generation,
                request,
            )
            .await
            .map_err(|_| SearchError::Unavailable)
    }

    async fn read(
        &self,
        a: &SourceAuthority,
    ) -> Result<super::service::SourceSnapshot, SearchError> {
        match a.provider.as_str() {
            "onedrive" | "sharepoint" => {
                let state = self.state.upgrade().ok_or(SearchError::Unavailable)?;
                let record = {
                    let state = state.read().await;
                    state
                        .microsoft_file_records
                        .get(&a.connector_id)
                        .filter(|record| record.agent_id == a.agent_id)
                        .cloned()
                        .ok_or(SearchError::Conflict)?
                };
                let mut documents = Vec::new();
                let mut incomplete_count = 0;
                for file in &record.selected_files {
                    let metadata = match self.microsoft_files.metadata_selected(a.agent_id.clone(), a.connector_id.clone(), record.generation.clone(), file.key.clone()).await {
                        Ok(value) => value,
                        Err(crate::connectors::microsoft_files::MicrosoftFileError::NotFound | crate::connectors::microsoft_files::MicrosoftFileError::PermissionDenied) => { incomplete_count += 1; continue; },
                        Err(_) => return Err(SearchError::Unavailable),
                    };
                    let mut document = document(
                        DocumentKind::File,
                        file.key.clone(),
                        "selected".into(),
                        metadata.name,
                        String::new(),
                    );
                    document.revision = metadata.revision.unwrap_or_default();
                    document.source_modified_at = metadata.modified_at;
                    document.source_url = metadata.source_url;
                    documents.push(document);
                }
                Ok(super::service::SourceSnapshot {
                    documents,
                    stream_epoch: None,
                    selection_revision: Some(record.selection_revision),
                    incomplete_count,
                })
            }
            "gmail" | "outlook" | "email" => self
                .mail
                .search_documents(&a.agent_id, &a.connector_id, &a.generation)
                .await
                .map_err(|_| SearchError::Unavailable),
            "gcalendar" => self
                .calendar
                .search_documents(&a.agent_id, &a.connector_id, &a.generation)
                .await
                .map(Into::into)
                .map_err(|_| SearchError::Unavailable),
            "gdrive" => {
                let state = self.state.upgrade().ok_or(SearchError::Unavailable)?;
                let (ids, generation) = {
                    let s = state.read().await;
                    let record = s
                        .drive_records
                        .get(&a.connector_id)
                        .filter(|r| r.agent_id == a.agent_id)
                        .ok_or(SearchError::Conflict)?;
                    (record.selected_file_ids.clone(), record.generation.clone())
                };
                let mut docs = vec![];
                for id in ids {
                    use crate::connectors::drive::model::{DriveError, DriveReadResult};
                    let result = self
                        .drive
                        .read_for_search(&a.agent_id, id.as_str(), &generation)
                        .await;
                    let (metadata, text, truncated) = match result {
                        Ok(DriveReadResult::Text(r)) => (r.metadata, r.text, r.truncated),
                        Ok(DriveReadResult::Unsupported { metadata, .. }) => {
                            (metadata, String::new(), false)
                        }
                        Err(DriveError::NotFound | DriveError::PermissionDenied) => continue,
                        Err(DriveError::ReauthRequired) => {
                            let generation = {
                                let s = state.read().await;
                                if source_authority(&s, &a.agent_id, &a.connector_id)
                                    .is_some_and(|current| current.generation == a.generation)
                                {
                                    s.drive_records
                                        .get(&a.connector_id)
                                        .map(|r| r.generation.clone())
                                } else {
                                    None
                                }
                            };
                            if let Some(generation) = generation {
                                let _ = self
                                    .drive
                                    .require_search_reauthorization(
                                        a.agent_id.clone(),
                                        a.connector_id.clone(),
                                        generation,
                                    )
                                    .await;
                            }
                            return Err(SearchError::Unavailable);
                        }
                        Err(_) => return Err(SearchError::Unavailable),
                    };
                    docs.push(IndexedDocument {
                        citation_id: String::new(),
                        external_id: id.to_string(),
                        collection_id: "selected".into(),
                        kind: DocumentKind::File,
                        title: metadata.name,
                        text,
                        sender: None,
                        source_url: Some(metadata.source_url),
                        source_modified_at: metadata.modified_time,
                        event_start: None,
                        event_end: None,
                        revision: metadata.version,
                        truncated,
                        indexed_at_ms: 0,
                    });
                }
                Ok(docs.into())
            }
            _ => Err(SearchError::Unavailable),
        }
    }
}

pub(crate) fn document(
    kind: DocumentKind,
    id: String,
    collection: String,
    title: String,
    text: String,
) -> IndexedDocument {
    IndexedDocument {
        citation_id: String::new(),
        external_id: id,
        collection_id: collection,
        kind,
        title,
        text,
        sender: None,
        source_url: None,
        source_modified_at: None,
        event_start: None,
        event_end: None,
        revision: String::new(),
        truncated: false,
        indexed_at_ms: 0,
    }
}
