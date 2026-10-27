use super::*;
impl DriveManager {
    pub(crate) async fn require_search_reauthorization(
        &self,
        agent: String,
        id: String,
        generation: String,
    ) -> Result<(), DriveError> {
        let manager = self.clone();
        self.owned(async move {
            let shared = manager.shared()?;
            let capture = {
                let s = shared.read().await;
                s.drive_records
                    .get(&id)
                    .filter(|r| r.agent_id == agent && r.generation == generation)
                    .cloned()
                    .ok_or(DriveError::Conflict)?
            };
            let mut next = capture.clone();
            next.status = DriveStatus::ReauthRequired;
            next.updated_at_ms = now_ms();
            manager.edit_record(Some(capture), next, Some(false)).await
        })
        .await
    }
    async fn capture(&self, agent: &str, tool: &str) -> Result<DriveRecord, DriveError> {
        let shared = self.shared()?;
        let capture = shared
            .read()
            .await
            .drive_records
            .values()
            .find(|r| r.agent_id == agent && r.status == DriveStatus::Active)
            .cloned()
            .ok_or(DriveError::NotConnected)?;
        self.current(&capture, Some(tool)).await?;
        Ok(capture)
    }
    pub(crate) async fn search_for_agent(
        &self,
        agent: &str,
        query: &str,
    ) -> Result<DriveSearchResult, DriveError> {
        let query = query.trim();
        if query.chars().count() > 256 {
            return Err(DriveError::InvalidInput);
        }
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            self.search_inner(agent, query),
        )
        .await
        .map_err(|_| DriveError::Deadline)?
    }
    async fn search_inner(
        &self,
        agent: &str,
        query: &str,
    ) -> Result<DriveSearchResult, DriveError> {
        let capture = self.capture(agent, "drive_search").await?;
        let grant = self
            .access_grant(capture.clone(), "drive_search".into())
            .await?;
        self.current(&capture, Some("drive_search")).await?;
        let lease = self
            .inner
            .oauth_apps
            .locked_config(OAuthProvider::Google)
            .await
            .map_err(|_| DriveError::Credential)?;
        if lease.revision() != capture.oauth_config_revision {
            return Err(DriveError::Conflict);
        }
        let mut files = Vec::new();
        for id in &capture.selected_file_ids {
            let fetched = self
                .inner
                .transport
                .metadata(grant.tokens.access_token(), id)
                .await;
            self.current(&capture, Some("drive_search")).await?;
            match fetched {
                Ok(metadata) => {
                    if metadata.id != *id {
                        return Err(DriveError::InvalidResponse);
                    }
                    if metadata.trashed {
                        files.push(DriveSearchItem {
                            file_id: id.clone(),
                            metadata: None,
                            availability: DriveFileAvailability::Unavailable,
                            reason: Some("selected file is unavailable".into()),
                        });
                    } else if metadata.name.to_lowercase().contains(&query.to_lowercase()) {
                        let supported = supported(&metadata);
                        files.push(DriveSearchItem {
                            file_id: id.clone(),
                            metadata: Some(metadata),
                            availability: if supported {
                                DriveFileAvailability::Available
                            } else {
                                DriveFileAvailability::Unsupported
                            },
                            reason: if supported {
                                None
                            } else {
                                Some("format is unsupported; metadata only".into())
                            },
                        });
                    }
                }
                Err(DriveError::NotFound | DriveError::PermissionDenied) => {
                    files.push(DriveSearchItem {
                        file_id: id.clone(),
                        metadata: None,
                        availability: DriveFileAvailability::Unavailable,
                        reason: Some("selected file is unavailable".into()),
                    })
                }
                Err(error) => return Err(error),
            }
        }
        self.current(&capture, Some("drive_search")).await?;
        let result = DriveSearchResult {
            files,
            searched_selected_count: capture.selected_file_ids.len(),
            search_scope: "selected_names".into(),
            fetched_at_ms: now_ms(),
        };
        if serde_json::to_vec(&result)
            .map_err(|_| DriveError::InvalidResponse)?
            .len()
            > 32 * 1024
        {
            return Err(DriveError::TooLarge);
        }
        Ok(result)
    }
    pub(crate) async fn read_for_agent(
        &self,
        agent: &str,
        file: &str,
    ) -> Result<DriveReadResult, DriveError> {
        let id = DriveFileId::parse(file)?;
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            self.read_inner(agent, id, None),
        )
        .await
        .map_err(|_| DriveError::Deadline)?
    }
    pub(crate) async fn read_for_search(
        &self,
        agent: &str,
        file: &str,
        generation: &str,
    ) -> Result<DriveReadResult, DriveError> {
        let id = DriveFileId::parse(file)?;
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            self.read_inner(agent, id, Some(generation)),
        )
        .await
        .map_err(|_| DriveError::Deadline)?
    }
    async fn read_inner(
        &self,
        agent: &str,
        id: DriveFileId,
        expected_generation: Option<&str>,
    ) -> Result<DriveReadResult, DriveError> {
        let capture = self.capture(agent, "drive_read").await?;
        if expected_generation.is_some_and(|expected| capture.generation != expected) {
            return Err(DriveError::Conflict);
        }
        if !capture.selected_file_ids.contains(&id) {
            return Err(DriveError::NotSelected);
        } // before vault or provider access
        let grant = self
            .access_grant(capture.clone(), "drive_read".into())
            .await?;
        self.current(&capture, Some("drive_read")).await?;
        let lease = self
            .inner
            .oauth_apps
            .locked_config(OAuthProvider::Google)
            .await
            .map_err(|_| DriveError::Credential)?;
        if lease.revision() != capture.oauth_config_revision {
            return Err(DriveError::Conflict);
        }
        let metadata = self
            .inner
            .transport
            .metadata(grant.tokens.access_token(), &id)
            .await?;
        self.current(&capture, Some("drive_read")).await?;
        if metadata.id != id {
            return Err(DriveError::InvalidResponse);
        }
        if metadata.trashed {
            return Err(DriveError::NotFound);
        }
        if !supported(&metadata) {
            return Ok(DriveReadResult::Unsupported {
                metadata,
                reason: "format is unsupported; metadata only".into(),
            });
        }
        if !metadata.can_download {
            return Err(DriveError::PermissionDenied);
        }
        if metadata.size_bytes.is_some_and(|size| size > 1_048_576) {
            return Err(DriveError::TooLarge);
        }
        let content = self
            .inner
            .transport
            .read_text(grant.tokens.access_token(), &metadata)
            .await?;
        self.current(&capture, Some("drive_read")).await?;
        if content.bytes.len() > 1_048_576 {
            return Err(DriveError::TooLarge);
        }
        let text = std::str::from_utf8(&content.bytes).map_err(|_| DriveError::InvalidUtf8)?;
        let after = self
            .inner
            .transport
            .metadata(grant.tokens.access_token(), &id)
            .await?;
        self.current(&capture, Some("drive_read")).await?;
        if after.id != metadata.id
            || after.version != metadata.version
            || after.modified_time != metadata.modified_time
            || after.mime_type != metadata.mime_type
            || after.trashed
            || !after.can_download
        {
            return Err(DriveError::Conflict);
        }
        let returned = text.chars().take(32768).collect::<String>();
        let result = DriveTextResult {
            metadata,
            text: returned.clone(),
            representation: content.representation,
            source_bytes: content.bytes.len(),
            returned_bytes: returned.len(),
            returned_scalars: returned.chars().count(),
            truncated: returned.len() < content.bytes.len(),
            source_bytes_sha256: hex::encode(Sha256::digest(&content.bytes)),
            returned_text_sha256: hex::encode(Sha256::digest(returned.as_bytes())),
            fetched_at_ms: now_ms(),
        };
        self.current(&capture, Some("drive_read")).await?;
        Ok(DriveReadResult::Text(result))
    }
}
fn supported(metadata: &DriveFileMetadata) -> bool {
    matches!(
        metadata.mime_type.as_str(),
        "application/vnd.google-apps.document" | "text/plain"
    )
}
