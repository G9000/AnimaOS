//! Metadata-only state transitions. The coordinator persists each transition before its effect.
use anima_document_content::{OfficeEditReview, OfficeFormat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MAX_DRAFTS: usize = 8;
const MAX_PATHS: usize = 512;
const MAX_METADATA_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentEditScope {
    pub(crate) profile_key: String,
    pub(crate) workspace_key: String,
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    pub(crate) clear_generation: u64,
    pub(crate) run_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DocumentEditSource {
    Attachment {
        attachment_id: String,
        sha256: String,
    },
    Workspace {
        file_path: String,
        sha256: String,
    },
}
impl DocumentEditSource {
    pub(crate) fn sha256(&self) -> &str {
        match self {
            Self::Attachment { sha256, .. } | Self::Workspace { sha256, .. } => sha256,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ExpectedDestination {
    Absent,
    Present { sha256: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentEditDraft {
    pub(crate) id: String,
    pub(crate) scope: DocumentEditScope,
    pub(crate) source: DocumentEditSource,
    #[serde(default)]
    pub(crate) attachment_record: Option<crate::attachments::AttachmentRecord>,
    pub(crate) file_path: String,
    pub(crate) expected_destination: ExpectedDestination,
    pub(crate) candidate_sha256: String,
    pub(crate) candidate_bytes: u64,
    pub(crate) operations_sha256: String,
    pub(crate) content_review: OfficeEditReview,
    pub(crate) created_at_ms: u64,
    pub(crate) expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DocumentEditStatus {
    Prepared,
    Applying {
        approval_id: String,
        tool_call_id: String,
    },
    RecoveryRequired {
        approval_id: String,
        tool_call_id: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DocumentPublicationPhase {
    #[default]
    Reserved,
    Publishing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentEditRecord {
    pub(crate) draft: DocumentEditDraft,
    pub(crate) review_sha256: String,
    pub(crate) status: DocumentEditStatus,
    #[serde(default)]
    pub(crate) owner_approval: Option<super::approval::DocumentEditOwnerApproval>,
    #[serde(default)]
    pub(crate) publication_phase: DocumentPublicationPhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentPublicationReceipt {
    pub(crate) proposal_id: String,
    pub(crate) profile_key: String,
    pub(crate) workspace_key: String,
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    pub(crate) clear_generation: u64,
    pub(crate) file_path: String,
    pub(crate) sha256: String,
    pub(crate) size_bytes: u64,
    pub(crate) format: OfficeFormat,
    pub(crate) source: DocumentEditSource,
    pub(crate) expected_destination: ExpectedDestination,
    pub(crate) review_sha256: String,
    pub(crate) run_id: String,
    pub(crate) approval_id: String,
    pub(crate) tool_call_id: String,
    pub(crate) checkpoint_id: Option<String>,
    pub(crate) published_at_ms: u64,
}

/// The host control plane owns this value; model inputs never deserialize it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentEditRegistry {
    drafts: BTreeMap<String, DocumentEditRecord>,
    publications: BTreeMap<String, DocumentPublicationReceipt>,
    reservations: BTreeMap<String, String>,
}

pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
fn valid_call_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && path.trim() == path
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|p| {
            !matches!(p, "" | "." | "..")
                && !crate::tools::generated_documents::excluded_component(p)
        })
        && !path
            .split('/')
            .next()
            .is_some_and(|p| p.eq_ignore_ascii_case("uploads"))
}
fn publication_key(scope: &DocumentEditScope, path: &str) -> String {
    let bytes = serde_json::to_vec(&(
        &scope.profile_key,
        &scope.workspace_key,
        path.to_lowercase(),
    ))
    .expect("string tuple serializes");
    hex::encode(Sha256::digest(bytes))
}
pub(crate) fn review_digest(draft: &DocumentEditDraft) -> Result<String, String> {
    let payload =
        serde_jcs::to_vec(draft).map_err(|_| "Document review cannot be canonicalized")?;
    let mut digest = Sha256::new();
    digest.update(b"anima-imported-document-review-v1\0");
    digest.update(payload);
    Ok(hex::encode(digest.finalize()))
}

impl DocumentEditDraft {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !valid_id(&self.id)
            || !valid_hash(self.source.sha256())
            || !valid_hash(&self.candidate_sha256)
            || !valid_hash(&self.operations_sha256)
            || !valid_path(&self.file_path)
            || self.candidate_bytes == 0
            || self.candidate_bytes > 4 * 1024 * 1024
            || self.expires_at_ms <= self.created_at_ms
            || self.expires_at_ms.saturating_sub(self.created_at_ms) > 30 * 60 * 1000
            || self.content_review.version != 1
            || self.content_review.before.format != self.content_review.after.format
            || !self.content_review.preservation.unchanged_bytes_verified
        {
            return Err("Document proposal metadata is invalid".into());
        }
        for value in [
            &self.scope.profile_key,
            &self.scope.workspace_key,
            &self.scope.agent_id,
            &self.scope.session_id,
            &self.scope.run_id,
        ] {
            if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
                return Err("Document proposal scope is invalid".into());
            }
        }
        let expected_extension = match self.content_review.before.format {
            OfficeFormat::Docx => "docx",
            OfficeFormat::Xlsx => "xlsx",
            OfficeFormat::Pptx => "pptx",
        };
        if self
            .file_path
            .rsplit_once('.')
            .map(|(_, x)| x.to_ascii_lowercase())
            .as_deref()
            != Some(expected_extension)
        {
            return Err("Document destination format does not match".into());
        }
        if let ExpectedDestination::Present { sha256 } = &self.expected_destination {
            if !valid_hash(sha256) {
                return Err("Document expected destination hash is invalid".into());
            }
        }
        match &self.source {
            DocumentEditSource::Attachment { attachment_id, .. } if !valid_id(attachment_id) => {
                return Err("Document attachment identity is invalid".into())
            }
            DocumentEditSource::Workspace { file_path, .. } if !valid_path(file_path) => {
                return Err("Document source path is invalid".into())
            }
            _ => {}
        }
        match (&self.source, &self.attachment_record) {
            (
                DocumentEditSource::Attachment {
                    attachment_id,
                    sha256,
                },
                Some(record),
            ) if record.id == *attachment_id
                && record.sha256 == *sha256
                && record.agent_id == self.scope.agent_id
                && record.session_id == self.scope.session_id => {}
            (DocumentEditSource::Workspace { .. }, None) => {}
            _ => return Err("Document source custody is invalid".into()),
        }
        if serde_json::to_vec(&self.content_review)
            .map_err(|_| "Document review cannot be encoded")?
            .len()
            > 64 * 1024
        {
            return Err("Document review is too large".into());
        }
        Ok(())
    }
}

impl DocumentEditRegistry {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.drafts.len() > MAX_DRAFTS || self.publications.len() > MAX_PATHS {
            return Err("Document registry capacity is invalid".into());
        }
        let mut expected_reservations = BTreeMap::new();
        for (id, record) in &self.drafts {
            record.draft.validate()?;
            if id != &record.draft.id || record.review_sha256 != review_digest(&record.draft)? {
                return Err("Document proposal seal is invalid".into());
            }
            match &record.status {
                DocumentEditStatus::Prepared => {
                    if record.owner_approval.is_some()
                        || record.publication_phase != DocumentPublicationPhase::Reserved
                    {
                        return Err("Prepared document cannot retain approval authority".into());
                    }
                }
                DocumentEditStatus::Applying {
                    approval_id,
                    tool_call_id,
                }
                | DocumentEditStatus::RecoveryRequired {
                    approval_id,
                    tool_call_id,
                } => {
                    if !valid_id(approval_id)
                        || !valid_call_id(tool_call_id)
                        || !record.owner_approval.as_ref().is_some_and(|proof| {
                            proof.id() == approval_id
                                && proof.matches(&record.draft, &record.review_sha256, tool_call_id)
                        })
                        || expected_reservations
                            .insert(
                                publication_key(&record.draft.scope, &record.draft.file_path),
                                id.clone(),
                            )
                            .is_some()
                    {
                        return Err("Document applying reservation is invalid".into());
                    }
                }
            }
        }
        if expected_reservations != self.reservations {
            return Err("Document publication reservations do not match applying intents".into());
        }
        for (key, receipt) in &self.publications {
            let scope = DocumentEditScope {
                profile_key: receipt.profile_key.clone(),
                workspace_key: receipt.workspace_key.clone(),
                agent_id: String::new(),
                session_id: String::new(),
                clear_generation: 0,
                run_id: receipt.run_id.clone(),
            };
            if !valid_id(&receipt.proposal_id)
                || !valid_hash(&receipt.sha256)
                || !valid_hash(receipt.source.sha256())
                || !valid_hash(&receipt.review_sha256)
                || !valid_path(&receipt.file_path)
                || !valid_id(&receipt.approval_id)
                || !valid_call_id(&receipt.tool_call_id)
                || [
                    &receipt.profile_key,
                    &receipt.workspace_key,
                    &receipt.agent_id,
                    &receipt.session_id,
                    &receipt.run_id,
                ]
                .iter()
                .any(|value| {
                    value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control)
                })
                || receipt
                    .checkpoint_id
                    .as_ref()
                    .is_some_and(|value| !valid_id(value))
                || key != &publication_key(&scope, &receipt.file_path)
                || receipt.size_bytes == 0
                || receipt.size_bytes > 4 * 1024 * 1024
            {
                return Err("Document publication receipt is invalid".into());
            }
            if let ExpectedDestination::Present { sha256 } = &receipt.expected_destination {
                if !valid_hash(sha256) {
                    return Err("Document publication preimage is invalid".into());
                }
            }
            let expected_extension = match receipt.format {
                OfficeFormat::Docx => "docx",
                OfficeFormat::Xlsx => "xlsx",
                OfficeFormat::Pptx => "pptx",
            };
            if receipt
                .file_path
                .rsplit_once('.')
                .map(|(_, extension)| extension.to_ascii_lowercase())
                .as_deref()
                != Some(expected_extension)
            {
                return Err("Document publication format is invalid".into());
            }
        }
        self.check_capacity()?;
        if serde_json::to_vec(self)
            .map_err(|_| "Document registry cannot be encoded")?
            .len()
            > MAX_METADATA_BYTES
        {
            return Err("Document registry metadata limit exceeded".into());
        }
        Ok(())
    }
    fn check_capacity(&self) -> Result<(), String> {
        let occupied = self
            .publications
            .keys()
            .chain(self.reservations.keys())
            .collect::<BTreeSet<_>>()
            .len();
        if occupied > MAX_PATHS {
            Err("Document publication receipt capacity is full".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn insert(
        &mut self,
        draft: DocumentEditDraft,
        now_ms: u64,
    ) -> Result<String, String> {
        draft.validate()?;
        if draft.created_at_ms > now_ms
            || now_ms >= draft.expires_at_ms
            || self.drafts.len() >= MAX_DRAFTS
            || self.drafts.contains_key(&draft.id)
            || self
                .publications
                .values()
                .any(|p| p.proposal_id == draft.id)
        {
            return Err("Document proposal is expired, duplicate or at capacity".into());
        }
        let digest = review_digest(&draft)?;
        let id = draft.id.clone();
        self.drafts.insert(
            id.clone(),
            DocumentEditRecord {
                draft,
                review_sha256: digest.clone(),
                status: DocumentEditStatus::Prepared,
                owner_approval: None,
                publication_phase: DocumentPublicationPhase::Reserved,
            },
        );
        if let Err(error) = self.validate() {
            self.drafts.remove(&id);
            return Err(error);
        }
        Ok(digest)
    }
    pub(crate) fn get(&self, id: &str) -> Option<&DocumentEditRecord> {
        self.drafts.get(id)
    }
    pub(crate) fn records(&self) -> impl Iterator<Item = &DocumentEditRecord> {
        self.drafts.values()
    }
    /// Retains native intent only while bounded publication recovery needs it.
    pub(crate) fn pending_native_scopes(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.drafts.values().filter_map(|record| {
            matches!(
                record.status,
                DocumentEditStatus::Applying { .. } | DocumentEditStatus::RecoveryRequired { .. }
            )
            .then(|| {
                let scope = &record.draft.scope;
                (
                    scope.run_id.as_str(),
                    scope.agent_id.as_str(),
                    scope.session_id.as_str(),
                )
            })
        })
    }
    pub(crate) fn receipt_for_proposal(&self, id: &str) -> Option<&DocumentPublicationReceipt> {
        self.publications
            .values()
            .find(|receipt| receipt.proposal_id == id)
    }
    pub(crate) fn references(&self, hash: &str) -> bool {
        self.drafts
            .values()
            .any(|record| record.draft.candidate_sha256 == hash)
    }
    pub(crate) fn expire_prepared(&mut self, now_ms: u64) {
        self.drafts.retain(|_, record| {
            record.status != DocumentEditStatus::Prepared || now_ms < record.draft.expires_at_ms
        });
    }
    /// Revocation is irreversible for these proposals even if a grant is later re-added.
    pub(crate) fn invalidate_agent(&mut self, agent_id: &str) {
        self.drafts.retain(|_, record| {
            record.draft.scope.agent_id != agent_id || record.status != DocumentEditStatus::Prepared
        });
        for record in self
            .drafts
            .values_mut()
            .filter(|record| record.draft.scope.agent_id == agent_id)
        {
            if let DocumentEditStatus::Applying {
                approval_id,
                tool_call_id,
            } = &record.status
            {
                record.status = DocumentEditStatus::RecoveryRequired {
                    approval_id: approval_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                };
            }
        }
    }

    /// The caller must save this transition durably before admitting any file effect.
    /// On save failure restore the coordinator's previous registry snapshot.
    pub(crate) fn begin_apply(
        &mut self,
        id: &str,
        review_sha256: &str,
        scope: &DocumentEditScope,
        owner_approval: &super::approval::DocumentEditOwnerApproval,
        tool_call_id: &str,
        now_ms: u64,
    ) -> Result<(), String> {
        let record = self
            .drafts
            .get(id)
            .ok_or("Document proposal is unavailable")?;
        if record.status != DocumentEditStatus::Prepared
            || &record.draft.scope != scope
            || record.review_sha256 != review_sha256
            || now_ms >= record.draft.expires_at_ms
            || !valid_id(owner_approval.id())
            || !valid_call_id(tool_call_id)
            || !owner_approval.matches(&record.draft, review_sha256, tool_call_id)
        {
            return Err("Document proposal approval is stale".into());
        }
        let key = publication_key(scope, &record.draft.file_path);
        if self.reservations.contains_key(&key) {
            return Err("An earlier document publication reserved this destination; it must settle or be reconciled before another write".into());
        }
        if !self.publications.contains_key(&key)
            && self
                .publications
                .keys()
                .chain(self.reservations.keys())
                .collect::<BTreeSet<_>>()
                .len()
                >= MAX_PATHS
        {
            return Err("Document publication receipt capacity is full".into());
        }
        self.reservations.insert(key.clone(), id.into());
        let record = self
            .drafts
            .get_mut(id)
            .ok_or("Document proposal is unavailable")?;
        record.owner_approval = Some(owner_approval.clone());
        record.status = DocumentEditStatus::Applying {
            approval_id: owner_approval.id().into(),
            tool_call_id: tool_call_id.into(),
        };
        if let Err(error) = self.validate() {
            self.reservations.remove(&key);
            if let Some(record) = self.drafts.get_mut(id) {
                record.status = DocumentEditStatus::Prepared;
                record.owner_approval = None;
            }
            return Err(error);
        }
        Ok(())
    }
    pub(crate) fn reserved_apply(
        &self,
        id: &str,
        review_sha256: &str,
        scope: &DocumentEditScope,
        tool_call_id: &str,
        now_ms: u64,
    ) -> bool {
        self.drafts.get(id).is_some_and(|record| &record.draft.scope == scope && record.review_sha256 == review_sha256 && now_ms < record.draft.expires_at_ms
            && matches!(&record.status, DocumentEditStatus::Applying { tool_call_id: current, .. } if current == tool_call_id)
            && self.reservations.get(&publication_key(scope, &record.draft.file_path)).map(String::as_str) == Some(id))
    }
    /// Save this transition before entering any native file publication boundary.
    pub(crate) fn begin_publication(&mut self, id: &str, call_id: &str) -> Result<(), String> {
        let record = self
            .drafts
            .get_mut(id)
            .ok_or("Document applying intent is unavailable")?;
        if record.publication_phase != DocumentPublicationPhase::Reserved
            || !matches!(&record.status,DocumentEditStatus::Applying{tool_call_id,..} if tool_call_id==call_id)
        {
            return Err("Document publication phase changed".into());
        }
        record.publication_phase = DocumentPublicationPhase::Publishing;
        self.validate()
    }
    /// Called only after native file/journal readback proves the exact candidate effect.
    pub(crate) fn confirm_publication(
        &mut self,
        id: &str,
        tool_call_id: &str,
        observed_sha256: &str,
        checkpoint_id: Option<String>,
        now_ms: u64,
    ) -> Result<DocumentPublicationReceipt, String> {
        let record = self
            .drafts
            .get(id)
            .ok_or("Document proposal is unavailable")?;
        let (approval_id, current) = match &record.status {
            DocumentEditStatus::Applying {
                approval_id,
                tool_call_id,
            }
            | DocumentEditStatus::RecoveryRequired {
                approval_id,
                tool_call_id,
            } => (approval_id, tool_call_id),
            _ => return Err("Document proposal has no applying intent".into()),
        };
        let draft = &record.draft;
        if record.publication_phase != DocumentPublicationPhase::Publishing {
            return Err("Document publication was never admitted".into());
        }
        let key = publication_key(&draft.scope, &draft.file_path);
        if current != tool_call_id
            || observed_sha256 != draft.candidate_sha256
            || self.reservations.get(&key).map(String::as_str) != Some(id)
        {
            return Err("Document publication proof does not match its reservation".into());
        }
        let receipt = DocumentPublicationReceipt {
            proposal_id: id.into(),
            profile_key: draft.scope.profile_key.clone(),
            workspace_key: draft.scope.workspace_key.clone(),
            agent_id: draft.scope.agent_id.clone(),
            session_id: draft.scope.session_id.clone(),
            clear_generation: draft.scope.clear_generation,
            file_path: draft.file_path.clone(),
            sha256: draft.candidate_sha256.clone(),
            size_bytes: draft.candidate_bytes,
            format: draft.content_review.before.format,
            source: draft.source.clone(),
            expected_destination: draft.expected_destination.clone(),
            review_sha256: record.review_sha256.clone(),
            run_id: draft.scope.run_id.clone(),
            approval_id: approval_id.clone(),
            tool_call_id: tool_call_id.into(),
            checkpoint_id,
            published_at_ms: now_ms,
        };
        self.publications.insert(key.clone(), receipt.clone());
        self.reservations.remove(&key);
        self.drafts.remove(id);
        Ok(receipt)
    }
    /// Remove a prepared draft, or an applying draft only after proving no file effect occurred.
    pub(crate) fn cancel(
        &mut self,
        id: &str,
        no_effect_proven: bool,
    ) -> Result<Option<DocumentEditDraft>, String> {
        let Some(record) = self.drafts.get(id) else {
            return Ok(None);
        };
        if record.status != DocumentEditStatus::Prepared && !no_effect_proven {
            return Err("Document revision needs effect reconciliation before cancellation".into());
        }
        let key = publication_key(&record.draft.scope, &record.draft.file_path);
        if self.reservations.get(&key).map(String::as_str) == Some(id) {
            self.reservations.remove(&key);
        }
        Ok(self.drafts.remove(id).map(|r| r.draft))
    }
    /// Expire prepared authority; retain applying reservations for hash/journal reconciliation.
    pub(crate) fn recover_after_restart(&mut self) -> Vec<String> {
        self.drafts
            .retain(|_, record| record.status != DocumentEditStatus::Prepared);
        let mut recovery = Vec::new();
        for (id, record) in &mut self.drafts {
            if let DocumentEditStatus::Applying {
                approval_id,
                tool_call_id,
            } = &record.status
            {
                record.status = DocumentEditStatus::RecoveryRequired {
                    approval_id: approval_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                };
                recovery.push(id.clone());
            }
        }
        recovery
    }
    pub(crate) fn publication(
        &self,
        profile_key: &str,
        workspace_key: &str,
        file_path: &str,
        sha256: &str,
    ) -> Option<&DocumentPublicationReceipt> {
        self.publications.values().find(|p| {
            p.profile_key == profile_key
                && p.workspace_key == workspace_key
                && p.file_path == file_path
                && p.sha256 == sha256
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anima_document_content::{OfficeChange, OfficeInventory, OfficePreservation};
    fn draft(id: &str, path: &str) -> DocumentEditDraft {
        let inventory = OfficeInventory {
            version: 1,
            format: OfficeFormat::Docx,
            targets: vec![],
            retained_hyperlinks: vec![],
            limitations: vec![],
        };
        DocumentEditDraft {
            id: id.into(),
            scope: DocumentEditScope {
                profile_key: "profile".into(),
                workspace_key: "workspace".into(),
                agent_id: "agent".into(),
                session_id: "session".into(),
                clear_generation: 1,
                run_id: "run".into(),
            },
            source: DocumentEditSource::Workspace {
                file_path: "original.docx".into(),
                sha256: "a".repeat(64),
            },
            attachment_record: None,
            file_path: path.into(),
            expected_destination: ExpectedDestination::Absent,
            candidate_sha256: "b".repeat(64),
            candidate_bytes: 100,
            operations_sha256: "c".repeat(64),
            content_review: OfficeEditReview {
                version: 1,
                before: inventory.clone(),
                after: inventory,
                changes: vec![OfficeChange {
                    operation_index: Some(0),
                    label: "Body".into(),
                    before: "Old".into(),
                    after: "New".into(),
                    derived: false,
                }],
                preservation: OfficePreservation {
                    unchanged_parts: 2,
                    edited_parts: 1,
                    unchanged_bytes_verified: true,
                    container_normalized: true,
                },
            },
            created_at_ms: 1,
            expires_at_ms: 1000,
        }
    }
    impl DocumentEditRegistry {
        fn test_begin_apply(
            &mut self,
            id: &str,
            hash: &str,
            scope: &DocumentEditScope,
            approval_id: &str,
            call_id: &str,
            now: u64,
        ) -> Result<(), String> {
            use crate::approvals::{
                ApprovalDecisionKind, ApprovalRequest, ApprovalResolution, ApprovalStatus,
                PendingApprovalStart, ResolvedBy,
            };
            use anima_core::{DataValue, ToolCall};
            let draft = &self.get(id).ok_or("missing draft")?.draft;
            let value = crate::tools::generated_documents::schema_data(
                serde_json::json!({"proposal_id":id,"review_sha256":hash,"file_path":draft.file_path,"expected_destination":draft.expected_destination}),
            );
            let DataValue::Object(args) = value else {
                unreachable!()
            };
            let call = ToolCall {
                id: call_id.into(),
                name: "apply_document_edit".into(),
                args,
            };
            let witness = super::super::approval::capture(self, scope, &call, now)?
                .ok_or("missing witness")?;
            let projected = witness.approval_call(&call);
            let mut record = ApprovalRequest::pending(
                PendingApprovalStart {
                    agent_id: &scope.agent_id,
                    session_id: &scope.session_id,
                    run_id: &scope.run_id,
                    call: &projected,
                    timeout_ms: 1000,
                    reason: None,
                },
                now,
            );
            record.id = approval_id.into();
            record.resolve(
                ApprovalStatus::Allowed,
                ApprovalResolution {
                    decision: Some(ApprovalDecisionKind::AllowOnce),
                    note: None,
                    matcher: None,
                    rule_id: None,
                    resolved_by: ResolvedBy::Owner,
                    resolved_at_ms: now,
                },
            );
            let proof = witness.resolved_approval(&call, &record)?;
            self.begin_apply(id, hash, scope, &proof, call_id, now)?;
            self.begin_publication(id, call_id)
        }
    }
    fn fill(registry: &mut DocumentEditRegistry, count: usize) {
        for n in 0..count {
            let draft = draft(&format!("existing-{n}"), &format!("published-{n}.docx"));
            let hash = registry.insert(draft.clone(), 1).unwrap();
            registry
                .test_begin_apply(&draft.id, &hash, &draft.scope, "approval", "call", 2)
                .unwrap();
            registry
                .confirm_publication(&draft.id, "call", &draft.candidate_sha256, None, 3)
                .unwrap();
        }
    }
    #[test]
    fn document_edits_full_receipt_registry_refuses_before_apply_reservation() {
        let mut registry = DocumentEditRegistry::default();
        fill(&mut registry, MAX_PATHS);
        let draft = draft("new", "new.docx");
        let hash = registry.insert(draft.clone(), 1).unwrap();
        assert!(registry
            .test_begin_apply("new", &hash, &draft.scope, "approval", "call", 2)
            .is_err());
        assert!(!registry.reserved_apply("new", &hash, &draft.scope, "call", 2));
        assert_eq!(
            registry.get("new").unwrap().status,
            DocumentEditStatus::Prepared
        );
    }
    #[test]
    fn document_edits_competing_proposals_cannot_share_last_receipt_slot() {
        let mut registry = DocumentEditRegistry::default();
        fill(&mut registry, MAX_PATHS - 1);
        let one = draft("one", "one.docx");
        let two = draft("two", "two.docx");
        let hash_one = registry.insert(one.clone(), 1).unwrap();
        let hash_two = registry.insert(two.clone(), 1).unwrap();
        registry
            .test_begin_apply("one", &hash_one, &one.scope, "approval", "call-one", 2)
            .unwrap();
        assert!(registry
            .test_begin_apply("two", &hash_two, &two.scope, "approval", "call-two", 2)
            .is_err());
        assert!(!registry.reserved_apply("two", &hash_two, &two.scope, "call-two", 2));
        registry.validate().unwrap();
    }
    #[test]
    fn document_edits_same_path_is_exclusive_and_restart_never_replays_it() {
        let mut registry = DocumentEditRegistry::default();
        let one = draft("one", "same.docx");
        let two = draft("two", "SAME.docx");
        let hash_one = registry.insert(one.clone(), 1).unwrap();
        let hash_two = registry.insert(two.clone(), 1).unwrap();
        registry
            .test_begin_apply("one", &hash_one, &one.scope, "approval", "call-one", 2)
            .unwrap();
        assert!(registry
            .test_begin_apply("two", &hash_two, &two.scope, "approval", "call-two", 2)
            .is_err());
        assert_eq!(registry.recover_after_restart(), vec!["one"]);
        assert!(!registry.reserved_apply("one", &hash_one, &one.scope, "call-one", 2));
        assert!(registry.cancel("one", false).is_err());
        registry.validate().unwrap();
    }
    #[test]
    fn document_edits_cancelling_a_prepared_competitor_keeps_the_applying_path_reservation() {
        let mut registry = DocumentEditRegistry::default();
        let one = draft("one", "same.docx");
        let two = draft("two", "SAME.docx");
        let hash = registry.insert(one.clone(), 1).unwrap();
        registry.insert(two, 1).unwrap();
        registry
            .test_begin_apply("one", &hash, &one.scope, "approval", "call-one", 2)
            .unwrap();
        registry.cancel("two", false).unwrap();
        registry.validate().unwrap();
        assert!(registry.reserved_apply("one", &hash, &one.scope, "call-one", 2));
        registry
            .confirm_publication("one", "call-one", &one.candidate_sha256, None, 3)
            .unwrap();
        registry.validate().unwrap();
    }
    #[test]
    fn document_edits_unsaved_reservation_rollback_grants_no_native_effect() {
        let mut registry = DocumentEditRegistry::default();
        let draft = draft("one", "one.docx");
        let hash = registry.insert(draft.clone(), 1).unwrap();
        let before = registry.clone();
        registry
            .test_begin_apply("one", &hash, &draft.scope, "approval", "call", 2)
            .unwrap();
        registry = before;
        assert!(!registry.reserved_apply("one", &hash, &draft.scope, "call", 2));
    }
    #[test]
    fn document_edits_revocation_cannot_be_undone_by_regranting_the_same_scope() {
        let mut registry = DocumentEditRegistry::default();
        let prepared = draft("prepared", "prepared.docx");
        registry.insert(prepared, 1).unwrap();
        let applying = draft("applying", "applying.docx");
        let hash = registry.insert(applying.clone(), 1).unwrap();
        registry
            .test_begin_apply("applying", &hash, &applying.scope, "approval", "call", 2)
            .unwrap();
        registry.invalidate_agent("agent");
        assert!(registry.get("prepared").is_none());
        assert!(matches!(
            registry.get("applying").unwrap().status,
            DocumentEditStatus::RecoveryRequired { .. }
        ));
        assert!(!registry.reserved_apply("applying", &hash, &applying.scope, "call", 2));
        assert!(registry
            .test_begin_apply(
                "applying",
                &hash,
                &applying.scope,
                "approval-again",
                "call-again",
                2
            )
            .is_err());
        registry.validate().unwrap();
    }
    #[test]
    fn document_edits_seal_detects_review_or_destination_tampering() {
        let mut registry = DocumentEditRegistry::default();
        let draft = draft("one", "one.docx");
        registry.insert(draft, 1).unwrap();
        registry.drafts.get_mut("one").unwrap().draft.file_path = "different.docx".into();
        assert!(registry.validate().is_err());
    }
}
