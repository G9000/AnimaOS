//! Candidate-only mutations; callers publish them only after durable save.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::records::{
    clean_title, digest, next_revision, valid_hash, valid_id, valid_instructions,
    valid_prefixed_uuid, valid_request_key, ProjectCreationReceipt, ProjectError,
    ProjectMemberGrant, ProjectRecord, ProjectResourceRecord, ProjectResourceRole, ProjectStatus,
    MAX_CREATION_RECEIPTS, MAX_MEMBERS, MAX_PROJECTS,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectCreateRequest {
    pub(crate) request_key: String,
    pub(crate) title: String,
    pub(crate) member_agent_ids: Vec<String>,
    #[serde(default)]
    pub(crate) instructions: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectPatchRequest {
    pub(crate) expected_revision: u64,
    pub(crate) title: Option<String>,
    pub(crate) instructions: Option<String>,
    pub(crate) status: Option<ProjectStatus>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ProjectRegistry {
    records: BTreeMap<String, ProjectRecord>,
    creation_receipts: BTreeMap<String, ProjectCreationReceipt>,
}

impl ProjectRegistry {
    pub(crate) fn snapshot(&self) -> Vec<ProjectRecord> {
        self.records.values().cloned().collect()
    }

    pub(crate) fn creation_receipts_snapshot(&self) -> Vec<ProjectCreationReceipt> {
        self.creation_receipts.values().cloned().collect()
    }

    pub(crate) fn get(&self, project_id: &str) -> Option<&ProjectRecord> {
        self.records.get(project_id)
    }

    pub(crate) fn records(&self) -> impl Iterator<Item = &ProjectRecord> {
        self.records.values()
    }

    pub(crate) fn from_snapshot(
        records: Vec<ProjectRecord>,
        receipts: Vec<ProjectCreationReceipt>,
    ) -> Result<Self, ProjectError> {
        if records.len() > MAX_PROJECTS || receipts.len() > MAX_CREATION_RECEIPTS {
            return Err(ProjectError::InvalidRecord(
                "saved project history exceeds its bounds",
            ));
        }
        let mut registry = Self::default();
        for record in records {
            record.validate()?;
            if registry.records.insert(record.id.clone(), record).is_some() {
                return Err(ProjectError::InvalidRecord(
                    "saved projects contain duplicate identities",
                ));
            }
        }
        let mut receipt_projects = BTreeSet::new();
        for receipt in receipts {
            let live = registry.records.get(&receipt.project_id);
            if !valid_request_key(&receipt.request_key)
                || !valid_hash(&receipt.request_sha256)
                || !valid_prefixed_uuid(&receipt.project_id, "project_")
                || receipt
                    .deleted_at_ms
                    .is_some_and(|at| at < receipt.created_at_ms)
                || !receipt_projects.insert(receipt.project_id.clone())
                || match receipt.deleted_at_ms {
                    Some(_) => live.is_some(),
                    None => {
                        live.is_none_or(|project| project.created_at_ms != receipt.created_at_ms)
                    }
                }
                || registry
                    .creation_receipts
                    .contains_key(&receipt.request_key)
            {
                return Err(ProjectError::InvalidRecord(
                    "saved project creation history is invalid",
                ));
            }
            registry
                .creation_receipts
                .insert(receipt.request_key.clone(), receipt);
        }
        if registry
            .records
            .keys()
            .any(|id| !receipt_projects.contains(id))
        {
            return Err(ProjectError::InvalidRecord(
                "saved project is missing its creation receipt",
            ));
        }
        Ok(registry)
    }

    pub(crate) fn normalize_create(
        mut input: ProjectCreateRequest,
    ) -> Result<ProjectCreateRequest, ProjectError> {
        if !valid_request_key(&input.request_key)
            || !valid_instructions(&input.instructions)
            || input.member_agent_ids.is_empty()
            || input.member_agent_ids.len() > MAX_MEMBERS
            || input.member_agent_ids.iter().any(|id| !valid_id(id))
        {
            return Err(ProjectError::InvalidRecord(
                "project members, instructions or request identity are invalid",
            ));
        }
        input.title = clean_title(&input.title)?;
        input.member_agent_ids.sort();
        if input
            .member_agent_ids
            .windows(2)
            .any(|ids| ids[0] == ids[1])
        {
            return Err(ProjectError::InvalidRecord(
                "select each project member once",
            ));
        }
        Ok(input)
    }

    pub(crate) fn create(
        &mut self,
        input: ProjectCreateRequest,
        now_ms: u64,
    ) -> Result<(ProjectRecord, bool), ProjectError> {
        let input = Self::normalize_create(input)?;
        let fingerprint = digest(&input)?;
        if let Some(receipt) = self.creation_receipts.get(&input.request_key) {
            if receipt.request_sha256 != fingerprint {
                return Err(ProjectError::StaleRevision);
            }
            if receipt.deleted_at_ms.is_some() {
                return Err(ProjectError::DeletedCreation);
            }
            return self
                .get(&receipt.project_id)
                .cloned()
                .map(|project| (project, false))
                .ok_or(ProjectError::AccessUnavailable);
        }
        // Capacity is reserved before either record is inserted. Receipts never evict.
        if self.creation_receipts.len() >= MAX_CREATION_RECEIPTS {
            return Err(ProjectError::ReceiptCapacity);
        }
        if self.records.len() >= MAX_PROJECTS {
            return Err(ProjectError::InvalidRecord(
                "remove an existing project before creating another",
            ));
        }
        let record = ProjectRecord {
            id: format!("project_{}", uuid::Uuid::new_v4()),
            title: input.title,
            status: ProjectStatus::Active,
            revision: 1,
            access_epoch: 1,
            members: input
                .member_agent_ids
                .into_iter()
                .map(|agent_id| ProjectMemberGrant {
                    agent_id,
                    generation: 1,
                    joined_at_ms: now_ms,
                })
                .collect(),
            instructions: input.instructions,
            resources: Vec::new(),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        };
        record.validate()?;
        self.creation_receipts.insert(
            input.request_key.clone(),
            ProjectCreationReceipt {
                request_key: input.request_key,
                request_sha256: fingerprint,
                project_id: record.id.clone(),
                created_at_ms: now_ms,
                deleted_at_ms: None,
            },
        );
        self.records.insert(record.id.clone(), record.clone());
        Ok((record, true))
    }

    fn checked(&self, id: &str, expected_revision: u64) -> Result<ProjectRecord, ProjectError> {
        let record = self.get(id).ok_or(ProjectError::NotFound)?;
        if record.revision != expected_revision {
            return Err(ProjectError::StaleRevision);
        }
        Ok(record.clone())
    }

    fn store_changed(
        &mut self,
        mut record: ProjectRecord,
        now_ms: u64,
        access_reduced: bool,
    ) -> Result<ProjectRecord, ProjectError> {
        record.revision = next_revision(record.revision)?;
        if access_reduced {
            record.access_epoch = next_revision(record.access_epoch)?;
        }
        record.updated_at_ms = now_ms.max(record.updated_at_ms);
        record.validate()?;
        self.records.insert(record.id.clone(), record.clone());
        Ok(record)
    }

    pub(crate) fn patch(
        &mut self,
        id: &str,
        input: ProjectPatchRequest,
        now_ms: u64,
    ) -> Result<ProjectRecord, ProjectError> {
        let mut record = self.checked(id, input.expected_revision)?;
        if input.title.is_none() && input.instructions.is_none() && input.status.is_none() {
            return Err(ProjectError::InvalidRecord(
                "choose a project change before saving",
            ));
        }
        if record.status == ProjectStatus::Archived && input.instructions.is_some() {
            return Err(ProjectError::Archived);
        }
        if let Some(title) = input.title {
            record.title = clean_title(&title)?;
        }
        if let Some(instructions) = input.instructions {
            if !valid_instructions(&instructions) {
                return Err(ProjectError::InvalidRecord(
                    "project instructions exceed the allowed length",
                ));
            }
            record.instructions = instructions;
        }
        let status_changed = input.status.is_some_and(|status| status != record.status);
        if let Some(status) = input.status {
            record.status = status;
        }
        self.store_changed(record, now_ms, status_changed)
    }

    pub(crate) fn replace_members(
        &mut self,
        id: &str,
        expected_revision: u64,
        mut agents: Vec<String>,
        now_ms: u64,
    ) -> Result<ProjectRecord, ProjectError> {
        let mut record = self.checked(id, expected_revision)?;
        if agents.len() > MAX_MEMBERS || agents.iter().any(|id| !valid_id(id)) {
            return Err(ProjectError::InvalidRecord(
                "project member selection is invalid",
            ));
        }
        agents.sort();
        if agents.windows(2).any(|ids| ids[0] == ids[1]) {
            return Err(ProjectError::InvalidRecord(
                "select each project member once",
            ));
        }
        let generation = next_revision(record.access_epoch)?;
        let members = agents
            .into_iter()
            .map(|agent_id| {
                record
                    .member(&agent_id)
                    .cloned()
                    .unwrap_or(ProjectMemberGrant {
                        agent_id,
                        generation,
                        joined_at_ms: now_ms,
                    })
            })
            .collect();
        record.members = members;
        self.store_changed(record, now_ms, true)
    }

    pub(crate) fn add_resource(
        &mut self,
        id: &str,
        expected_revision: u64,
        resource: ProjectResourceRecord,
        now_ms: u64,
    ) -> Result<ProjectRecord, ProjectError> {
        resource.validate()?;
        let current = self.get(id).ok_or(ProjectError::NotFound)?;
        if let Some(saved) = current
            .resources
            .iter()
            .find(|row| row.add_replay.request_key == resource.add_replay.request_key)
        {
            return if saved.add_replay.request_sha256 == resource.add_replay.request_sha256 {
                Ok(current.clone())
            } else {
                Err(ProjectError::StaleRevision)
            };
        }
        let mut record = self.checked(id, expected_revision)?;
        if record.status == ProjectStatus::Archived {
            return Err(ProjectError::Archived);
        }
        record.resources.push(resource);
        self.store_changed(record, now_ms, false)
    }

    pub(crate) fn patch_resource(
        &mut self,
        id: &str,
        expected_revision: u64,
        resource_id: &str,
        label: Option<String>,
        role: Option<ProjectResourceRole>,
        use_in_context: Option<bool>,
        now_ms: u64,
    ) -> Result<ProjectRecord, ProjectError> {
        let mut record = self.checked(id, expected_revision)?;
        if record.status == ProjectStatus::Archived {
            return Err(ProjectError::Archived);
        }
        if label.is_none() && role.is_none() && use_in_context.is_none() {
            return Err(ProjectError::InvalidRecord(
                "choose a source change before saving",
            ));
        }
        let resource = record
            .resources
            .iter_mut()
            .find(|row| row.id == resource_id)
            .ok_or(ProjectError::NotFound)?;
        if let Some(label) = label {
            resource.label = if label.trim().is_empty() {
                None
            } else {
                Some(clean_title(&label)?)
            };
        }
        if let Some(role) = role {
            resource.role = role;
        }
        let access_reduced = resource.use_in_context && use_in_context == Some(false);
        if let Some(value) = use_in_context {
            resource.use_in_context = value;
        }
        self.store_changed(record, now_ms, access_reduced)
    }

    pub(crate) fn remove_resource(
        &mut self,
        id: &str,
        expected_revision: u64,
        resource_id: &str,
        now_ms: u64,
    ) -> Result<ProjectRecord, ProjectError> {
        let mut record = self.checked(id, expected_revision)?;
        if record.status == ProjectStatus::Archived {
            return Err(ProjectError::Archived);
        }
        let before = record.resources.len();
        record.resources.retain(|row| row.id != resource_id);
        if record.resources.len() == before {
            return Err(ProjectError::NotFound);
        }
        self.store_changed(record, now_ms, true)
    }

    pub(crate) fn reorder_resources(
        &mut self,
        id: &str,
        expected_revision: u64,
        resource_ids: &[String],
        now_ms: u64,
    ) -> Result<ProjectRecord, ProjectError> {
        let mut record = self.checked(id, expected_revision)?;
        if record.status == ProjectStatus::Archived {
            return Err(ProjectError::Archived);
        }
        if resource_ids.len() != record.resources.len()
            || resource_ids.iter().collect::<BTreeSet<_>>().len() != resource_ids.len()
        {
            return Err(ProjectError::InvalidRecord(
                "refresh the complete source order before saving",
            ));
        }
        let mut resources: BTreeMap<_, _> = record
            .resources
            .into_iter()
            .map(|row| (row.id.clone(), row))
            .collect();
        record.resources = resource_ids
            .iter()
            .map(|id| resources.remove(id).ok_or(ProjectError::NotFound))
            .collect::<Result<_, _>>()?;
        self.store_changed(record, now_ms, false)
    }

    pub(crate) fn delete(
        &mut self,
        id: &str,
        expected_revision: u64,
        now_ms: u64,
    ) -> Result<ProjectRecord, ProjectError> {
        let record = self.checked(id, expected_revision)?;
        next_revision(record.revision)?;
        next_revision(record.access_epoch)?;
        let receipt = self
            .creation_receipts
            .values_mut()
            .find(|row| row.project_id == id)
            .ok_or(ProjectError::AccessUnavailable)?;
        receipt.deleted_at_ms = Some(now_ms.max(receipt.created_at_ms));
        self.records.remove(id);
        Ok(record)
    }

    pub(crate) fn invalidate_sources_for_session(
        &mut self,
        agent: &str,
        session: &str,
        now_ms: u64,
    ) -> Result<Vec<ProjectRecord>, ProjectError> {
        let affected: Vec<_> = self
            .records
            .values()
            .filter(|project| {
                project.resources.iter().any(|source| {
                    source.target.owner_agent_id() == Some(agent)
                        && source.target.session_id() == Some(session)
                })
            })
            .cloned()
            .collect();
        affected
            .into_iter()
            .map(|record| self.store_changed(record, now_ms, true))
            .collect()
    }
}
