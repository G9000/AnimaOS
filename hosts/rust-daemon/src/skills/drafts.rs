//! Skill drafts (spec §8.2): a companion's proposal, an imported file, or a
//! `SKILL.md` found without a record, each waiting for the owner. Only the
//! owner's approval writes `SKILL.md` and pins its hash; a file draft is
//! approved only at the hash the owner reviewed.
//!
//! A file draft is read and checked before the control-plane transaction is
//! taken (a slow or stuck folder never holds it up); the hash pinned is that
//! of the bytes read, or of the bytes written when the owner edited the body,
//! so a file that moves on meanwhile reads `changed` (fail closed).

use std::collections::HashSet;

use tracing::warn;

use super::disk;
use super::registry::{is_hex_hash, DraftView, ScannedFile, SkillRegistry};
use super::service::{blocking, Change, SkillError, SkillService};
use super::{
    compose_skill_file, is_valid_slug, parse_skill_file, slugify, validate_body,
    validate_description, validate_name, DraftSource, DraftStatus, ProposedBy, SkillDraft,
    SkillFile, SkillRecord, FILE_DRAFT_ID_PREFIX, MAX_PENDING_DRAFTS_PER_AGENT,
    MAX_PENDING_IMPORT_DRAFTS, SKILL_DRAFT_DECIDED, SKILL_FILE_UNREVIEWED, SKILL_HASH_MISMATCH,
    SKILL_HASH_REQUIRED, SKILL_SLUG_INVALID, TOO_MANY_IMPORT_DRAFTS, TOO_MANY_PENDING_DRAFTS,
};

/// A companion's `propose_skill` call (spec §8.3).
#[derive(Clone, Debug)]
pub(crate) struct Proposal {
    pub(crate) files: Vec<super::SkillSupportingFile>,
    pub(crate) by: ProposedBy,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) body: String,
    pub(crate) slug: Option<String>,
}

/// The owner's approval: an edited body, and for a file draft the hash of
/// the `SKILL.md` they reviewed.
#[derive(Clone, Debug, Default)]
pub(crate) struct DraftApproval {
    pub(crate) files: Option<Vec<super::SkillSupportingFile>>,
    pub(crate) body: Option<String>,
    pub(crate) hash: Option<String>,
}

/// What approving a draft made.
#[derive(Clone, Debug)]
pub(crate) struct ApprovedDraft {
    pub(crate) skill: SkillRecord,
    pub(crate) draft: SkillDraft,
}

fn checked_file(name: &str, description: &str, body: String) -> Result<SkillFile, SkillError> {
    let file = SkillFile {
        name: validate_name(name).map_err(SkillError::invalid)?,
        description: validate_description(description).map_err(SkillError::invalid)?,
        body,
    };
    validate_body(&file.body).map_err(SkillError::invalid)?;
    Ok(file)
}

/// The given slug, or one derived from the name.
fn draft_slug(slug: Option<&str>, name: &str) -> Result<String, SkillError> {
    match slug.map(str::trim).filter(|slug| !slug.is_empty()) {
        Some(slug) if is_valid_slug(slug) => Ok(slug.to_string()),
        Some(_) => Err(SkillError::invalid(SKILL_SLUG_INVALID)),
        None => slugify(name).ok_or_else(|| SkillError::invalid(SKILL_SLUG_INVALID)),
    }
}

/// Stores a new pending draft (the caller checked its cap).
fn add_draft(skills: &mut SkillRegistry, draft: SkillDraft, now_ms: u64) -> Change<SkillDraft> {
    skills.put_draft(draft.clone());
    skills.prune_decided(now_ms);
    let (undo_id, draft_id) = (draft.id.clone(), draft.id.clone());
    Change {
        slug: Some(draft.slug.clone()),
        value: draft,
        undo: Box::new(move |skills: &mut SkillRegistry| {
            skills.remove_draft(&undo_id);
        }),
        write: None,
        draft_id: Some(draft_id),
    }
}

/// Drops pending proposals whose agent no longer exists (besides
/// `proposer`, who is proposing now), so agents that come and go cannot
/// grow the registry past the per-agent cap. Like `prune_decided`, this is
/// not undone by a failed save.
fn prune_orphaned(skills: &mut SkillRegistry, live: &HashSet<String>, proposer: &str) -> usize {
    let orphaned: Vec<String> = skills
        .pending_drafts()
        .into_iter()
        .filter(|draft| {
            draft.source == DraftSource::Agent
                && draft
                    .proposed_by
                    .as_ref()
                    .is_some_and(|by| by.agent_id != proposer && !live.contains(&by.agent_id))
        })
        .map(|draft| draft.id.clone())
        .collect();
    for id in &orphaned {
        skills.remove_draft(id);
    }
    orphaned.len()
}

/// The hash the owner reviewed, lowercased; a missing or malformed one is
/// a 400.
fn reviewed_hash(hash: Option<&str>) -> Result<String, SkillError> {
    hash.map(|hash| hash.trim().to_ascii_lowercase())
        .filter(|hash| is_hex_hash(hash))
        .ok_or_else(|| SkillError::invalid(SKILL_HASH_REQUIRED))
}

fn file_draft_id(slug: &str) -> String {
    format!("{FILE_DRAFT_ID_PREFIX}{slug}")
}

impl SkillService {
    /// A companion proposes a skill (spec §8.3 `propose_skill`).
    pub(crate) async fn propose(&self, proposal: Proposal) -> Result<SkillDraft, SkillError> {
        self.propose_with_base(proposal, None).await
    }

    /// Resolve omitted files from a complete approved package; absent partial
    /// folders and failed reads never become an implicit empty file list.
    pub(crate) async fn supporting_files_for_proposal(
        &self,
        name: &str,
        slug: Option<&str>,
    ) -> Result<(String, Option<String>, Vec<super::SkillSupportingFile>), SkillError> {
        let slug = draft_slug(slug, name)?;
        let (root, _) = self.read_start(&slug).await?;
        let target = slug.clone();
        let (package, partial) = blocking(move || {
            let package = super::package::read_package(&root, &target)?;
            let partial = package.is_none() && super::package::folder_has_content(&root, &target)?;
            Ok::<_, String>((package, partial))
        })
        .await?
        .map_err(SkillError::Unavailable)?;
        let record = self.state.read().await.skills.get(&slug).cloned();
        match (record, package) {
            (Some(record), Some(package)) if record.approved_hash == package.hash() => {
                Ok((slug, Some(record.approved_hash), package.files))
            }
            (None, None) if !partial => Ok((slug, None, Vec::new())),
            _ => Err(SkillError::conflict(SKILL_FILE_UNREVIEWED)),
        }
    }

