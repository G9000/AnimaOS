use super::*;
use async_trait::async_trait;
use std::sync::Weak;
use tokio::sync::RwLock;

pub(super) struct NativeReader {
    pub(super) state: Weak<RwLock<DaemonState>>,
    pub(super) mail: crate::connectors::mail::MailManager,
    pub(super) drive: crate::connectors::drive::DriveManager,
    pub(super) calendar: crate::connectors::gcalendar::CalendarManager,
}
#[async_trait]
impl super::service::SourceReader for NativeReader {
    async fn read(
        &self,
        a: &SourceAuthority,
    ) -> Result<super::service::SourceSnapshot, SearchError> {
        match a.provider.as_str() {
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