    /// A preservation read pins both supporting files and the draft's base.
    pub(crate) async fn propose_with_base(
        &self,
        proposal: Proposal,
        expected_base: Option<Option<String>>,
    ) -> Result<SkillDraft, SkillError> {
        let file = checked_file(&proposal.name, &proposal.description, proposal.body)?;
        anima_core::skill_packages::validate_skill_package(
            compose_skill_file(&file.name, &file.description, &file.body).as_bytes(),
            &proposal.files,
        )
        .map_err(SkillError::invalid)?;
        let slug = draft_slug(proposal.slug.as_deref(), &file.name)?;
        let by = proposal.by;
        self.locked(move |service, root| async move {
            let live: HashSet<String> = service.state.read().await.agents.keys().cloned().collect();
            service
                .apply(&root, move |skills, now_ms| {
                    if skills.pending_from(&by.agent_id) >= MAX_PENDING_DRAFTS_PER_AGENT {
                        return Err(SkillError::conflict(TOO_MANY_PENDING_DRAFTS));
                    }
                    let pruned = prune_orphaned(skills, &live, &by.agent_id);
                    if pruned > 0 {
                        warn!(
                            pruned,
                            "dropped the skill drafts of agents that no longer exist"
                        );
                    }
                    let base_hash = skills.get(&slug).map(|record| record.approved_hash.clone());
                    if expected_base
                        .as_ref()
                        .is_some_and(|expected| expected != &base_hash)
                    {
                        return Err(SkillError::conflict(
                            "Skill changed after preserving its supporting files; read it again",
                        ));
                    }
                    let mut draft = SkillDraft::new(
                        &slug,
                        file,
                        DraftSource::Agent,
                        Some(by),
                        base_hash,
                        now_ms,
                    );
                    draft.files = proposal.files;
                    Ok(add_draft(skills, draft, now_ms))
                })
                .await
        })
        .await
    }

    /// The owner imports a `SKILL.md` as a draft (spec §8.4).
    pub(crate) async fn import(
        &self,
        bytes: Vec<u8>,
        slug: Option<String>,
    ) -> Result<SkillDraft, SkillError> {
        self.import_package(bytes, slug, Vec::new()).await
    }
    pub(crate) async fn import_package(
        &self,
        bytes: Vec<u8>,
        slug: Option<String>,
        files: Vec<super::SkillSupportingFile>,
    ) -> Result<SkillDraft, SkillError> {
        let file = parse_skill_file(&bytes).map_err(SkillError::invalid)?;
        anima_core::skill_packages::validate_skill_package(&bytes, &files)
            .map_err(SkillError::invalid)?;
        let slug = draft_slug(slug.as_deref(), &file.name)?;
        self.locked(move |service, root| async move {
            service
                .apply(&root, move |skills, now_ms| {
                    if skills.pending_imports() >= MAX_PENDING_IMPORT_DRAFTS {
                        return Err(SkillError::conflict(TOO_MANY_IMPORT_DRAFTS));
                    }
                    let base_hash = skills.get(&slug).map(|record| record.approved_hash.clone());
                    let mut draft =
                        SkillDraft::new(&slug, file, DraftSource::Import, None, base_hash, now_ms);
                    draft.files = files;
                    Ok(add_draft(skills, draft, now_ms))
                })
                .await
        })
        .await
    }

    /// Drafts waiting for the owner, oldest first, including `SKILL.md`
    /// files without a record (after a rescan); or decided drafts, newest
    /// first.
    pub(crate) async fn drafts(&self, decided: bool) -> Result<Vec<DraftView>, SkillError> {
        if decided {
            self.workspace().await?;
        } else {
            match self.scan().await {
                Ok(_) => {}
                Err(SkillError::NoWorkspace) => return Err(SkillError::NoWorkspace),
                Err(error) => {
                    warn!(error = %error.message(), "skills scan failed; listing the last known drafts")
                }
            }
        }
        let guard = self.state.read().await;
        let skills = &guard.skills;
        let view = |draft: &SkillDraft| DraftView {
            draft: draft.clone(),
            problem: None,
            current_hash: skills
                .get(&draft.slug)
                .map(|record| record.approved_hash.clone()),
        };
        if decided {
            return Ok(skills.decided_drafts().into_iter().map(view).collect());
        }
        let mut views: Vec<DraftView> = skills.pending_drafts().into_iter().map(view).collect();
        views.extend(skills.file_drafts());
        views.sort_by(|left, right| {
            (left.draft.created_at_ms, &left.draft.id)
                .cmp(&(right.draft.created_at_ms, &right.draft.id))
        });
        Ok(views)
    }

    /// The owner approves a draft, optionally with an edited body (spec
    /// §8.4). A stored draft never overwrites a `SKILL.md` the owner has not
    /// reviewed.
    pub(crate) async fn approve_draft(
        &self,
        id: &str,
        approval: DraftApproval,
    ) -> Result<ApprovedDraft, SkillError> {
        self.approve_draft_by(id, approval, None).await
    }

    /// `approve_draft`, recording who approved it when it was not the owner
    /// (`decided_by`, e.g. `autonomy:free`). Every approval takes this one
    /// path, so hashing, pinning, the file write, and the scan agree.
    pub(crate) async fn approve_draft_by(
        &self,
        id: &str,
        approval: DraftApproval,
        decided_by: Option<String>,
    ) -> Result<ApprovedDraft, SkillError> {
        self.approve_draft_by_exact(id, approval, decided_by, None)
            .await
    }

    /// An automatic handoff may only activate the exact persisted draft it
    /// reviewed, even if another caller changed that draft while it waited.
    pub(super) async fn approve_draft_by_exact(
        &self,
        id: &str,
        approval: DraftApproval,
        decided_by: Option<String>,
        expected: Option<SkillDraft>,
    ) -> Result<ApprovedDraft, SkillError> {
        let automatic = decided_by.as_deref() == Some(super::ledger::AUTONOMY_FREE_DECIDER);
        if let Some(body) = &approval.body {
            validate_body(body).map_err(SkillError::invalid)?;
        }
        if let Some(files) = &approval.files {
            anima_core::skill_packages::validate_skill_package(&[], files)
                .map_err(SkillError::invalid)?;
        }
        if let Some(slug) = id.strip_prefix(FILE_DRAFT_ID_PREFIX) {
            if automatic {
                return Err(SkillError::conflict(
                    "Only agent-proposed drafts can activate automatically",
                ));
            }
            return self.approve_file_draft(slug, approval).await;
        }
        let id = id.to_string();
        // Read before the transaction; a decided or unknown draft is
        // answered by `apply` below.
        let pending_slug = self
            .state
            .read()
            .await
            .skills
            .draft(&id)
            .filter(|draft| draft.is_pending())
            .map(|draft| draft.slug.clone());
        let review = match pending_slug {
            Some(slug) => Some(self.review_file(&slug).await?),
            None => None,
        };
        self.locked(move |service, root| async move {
            let reviewed_root = root.clone();
            let linked = service.state.read().await.skills.draft(&id).cloned();
            if expected
                .as_ref()
                .is_some_and(|expected| linked.as_ref() != Some(expected))
            {
                return Err(SkillError::conflict(
                    "The reviewed skill draft changed before automatic activation",
                ));
            }
            if let Some(draft) = linked.as_ref().filter(|draft| draft.is_pending()) {
                let guard = service.state.read().await;
                crate::community_bundles::check_skill_edit(&guard, &draft.slug, true)?;
                let canonical = anima_core::skill_packages::skill_package_hash(
                    compose_skill_file(
                        &draft.name,
                        &draft.description,
                        approval.body.as_deref().unwrap_or(&draft.body),
                    )
                    .as_bytes(),
                    approval.files.as_deref().unwrap_or(&draft.files),
                );
                crate::community_bundles::check_draft(&guard, draft, Some(&canonical))?;
                drop(guard);
                if draft.bundle_source.is_some() {
                    let workspace = root.clone();
                    let slug = draft.slug.clone();
                    let bytes = blocking(move || super::package::read_package(&workspace, &slug))
                        .await?
                        .map_err(SkillError::Unavailable)?;
                    if bytes.as_ref().map(|p| p.hash()) != draft.base_hash {
                        return Err(SkillError::conflict("Community bundle skill base changed"));
                    }
                }
            }
            if let Some(draft) = linked
                .as_ref()
                .filter(|draft| draft.is_pending() && draft.learning_source.is_some())
            {
                let read_root = root.clone();
                let read_slug = draft.slug.clone();
                let bytes = blocking(move || super::package::read_package(&read_root, &read_slug))
                    .await?
                    .map_err(SkillError::Unavailable)?;
                let actual_hash = bytes.as_ref().map(|p| p.hash());
                crate::learning::validate_skill_source(
                    &service.state,
                    draft.learning_source.as_ref().expect("linked draft"),
                    &draft.id,
                )
                .await?;
                let guard = service.state.read().await;
                let current_hash = guard
                    .skills
                    .get(&draft.slug)
                    .map(|record| record.approved_hash.clone());
                if current_hash != draft.base_hash || actual_hash != draft.base_hash {
                    return Err(SkillError::conflict(
                        "Skill base changed; review a fresh draft before replacing it",
                    ));
                }
            }
            let ledger = if automatic {
                let draft = linked.as_ref().ok_or(SkillError::NotFound)?;
                if !draft.is_pending() {
                    return Err(SkillError::conflict(SKILL_DRAFT_DECIDED));
                }
                let by = draft.proposed_by.as_ref().ok_or_else(|| {
                    SkillError::conflict("Only agent-proposed drafts can activate automatically")
                })?;
                let read_root = root.clone();
                let read_slug = draft.slug.clone();
                let previous =
                    blocking(move || super::package::read_package(&read_root, &read_slug))
                        .await?
                        .map_err(SkillError::Unavailable)?;
                let current_hash = service
                    .state
                    .read()
                    .await
                    .skills
                    .get(&draft.slug)
                    .map(|record| record.approved_hash.clone());
                if current_hash != draft.base_hash
                    || previous.as_ref().map(|p| p.hash()) != draft.base_hash
                {
                    return Err(SkillError::conflict(
                        "Skill base changed; review a fresh draft before replacing it",
                    ));
                }
                if previous.as_ref().is_some_and(|package| {
                    package.bytes.len() > super::ledger::MAX_LEDGER_PREVIOUS_CONTENT_BYTES
                }) {
                    return Err(SkillError::conflict(
                        "The previous skill package exceeds the automatic undo bound",
                    ));
                }
                let (previous_content, previous_files) = match previous {
                    Some(package) => (
                        Some(String::from_utf8(package.bytes).map_err(|_| {
                            SkillError::conflict("The previous skill package is not valid text")
                        })?),
                        package.files,
                    ),
                    None => (None, Vec::new()),
                };
                Some(super::ledger::SkillLedgerEntry {
                    previous_files,
                    id: format!(
                        "{}{}",
                        super::ledger::LEDGER_ID_PREFIX,
                        uuid::Uuid::new_v4()
                    ),
                    agent_id: by.agent_id.clone(),
                    skill_slug: draft.slug.clone(),
                    action: if draft.base_hash.is_some() {
                        super::ledger::SkillLedgerAction::Updated
                    } else {
                        super::ledger::SkillLedgerAction::Created
                    },
                    previous_content,
                    previous_content_hash: draft.base_hash.clone(),
                    content_hash: anima_core::skill_packages::skill_package_hash(
                        compose_skill_file(
                            &draft.name,
                            &draft.description,
                            approval.body.as_deref().unwrap_or(&draft.body),
                        )
                        .as_bytes(),
                        approval.files.as_deref().unwrap_or(&draft.files),
                    ),
                    at: 0, // Filled with the approval timestamp under the same write guard.
                    run_id: Some(by.run_id.clone()),
                    reverted_at: None,
                })
            } else {
                None
            };
            let (slug, draft) = service
                .apply_with_ledger(&root, ledger, move |skills, now_ms| {
                    let previous_draft = skills.draft(&id).cloned().ok_or(SkillError::NotFound)?;
                    if !previous_draft.is_pending() {
                        return Err(SkillError::conflict(SKILL_DRAFT_DECIDED));
                    }
                    if (automatic || previous_draft.learning_source.is_some())
                        && previous_draft.base_hash
                            != skills
                                .get(&previous_draft.slug)
                                .map(|record| record.approved_hash.clone())
                    {
                        return Err(SkillError::conflict(
                            "Skill base changed; review a fresh draft before replacing it",
                        ));
                    }
                    // Pending now but not when read: refuse (fail closed).
                    review
                        .as_ref()
                        .ok_or_else(|| SkillError::conflict(SKILL_FILE_UNREVIEWED))?
                        .check(skills, &reviewed_root, &previous_draft.slug)?;
                    let file = SkillFile {
                        name: previous_draft.name.clone(),
                        description: previous_draft.description.clone(),
                        body: approval.body.unwrap_or_else(|| previous_draft.body.clone()),
                    };
                    let bytes =
                        compose_skill_file(&file.name, &file.description, &file.body).into_bytes();
                    let files = approval
                        .files
                        .unwrap_or_else(|| previous_draft.files.clone());
                    anima_core::skill_packages::validate_skill_package(&bytes, &files)
                        .map_err(SkillError::invalid)?;
                    let package = super::package::SkillPackage { bytes, files };
                    let slug = previous_draft.slug.clone();
                    let previous_record = skills.get(&slug).cloned();
                    let mut record = SkillRecord::approved(&slug, &file, package.hash(), now_ms);
                    if let Some(previous) = &previous_record {
                        record.enabled = previous.enabled;
                    }
                    skills.put(record).map_err(SkillError::conflict)?;
                    let mut draft = previous_draft.clone();
                    // Keep the original proposal as its durable provenance and
                    // idempotent handoff identity. Edited approved content is
                    // pinned by the installed package and skill record.
                    draft.decide(DraftStatus::Approved, now_ms);
                    draft.decided_by = decided_by;
                    skills.put_draft(draft.clone());
                    skills.prune_decided(now_ms);
                    let undo_slug = slug.clone();
                    Ok(Change {
                        value: (slug.clone(), draft),
                        undo: Box::new(move |skills: &mut SkillRegistry| {
                            skills.restore(&undo_slug, previous_record);
                            skills.put_draft(previous_draft);
                        }),
                        write: Some((slug.clone(), package)),
                        slug: Some(slug),
                        draft_id: Some(id),
                    })
                })
                .await?;
            Ok(ApprovedDraft {
                skill: service.record(&slug).await?,
                draft,
            })
        })
        .await
    }

    /// Approves a `SKILL.md` found without a record, at the hash the owner
    /// reviewed. The file is read and checked before the transaction; it is
    /// rewritten only when the owner edited the body.
    async fn approve_file_draft(
        &self,
        slug: &str,
        approval: DraftApproval,
    ) -> Result<ApprovedDraft, SkillError> {
        if !is_valid_slug(slug) {
            return Err(SkillError::NotFound);
        }
        let reviewed = reviewed_hash(approval.hash.as_deref())?;
        let slug = slug.to_string();
        let (read_root, seen) = self.read_start(&slug).await?;
        let (workspace, target) = (read_root.clone(), slug.clone());
        let package = blocking(move || super::package::read_package(&workspace, &target))
            .await?
            .map_err(SkillError::Unavailable)?
            .ok_or(SkillError::NotFound)?;
        if package.hash() != reviewed {
            return Err(SkillError::conflict(SKILL_HASH_MISMATCH));
        }
        let parsed = parse_skill_file(&package.bytes).map_err(SkillError::invalid)?;
        let edited = approval.body.is_some() || approval.files.is_some();
        let file = SkillFile {
            body: approval.body.unwrap_or_else(|| parsed.body.clone()),
            ..parsed
        };
        let approved_bytes = if edited {
            compose_skill_file(&file.name, &file.description, &file.body).into_bytes()
        } else {
            package.bytes.clone()
        };
        let files = approval.files.unwrap_or_else(|| package.files.clone());
        anima_core::skill_packages::validate_skill_package(&approved_bytes, &files)
            .map_err(SkillError::invalid)?;
        let approved_package = super::package::SkillPackage {
            bytes: approved_bytes,
            files,
        };
        let approved_hash = approved_package.hash();
        let approved_files = approved_package.files.clone();
        let scanned = (!edited).then(|| ScannedFile::package(&package, None));
        self.locked(move |service, root| async move {
            if root != read_root {
                return Err(SkillError::conflict(SKILL_HASH_MISMATCH));
            }
            crate::community_bundles::check_skill_edit(&*service.state.read().await, &slug, true)?;
            let (target, approved_file) = (slug.clone(), file.clone());
            service
                .apply(&root, move |skills, now_ms| {
                    if skills.get(&target).is_some() {
                        return Err(SkillError::conflict(SKILL_DRAFT_DECIDED));
                    }
                    if let Some(scanned) = &scanned {
                        if skills.scan_moved(&target, &seen, &scanned.hash) {
                            return Err(SkillError::conflict(SKILL_HASH_MISMATCH));
                        }
                    }
                    let record =
                        SkillRecord::approved(&target, &approved_file, approved_hash, now_ms);
                    skills.put(record).map_err(SkillError::conflict)?;
                    if let Some(scanned) = scanned {
                        skills.set_scanned(&target, Some(scanned));
                    }
                    let undo_slug = target.clone();
                    Ok(Change {
                        value: (),
                        undo: Box::new(move |skills: &mut SkillRegistry| {
                            skills.restore(&undo_slug, None)
                        }),
                        write: edited.then(|| (target.clone(), approved_package)),
                        draft_id: Some(file_draft_id(&target)),
                        slug: Some(target),
                    })
                })
                .await?;
            let skill = service.record(&slug).await?;
            let mut draft = SkillDraft::new(
                &slug,
                file,
                DraftSource::File,
                None,
                None,
                skill.approved_at_ms,
            );
            draft.id = file_draft_id(&slug);
            draft.files = approved_files;
            draft.file_hash = Some(reviewed);
            draft.decide(DraftStatus::Approved, skill.approved_at_ms);
            Ok(ApprovedDraft { skill, draft })
        })
        .await
    }

    /// The owner rejects a draft (spec §8.4). A file draft's rejection is
    /// stored with the file's hash, which hides it until the file changes;
    /// the file stays.
    pub(crate) async fn reject_draft(&self, id: &str) -> Result<SkillDraft, SkillError> {
        if let Some(slug) = id.strip_prefix(FILE_DRAFT_ID_PREFIX) {
            return self.reject_file_draft(slug).await;
        }
        let id = id.to_string();
        self.locked(move |service, root| async move {
            service
                .apply(&root, move |skills, now_ms| {
                    let previous = skills.draft(&id).cloned().ok_or(SkillError::NotFound)?;
                    if !previous.is_pending() {
                        return Err(SkillError::conflict(SKILL_DRAFT_DECIDED));
                    }
                    let mut draft = previous.clone();
                    draft.decide(DraftStatus::Rejected, now_ms);
                    skills.put_draft(draft.clone());
                    skills.prune_decided(now_ms);
                    Ok(Change {
                        slug: Some(draft.slug.clone()),
                        value: draft,
                        undo: Box::new(move |skills: &mut SkillRegistry| {
                            skills.put_draft(previous);
                        }),
                        write: None,
                        draft_id: Some(id),
                    })
                })
                .await
        })
        .await
    }

    /// Rejects a `SKILL.md` found without a record as it is now (read before
    /// the transaction).
    async fn reject_file_draft(&self, slug: &str) -> Result<SkillDraft, SkillError> {
        if !is_valid_slug(slug) {
            return Err(SkillError::NotFound);
        }
        let slug = slug.to_string();
        let (read_root, seen) = self.read_start(&slug).await?;
        let (workspace, target) = (read_root.clone(), slug.clone());
        let scanned = blocking(move || disk::scan_skill(&workspace, &target, None))
            .await?
            .map_err(SkillError::Unavailable)?
            .ok_or(SkillError::NotFound)?;
        self.locked(move |service, root| async move {
            if root != read_root {
                return Err(SkillError::conflict(SKILL_HASH_MISMATCH));
            }
            service
                .apply(&root, move |skills, now_ms| {
                    if skills.get(&slug).is_some() {
                        return Err(SkillError::conflict(SKILL_DRAFT_DECIDED));
                    }
                    let already_rejected = skills.decided_drafts().into_iter().any(|draft| {
                        draft.source == DraftSource::File
                            && draft.status == DraftStatus::Rejected
                            && draft.slug == slug
                            && draft.file_hash == scanned.hash
                    });
                    if already_rejected {
                        return Err(SkillError::conflict(SKILL_DRAFT_DECIDED));
                    }
                    if skills.scan_moved(&slug, &seen, &scanned.hash) {
                        return Err(SkillError::conflict(SKILL_HASH_MISMATCH));
                    }
                    let file = scanned.parsed.clone().unwrap_or_else(|_| SkillFile {
                        name: slug.clone(),
                        description: String::new(),
                        body: String::new(),
                    });
                    let mut draft =
                        SkillDraft::new(&slug, file, DraftSource::File, None, None, now_ms);
                    draft.files = scanned.files.clone();
                    draft.file_hash = scanned.hash.clone();
                    draft.decide(DraftStatus::Rejected, now_ms);
                    skills.set_scanned(&slug, Some(scanned));
                    skills.put_draft(draft.clone());
                    skills.prune_decided(now_ms);
                    let undo_id = draft.id.clone();
                    Ok(Change {
                        value: draft,
                        undo: Box::new(move |skills: &mut SkillRegistry| {
                            skills.remove_draft(&undo_id);
                        }),
                        write: None,
                        draft_id: Some(file_draft_id(&slug)),
                        slug: Some(slug),
                    })
                })
                .await
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::RwLock;

    use super::*;
    use crate::agent_runs::test_support::companion_config;
    use crate::app::SharedDaemonState;
    use crate::skills::test_support::{
        broken_store, content, service, skill_text, temp_workspace, with_workspace, write_skill,
    };
    use crate::skills::{
        skill_hash, DraftSource, DraftStatus, SkillStatus, MAX_PENDING_DRAFTS_PER_AGENT,
        MAX_PENDING_IMPORT_DRAFTS, SKILL_BODY_EMPTY, SKILL_DRAFT_DECIDED,
        SKILL_FILE_NO_FRONT_MATTER, SKILL_FILE_UNREVIEWED, SKILL_HASH_MISMATCH,
        SKILL_HASH_REQUIRED, SKILL_SLUG_INVALID, SKILL_TEXT_HIDDEN, TOO_MANY_IMPORT_DRAFTS,
        TOO_MANY_PENDING_DRAFTS,
    };
    use crate::state::DaemonState;

    fn daemon(label: &str) -> (SharedDaemonState, std::path::PathBuf) {
        let root = temp_workspace(label);
        let state = with_workspace(DaemonState::new(), &root);
        (Arc::new(RwLock::new(state)), root)
    }

    fn proposal(agent: &str, name: &str) -> Proposal {
        Proposal {
            files: Vec::new(),
            by: ProposedBy {
                agent_id: agent.into(),
                session_id: "chat:1".into(),
                run_id: "run_1".into(),
            },
            name: name.into(),
            description: format!("About {name}"),
            body: format!("Do {name}."),
            slug: None,
        }
    }

    fn reviewed(hash: &str) -> DraftApproval {
        DraftApproval {
            files: None,
            body: None,
            hash: Some(hash.into()),
        }
    }

    #[tokio::test]
    async fn package_proposal_review_survives_restart_and_approves_only_reviewed_files() {
        let (state, root) = daemon("package-proposal");
        let skills = service(&state);
        let mut proposal = proposal("agent-1", "Packaged notes");
        proposal.files = vec![super::super::SkillSupportingFile {
            path: "templates/note.md".into(),
            content: "Original proposed template".into(),
        }];
        let draft = skills.propose(proposal).await.unwrap();
        assert!(!root.join("skills/packaged-notes").exists());
        let json = serde_json::to_value(state.read().await.control_plane_snapshot()).unwrap();
        assert_eq!(
            json["skillDrafts"][0]["files"][0]["content"],
            "Original proposed template"
        );
        let mut restarted = with_workspace(DaemonState::new(), &root);
        restarted
            .restore_control_plane_snapshot(serde_json::from_value(json).unwrap())
            .unwrap();
        let restored = service(&Arc::new(RwLock::new(restarted)));
        let found = restored.drafts(false).await.unwrap();
        assert_eq!(found[0].draft.files, draft.files);
        let files = vec![super::super::SkillSupportingFile {
            path: "templates/note.md".into(),
            content: "Owner reviewed edit".into(),
        }];
        let approved = restored
            .approve_draft(
                &draft.id,
                DraftApproval {
                    files: Some(files.clone()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(approved.draft.files, draft.files);
        assert_eq!(approved.draft.body, draft.body);
        assert!(restored
            .load_file("packaged-notes", Some("templates/note.md"))
            .await
            .unwrap()
            .body
            .ends_with("Owner reviewed edit"));
        assert!(restored
            .approve_draft(&draft.id, DraftApproval::default())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn package_file_draft_hash_rejects_a_supporting_file_changed_after_review() {
        let (state, root) = daemon("package-file-draft");
        let skills = service(&state);
        write_skill(&root, "notes", &skill_text("notes"));
        std::fs::create_dir_all(root.join("skills/notes/references")).unwrap();
        std::fs::write(root.join("skills/notes/references/guide.md"), "First guide").unwrap();
        let drafts = skills.drafts(false).await.unwrap();
        let reviewed = drafts[0].draft.file_hash.clone().unwrap();
        assert_eq!(drafts[0].draft.files.len(), 1);
        std::fs::write(
            root.join("skills/notes/references/guide.md"),
            "Changed guide",
        )
        .unwrap();
        assert_eq!(
            skills
                .approve_draft(
                    "file:notes",
                    DraftApproval {
                        hash: Some(reviewed),
                        ..Default::default()
                    }
                )
                .await
                .unwrap_err(),
            SkillError::Conflict(SKILL_HASH_MISMATCH.into())
        );
        assert!(state.read().await.skills.get("notes").is_none());
    }

    #[tokio::test]
    async fn a_proposal_is_a_pending_draft_and_writes_nothing() {
        let (state, root) = daemon("propose");
        let skills = service(&state);
        skills
            .save("weekly-review", content("weekly-review"))
            .await
            .unwrap();
        let base = state
            .read()
            .await
            .skills
            .get("weekly-review")
            .unwrap()
            .approved_hash
            .clone();

        let draft = skills
            .propose(proposal("agent-1", "Weekly Review"))
            .await
            .unwrap();

        assert!(draft.is_pending());
        assert_eq!(draft.slug, "weekly-review", "derived from the name");
        assert_eq!(draft.source, DraftSource::Agent);
        assert_eq!(draft.proposed_by.as_ref().unwrap().run_id, "run_1");
        assert_eq!(draft.base_hash, Some(base));
        assert_eq!(
            std::fs::read_to_string(root.join("skills/weekly-review/SKILL.md")).unwrap(),
            skill_text("weekly-review"),
            "the approved file is untouched"
        );
        let mut bad = proposal("agent-1", "x");
        bad.slug = Some("Bad Slug".into());
        assert_eq!(
            skills.propose(bad).await,
            Err(SkillError::Invalid(SKILL_SLUG_INVALID.into()))
        );
        let mut empty = proposal("agent-1", "x");
        empty.body = " ".into();
        assert_eq!(
            skills.propose(empty).await,
            Err(SkillError::Invalid(SKILL_BODY_EMPTY.into()))
        );
        let mut hidden = proposal("agent-1", "x");
        hidden.body = "Do x.\u{E0041}".into();
        assert_eq!(
            skills.propose(hidden).await,
            Err(SkillError::Invalid(SKILL_TEXT_HIDDEN.into()))
        );
    }

    #[tokio::test]
    async fn an_agent_may_have_ten_drafts_waiting() {
        let (state, _) = daemon("propose-cap");
        let skills = service(&state);
        for index in 0..MAX_PENDING_DRAFTS_PER_AGENT {
            skills
                .propose(proposal("agent-1", &format!("skill {index}")))
                .await
                .unwrap();
        }
        assert_eq!(
            skills.propose(proposal("agent-1", "one more")).await,
            Err(SkillError::Conflict(TOO_MANY_PENDING_DRAFTS.into()))
        );
        assert!(
            skills
                .propose(proposal("agent-2", "another agent"))
                .await
                .is_ok(),
            "the cap is per agent"
        );
    }

    #[tokio::test]
    async fn a_new_proposal_drops_the_pending_drafts_of_deleted_agents() {
        let (state, _) = daemon("propose-orphans");
        let (kept, gone) = {
            let mut guard = state.write().await;
            let kept = guard
                .create_agent(companion_config("kept"))
                .unwrap()
                .state
                .id;
            let gone = guard
                .create_agent(companion_config("gone"))
                .unwrap()
                .state
                .id;
            (kept, gone)
        };
        let skills = service(&state);
        let first = skills.propose(proposal(&kept, "First")).await.unwrap();
        let orphan = skills.propose(proposal(&gone, "Orphan")).await.unwrap();
        let rejected = skills.propose(proposal(&gone, "Rejected")).await.unwrap();
        skills.reject_draft(&rejected.id).await.unwrap();
        state.write().await.remove_agent(&gone);

        let second = skills.propose(proposal(&kept, "Second")).await.unwrap();

        let guard = state.read().await;
        assert!(guard.skills.draft(&first.id).unwrap().is_pending());
        assert!(guard.skills.draft(&second.id).unwrap().is_pending());
        assert!(
            guard.skills.draft(&orphan.id).is_none(),
            "a deleted agent's pending draft goes"
        );
        assert!(
            guard.skills.draft(&rejected.id).is_some(),
            "decided drafts follow their own retention"
        );
        assert_eq!(guard.skills.pending_from(&gone), 0);
    }

    #[tokio::test]
    async fn an_import_is_a_pending_draft_capped_at_ten() {
        let (state, _) = daemon("import");
        let skills = service(&state);
        let draft = skills
            .import(skill_text("Imported").into_bytes(), None)
            .await
            .unwrap();
        assert_eq!(draft.slug, "imported");
        assert_eq!(draft.source, DraftSource::Import);
        assert!(draft.proposed_by.is_none());
        assert_eq!(
            skills.import(b"no front matter".to_vec(), None).await,
            Err(SkillError::Invalid(SKILL_FILE_NO_FRONT_MATTER.into()))
        );
        assert_eq!(
            skills
                .import(skill_text("Imp\u{200B}orted").into_bytes(), None)
                .await,
            Err(SkillError::Invalid(SKILL_TEXT_HIDDEN.into()))
        );
        let named = skills
            .import(skill_text("Imported").into_bytes(), Some("chosen".into()))
            .await
            .unwrap();
        assert_eq!(named.slug, "chosen");
        for index in 2..MAX_PENDING_IMPORT_DRAFTS {
            skills
                .import(skill_text(&format!("imported {index}")).into_bytes(), None)
                .await
                .unwrap();
        }
        assert_eq!(
            skills
                .import(skill_text("too many").into_bytes(), None)
                .await,
            Err(SkillError::Conflict(TOO_MANY_IMPORT_DRAFTS.into()))
        );
    }

    #[tokio::test]
    async fn pending_drafts_include_files_without_a_record() {
        let (state, root) = daemon("list");
        let skills = service(&state);
        let proposed = skills.propose(proposal("agent-1", "Notes")).await.unwrap();
        write_skill(&root, "found", &skill_text("found"));
        skills.save("kept", content("kept")).await.unwrap();

        let pending = skills.drafts(false).await.unwrap();
        let ids: Vec<_> = pending.iter().map(|view| view.draft.id.clone()).collect();
        assert!(ids.contains(&proposed.id));
        assert!(ids.contains(&"file:found".to_string()));
        assert!(
            !ids.iter().any(|id| id == "file:kept"),
            "a recorded skill is no draft"
        );
        assert!(skills.drafts(true).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn approving_a_draft_writes_and_pins_it() {
        let (state, root) = daemon("approve");
        let skills = service(&state);
        let draft = skills.propose(proposal("agent-1", "Notes")).await.unwrap();

        let approved = skills
            .approve_draft(&draft.id, DraftApproval::default())
            .await
            .unwrap();

        let written = std::fs::read(root.join("skills/notes/SKILL.md")).unwrap();
        assert_eq!(written, skill_text("Notes").into_bytes());
        assert_eq!(approved.skill.approved_hash, skill_hash(&written));
        assert_eq!(approved.skill.status, SkillStatus::Active);
        assert!(approved.skill.enabled);
        assert_eq!(approved.draft.status, DraftStatus::Approved);
        assert_eq!(skills.load("notes").await.unwrap().body, "Do Notes.");
        assert_eq!(
            skills
                .approve_draft(&draft.id, DraftApproval::default())
                .await
                .err(),
            Some(SkillError::Conflict(SKILL_DRAFT_DECIDED.into()))
        );
        assert_eq!(skills.drafts(true).await.unwrap()[0].draft.id, draft.id);
    }

    #[tokio::test]
    async fn an_edited_body_is_what_gets_approved_and_the_switch_is_kept() {
        let (state, _) = daemon("approve-edit");
        let skills = service(&state);
        skills.save("notes", content("Notes")).await.unwrap();
        skills.set_enabled("notes", false).await.unwrap();
        let draft = skills.propose(proposal("agent-1", "Notes")).await.unwrap();

        let approved = skills
            .approve_draft(
                &draft.id,
                DraftApproval {
                    files: None,
                    body: Some("Edited by the owner.".into()),
                    hash: None,
                },
            )
            .await
            .unwrap();

        assert!(
            !approved.skill.enabled,
            "an existing skill keeps its switch"
        );
        skills.set_enabled("notes", true).await.unwrap();
        assert_eq!(
            skills.load("notes").await.unwrap().body,
            "Edited by the owner."
        );
    }

    #[tokio::test]
    async fn approving_a_draft_never_overwrites_an_unreviewed_file() {
        let (state, root) = daemon("approve-unreviewed");
        let skills = service(&state);
        let path = write_skill(&root, "notes", &skill_text("Written by hand"));
        let draft = skills.propose(proposal("agent-1", "Notes")).await.unwrap();

        assert_eq!(
            skills
                .approve_draft(&draft.id, DraftApproval::default())
                .await
                .err(),
            Some(SkillError::Conflict(SKILL_FILE_UNREVIEWED.into()))
        );
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            skill_text("Written by hand")
        );
        assert!(state
            .read()
            .await
            .skills
            .draft(&draft.id)
            .unwrap()
            .is_pending());
    }

    #[tokio::test]
    async fn a_file_the_owner_rejected_may_be_replaced() {
        let (state, root) = daemon("approve-rejected-file");
        let skills = service(&state);
        write_skill(&root, "notes", &skill_text("Written by hand"));
        let draft = skills.propose(proposal("agent-1", "Notes")).await.unwrap();
        skills.reject_draft("file:notes").await.unwrap();

        let approved = skills
            .approve_draft(&draft.id, DraftApproval::default())
            .await
            .unwrap();
        assert_eq!(approved.skill.slug, "notes");
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/SKILL.md")).unwrap(),
            skill_text("Notes")
        );

        write_skill(&root, "other", &skill_text("Other by hand"));
        skills.reject_draft("file:other").await.unwrap();
        assert!(skills.save("other", content("other")).await.is_ok());
    }

    #[tokio::test]
    async fn approving_a_file_draft_needs_the_hash_the_owner_reviewed() {
        let (state, root) = daemon("approve-file");
        let skills = service(&state);
        write_skill(&root, "found", &skill_text("found"));
        let reviewed_hash = skill_hash(skill_text("found").as_bytes());

        for approval in [
            DraftApproval::default(),
            reviewed(""),
            reviewed("not-a-hash"),
        ] {
            assert_eq!(
                skills.approve_draft("file:found", approval).await.err(),
                Some(SkillError::Invalid(SKILL_HASH_REQUIRED.into()))
            );
        }
        write_skill(&root, "found", &skill_text("swapped after review"));
        assert_eq!(
            skills
                .approve_draft("file:found", reviewed(&reviewed_hash))
                .await
                .err(),
            Some(SkillError::Conflict(SKILL_HASH_MISMATCH.into()))
        );

        write_skill(&root, "found", &skill_text("found"));
        let approved = skills
            .approve_draft("file:found", reviewed(&reviewed_hash.to_uppercase()))
            .await
            .unwrap();
        assert_eq!(approved.skill.approved_hash, reviewed_hash);
        assert_eq!(approved.draft.id, "file:found");
        assert_eq!(approved.draft.status, DraftStatus::Approved);
        assert_eq!(
            std::fs::read_to_string(root.join("skills/found/SKILL.md")).unwrap(),
            skill_text("found"),
            "an unedited file draft is not rewritten"
        );
        assert_eq!(skills.load("found").await.unwrap().body, "Do found.");
        assert_eq!(
            skills
                .approve_draft("file:found", reviewed(&reviewed_hash))
                .await
                .err(),
            Some(SkillError::Conflict(SKILL_DRAFT_DECIDED.into()))
        );
    }

    #[tokio::test]
    async fn an_edited_file_draft_is_rewritten_and_pinned_to_what_was_written() {
        let (state, root) = daemon("approve-file-edit");
        let skills = service(&state);
        write_skill(&root, "found", &skill_text("found"));

        let approved = skills
            .approve_draft(
                "file:found",
                DraftApproval {
                    files: None,
                    body: Some("Edited by the owner.".into()),
                    hash: Some(skill_hash(skill_text("found").as_bytes())),
                },
            )
            .await
            .unwrap();

        let written = std::fs::read(root.join("skills/found/SKILL.md")).unwrap();
        assert_eq!(approved.skill.approved_hash, skill_hash(&written));
        assert_eq!(
            skills.load("found").await.unwrap().body,
            "Edited by the owner."
        );
    }

    #[tokio::test]
    async fn a_file_draft_is_checked_before_waiting_for_the_transaction() {
        let (state, root) = daemon("approve-file-outside");
        let skills = service(&state);
        write_skill(&root, "found", &skill_text("found"));
        let _held = skills.transactions.clone().lock_owned().await;

        // Generous: a check that waited for the transaction would never end.
        let refused = tokio::time::timeout(
            Duration::from_secs(10),
            skills.approve_draft("file:found", reviewed(&"0".repeat(64))),
        )
        .await;

        assert_eq!(
            refused.map(|result| result.err()),
            Ok(Some(SkillError::Conflict(SKILL_HASH_MISMATCH.into())))
        );
    }

    #[tokio::test]
    async fn rejecting_keeps_drafts_and_hides_a_file_until_it_changes() {
        let (state, root) = daemon("reject");
        let skills = service(&state);
        let draft = skills.propose(proposal("agent-1", "Notes")).await.unwrap();
        let rejected = skills.reject_draft(&draft.id).await.unwrap();
        assert_eq!(rejected.status, DraftStatus::Rejected);
        assert_eq!(
            skills.reject_draft(&draft.id).await,
            Err(SkillError::Conflict(SKILL_DRAFT_DECIDED.into()))
        );
        assert_eq!(
            skills.reject_draft("skd_missing").await,
            Err(SkillError::NotFound)
        );

        write_skill(&root, "found", &skill_text("found"));
        let hidden = skills.reject_draft("file:found").await.unwrap();
        assert_eq!(hidden.source, DraftSource::File);
        assert!(
            root.join("skills/found/SKILL.md").exists(),
            "the file stays"
        );
        assert_eq!(
            skills.reject_draft("file:found").await,
            Err(SkillError::Conflict(SKILL_DRAFT_DECIDED.into())),
            "already rejected at this hash"
        );
        let pending =
            |views: Vec<DraftView>| views.into_iter().any(|view| view.draft.id == "file:found");
        assert!(!pending(skills.drafts(false).await.unwrap()));
        write_skill(&root, "found", &skill_text("found, edited"));
        assert!(pending(skills.drafts(false).await.unwrap()));
        assert_eq!(skills.drafts(true).await.unwrap().len(), 2);
        assert_eq!(
            skills.reject_draft("file:ghost").await,
            Err(SkillError::NotFound)
        );
    }

    #[tokio::test]
    async fn a_failed_save_leaves_the_draft_pending_and_the_skill_untrusted() {
        let (state, root) = daemon("approve-fail");
        let skills = service(&state);
        let draft = skills.propose(proposal("agent-1", "Notes")).await.unwrap();
        state
            .write()
            .await
            .set_control_plane_store(Some(broken_store()));

        assert!(matches!(
            skills
                .approve_draft(&draft.id, DraftApproval::default())
                .await,
            Err(SkillError::Unavailable(_))
        ));

        let guard = state.read().await;
        assert!(guard.skills.draft(&draft.id).unwrap().is_pending());
        assert!(guard.skills.get("notes").is_none());
        drop(guard);
        assert!(
            root.join("skills/notes/SKILL.md").exists(),
            "the written file stays, as a file draft for review"
        );
        assert!(matches!(
            skills.load("notes").await,
            Err(message) if message.starts_with("No owner-approved skill")
        ));
    }
    #[tokio::test]
    async fn inferred_supporting_files_cannot_replace_a_newer_owner_package() {
        let (state, root) = daemon("inferred-files-base");
        let skills = service(&state);
        let mut first = content("Notes");
        first.files = Some(vec![super::super::SkillSupportingFile {
            path: "references/check.md".into(),
            content: "Version one".into(),
        }]);
        let (original, _) = skills.save("notes", first.clone()).await.unwrap();
        let mut inferred = proposal("agent-1", "Notes");
        inferred.files = first.files.unwrap();
        let mut next = content("Notes");
        next.files = Some(vec![super::super::SkillSupportingFile {
            path: "references/check.md".into(),
            content: "Owner's newer version".into(),
        }]);
        let (updated, _) = skills.save("notes", next).await.unwrap();
        let result = skills
            .propose_with_base(inferred.clone(), Some(Some(original.approved_hash)))
            .await;
        assert!(
            matches!(result, Err(SkillError::Conflict(_))),
            "a stale inferred package must not be drafted"
        );
        assert_eq!(state.read().await.skills.pending_from("agent-1"), 0);
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/references/check.md")).unwrap(),
            "Owner's newer version"
        );
        assert!(
            matches!(
                skills.propose_with_base(inferred, Some(None)).await,
                Err(SkillError::Conflict(_))
            ),
            "a concurrent creation also changes the inferred base"
        );
        assert_eq!(
            skills.record("notes").await.unwrap().approved_hash,
            updated.approved_hash
        );
    }
}
