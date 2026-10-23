//! Skill operations (spec §8): rescans, the owner's changes, and loading an
//! approved skill for a run.
//!
//! Changes run under the control-plane transaction in their own task
//! (`locked`), so a dropped caller never leaves an unsaved change in memory.
//! `apply` changes the registry under the state lock, writes the file on the
//! blocking pool (bounded by `SKILL_IO_TIMEOUT_MS`, no lock held), records
//! what was written, saves, and announces `skill.updated`; a failed write or
//! save runs the change's undo. A file already written stays: its hash no
//! longer matches the record, so the skill reads `changed` and is never
//! loaded until approved again (fail closed). Scans take no transaction and
//! are dropped when a change overtook them (`SkillRegistry::apply_scan`).

use std::collections::BTreeSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anima_core::primitives::now_millis;
use tokio::sync::Mutex;
use tracing::warn;

use super::disk;
use super::registry::{is_hex_hash, ScannedFile, SkillRegistry};
use super::{
    compose_skill_file, is_valid_slug, parse_skill_file, skill_not_found, validate_body,
    validate_description, validate_name, DraftSource, DraftStatus, SkillFile, SkillRecord,
    SKILLS_NEED_WORKSPACE, SKILL_CHANGED, SKILL_DISABLED, SKILL_FILE_UNREVIEWED,
    SKILL_HASH_MISMATCH, SKILL_HASH_REQUIRED, SKILL_IO_SLOW_MS, SKILL_IO_TIMED_OUT,
    SKILL_IO_TIMEOUT_MS, SKILL_MISSING, SKILL_NOT_CHANGED, SKILL_NOT_RUNNABLE, SKILL_SLUG_INVALID,
    UNKNOWN_SKILL,
};
use crate::app::SharedDaemonState;

/// Why a skill operation did not happen; routes map each to a status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SkillError {
    /// 409: no workspace is configured.
    NoWorkspace,
    /// 404.
    NotFound,
    /// 400.
    Invalid(String),
    /// 409.
    Conflict(String),
    /// 503: file work failed or timed out, or the change could not be saved.
    Unavailable(String),
}

impl SkillError {
    pub(super) fn invalid(message: &str) -> Self {
        Self::Invalid(message.to_string())
    }

    pub(crate) fn conflict(message: &str) -> Self {
        Self::Conflict(message.to_string())
    }

    /// What a tool result or a log line says.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::NoWorkspace => SKILLS_NEED_WORKSPACE.to_string(),
            Self::NotFound => "not found".to_string(),
            Self::Invalid(message) | Self::Conflict(message) | Self::Unavailable(message) => {
                message.clone()
            }
        }
    }
}

/// The owner's content for `PUT /api/skills/{slug}`.
#[derive(Clone, Debug)]
pub(crate) struct SkillContent {
    /// None preserves existing supporting files; Some([]) removes them.
    pub(crate) files: Option<Vec<super::SkillSupportingFile>>,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) body: String,
    /// `None` keeps an existing skill's switch (a new skill starts on).
    pub(crate) enabled: Option<bool>,
}

/// A skill's record and what its `SKILL.md` holds now.
#[derive(Clone, Debug)]
pub(crate) struct SkillDetail {
    pub(crate) record: Option<SkillRecord>,
    pub(crate) file: Option<ScannedFile>,
}

/// An approved skill's instructions, read now and checked against its hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LoadedSkill {
    pub(crate) slug: String,
    pub(crate) name: String,
    pub(crate) body: String,
    pub(crate) approved_hash: String,
}

/// A registry change `apply` makes, and how to undo it.
pub(super) struct Change<T> {
    pub(super) value: T,
    pub(super) undo: Box<dyn FnOnce(&mut SkillRegistry) + Send>,
    /// Written to `skills/<slug>/SKILL.md` after the registry change.
    pub(super) write: Option<(String, super::package::SkillPackage)>,
    /// What `skill.updated` names.
    pub(super) slug: Option<String>,
    pub(super) draft_id: Option<String>,
}

/// Runs blocking file work on the blocking pool, bounded by
/// `SKILL_IO_TIMEOUT_MS`; work slower than `SKILL_IO_SLOW_MS` is logged.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, SkillError> {
    let started = Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_millis(SKILL_IO_TIMEOUT_MS),
        tokio::task::spawn_blocking(work),
    )
    .await;
    let elapsed_ms = started.elapsed().as_millis();
    if elapsed_ms > u128::from(SKILL_IO_SLOW_MS) {
        warn!(elapsed_ms = %elapsed_ms, "slow skill file work");
    }
    match outcome {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(SkillError::Unavailable(format!(
            "skill file work failed: {error}"
        ))),
        Err(_) => Err(SkillError::Unavailable(SKILL_IO_TIMED_OUT.to_string())),
    }
}

/// The state a scan reads before its file work.
struct ScanInput {
    root: PathBuf,
    generation: u64,
    previous: std::collections::BTreeMap<String, ScannedFile>,
    registered: BTreeSet<String>,
}

/// A `SKILL.md` read before the transaction, so a change never writes over
/// a file the owner has not reviewed (`SkillService::review_file`).
pub(super) struct FileReview {
    root: PathBuf,
    slug: String,
    /// `None` when the skill had a record (nothing was read); otherwise the
    /// file's hash (`Ok(None)` without a file), or `Err` when it could not
    /// be read.
    read: Option<Result<Option<String>, ()>>,
}

impl FileReview {
    /// The cheap check made inside the transaction, against the registry
    /// as it is now: a skill with a record passes; otherwise the file read
    /// must be absent or one the owner rejected at its hash. A read error, a
    /// workspace switched since the read, or a record gone since a read that
    /// was skipped all refuse (fail closed).
    pub(super) fn check(
        &self,
        skills: &SkillRegistry,
        root: &Path,
        slug: &str,
    ) -> Result<(), SkillError> {
        let refused = || Err(SkillError::conflict(SKILL_FILE_UNREVIEWED));
        if root != self.root || slug != self.slug {
            return refused();
        }
        if skills.get(slug).is_some() {
            return Ok(());
        }
        let hash = match &self.read {
            Some(Ok(None)) => return Ok(()),
            Some(Ok(Some(hash))) => hash,
            Some(Err(())) | None => return refused(),
        };
        let rejected = skills.decided_drafts().into_iter().any(|draft| {
            draft.source == DraftSource::File
                && draft.status == DraftStatus::Rejected
                && draft.slug == slug
                && draft.file_hash.as_deref() == Some(hash.as_str())
        });
        if rejected {
            Ok(())
        } else {
            refused()
        }
    }
}

#[derive(Clone)]
pub(crate) struct SkillService {
    pub(super) state: SharedDaemonState,
    pub(super) transactions: Arc<Mutex<()>>,
    pub(super) evaluation_admission: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl SkillService {
    pub(crate) fn new(state: SharedDaemonState, transactions: Arc<Mutex<()>>) -> Self {
        Self {
            state,
            transactions,
            evaluation_admission: None,
        }
    }

    pub(crate) fn with_evaluation_admission(
        mut self,
        probe: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        self.evaluation_admission = Some(probe);
        self
    }

    pub(super) async fn workspace(&self) -> Result<PathBuf, SkillError> {
        self.state
            .read()
            .await
            .workspace
            .as_ref()
            .map(|workspace| workspace.root_path.clone())
            .ok_or(SkillError::NoWorkspace)
    }

    async fn profile_authority(
        &self,
    ) -> Result<Option<crate::profile_owner::ProfileOwner>, SkillError> {
        let owner = self.state.read().await.profile_owner();
        if let Some(owner) = &owner {
            owner
                .verify_owned()
                .await
                .map_err(|error| SkillError::Unavailable(error.to_string()))?;
        }
        Ok(owner)
    }

    /// Runs `work` holding the control-plane transaction, in its own task,
    /// so it reaches its end even if the caller is dropped.
    pub(super) async fn locked<T, F, Fut>(&self, work: F) -> Result<T, SkillError>
    where
        T: Send + 'static,
        F: FnOnce(SkillService, PathBuf) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, SkillError>> + Send + 'static,
    {
        let this = self.clone();
        tokio::spawn(async move {
            let _transaction = Arc::clone(&this.transactions).lock_owned().await;
            let _owner = this.profile_authority().await?;
            let root = this.workspace().await?;
            work(this, root).await
        })
        .await
        .unwrap_or_else(|error| {
            Err(SkillError::Unavailable(format!(
                "the skill change failed: {error}"
            )))
        })
    }

    /// Applies `change`, writes its file, saves, and announces it; call it
    /// inside `locked`. The registry is put back if the write or the save
    /// fails.
    pub(super) async fn apply<T>(
        &self,
        root: &Path,
        change: impl FnOnce(&mut SkillRegistry, u64) -> Result<Change<T>, SkillError>,
    ) -> Result<T, SkillError> {
        self.apply_with_ledger(root, None, None, change).await
    }

    /// Automatic activation and its complete undo record share this save.
    /// Call inside `locked`, as for ordinary registry changes.
    pub(super) async fn apply_with_ledger<T>(
        &self,
        root: &Path,
        ledger: Option<super::ledger::SkillLedgerEntry>,
        evaluation: Option<super::evaluation_service::EvaluationGate>,
        change: impl FnOnce(&mut SkillRegistry, u64) -> Result<Change<T>, SkillError>,
    ) -> Result<T, SkillError> {
        self.apply_with_history(
            root,
            ledger.map(|entry| super::ledger::LedgerChange::Append {
                entry,
                require_free: true,
            }),
            evaluation,
            change,
        )
        .await
    }

    pub(super) async fn apply_with_history<T>(
        &self,
        root: &Path,
        history: Option<super::ledger::LedgerChange>,
        evaluation: Option<super::evaluation_service::EvaluationGate>,
        change: impl FnOnce(&mut SkillRegistry, u64) -> Result<Change<T>, SkillError>,
    ) -> Result<T, SkillError> {
        let owner = self.profile_authority().await?;
        let rollback_owner = owner.clone();
        // Undo must remain retryable after a definite persistence failure.
        // Read the preimage before changing the registry, while the caller holds
        // the shared transaction; an I/O error here changes nothing.
        let undo_preimage = if let Some(super::ledger::LedgerChange::Revert(id)) = &history {
            let slug = self
                .state
                .read()
                .await
                .skill_ledger
                .iter()
                .find(|entry| &entry.id == id)
                .map(|entry| entry.skill_slug.clone())
                .ok_or(SkillError::NotFound)?;
            let (workspace, target) = (root.to_path_buf(), slug.clone());
            let package = blocking(move || super::package::read_package(&workspace, &target))
                .await?
                .map_err(SkillError::Unavailable)?;
            Some((slug, package))
        } else {
            None
        };
        let mut guard = self.state.write().await;
        guard
            .ensure_profile_owned()
            .map_err(|error| SkillError::Unavailable(error.to_string()))?;
        if let Some(gate) = evaluation.as_ref() {
            super::evaluation_service::check_promotion(&guard, root, gate)?;
        }
        if let Some(history) = &history {
            history.check(&guard)?;
        }
        let now_ms = now_millis();
        let change = change(&mut guard.skills, now_ms)?;
        if let Some(slug) = change.slug.as_deref() {
            // Explicit authoring, activation, and exact Undo each invalidate
            // old response evidence. None grants another approval.
            guard.skill_evaluations.invalidate(slug);
        }
        let managed = change
            .slug
            .as_ref()
            .is_some_and(|slug| crate::community_bundles::managed_skill(&guard, slug).is_some());
        if let Some(slug) = &change.slug {
            if let Err(error) = crate::community_bundles::check_skill_edit(&guard, slug, false) {
                (change.undo)(&mut guard.skills);
                return Err(error);
            }
        }
        let prior_generation = guard.community_generation;
        if managed {
            let Some(next_generation) = prior_generation.checked_add(1) else {
                (change.undo)(&mut guard.skills);
                return Err(SkillError::conflict(
                    "Community bundle generation exhausted",
                ));
            };
            guard.community_generation = next_generation;
        }
        let prior_ledger = history.as_ref().map(|_| guard.skill_ledger.clone());
        if let Some(history) = history {
            history.apply(&mut guard.skill_ledger, now_ms);
        }
        drop(guard);
        let Change {
            value,
            undo,
            write,
            slug,
            draft_id,
        } = change;
        let written_hash = write.as_ref().map(|(_, package)| package.hash());
        if let Some((written, package)) = write {
            let (workspace, target) = (root.to_path_buf(), written.clone());
            let outcome = blocking(move || {
                if let Some(owner) = &owner {
                    owner.ensure_owned().map_err(|error| error.to_string())?;
                }
                super::package::write_package(&workspace, &target, &package)?;
                disk::scan_skill(&workspace, &target, None)
            })
            .await
            .and_then(|result| result.map_err(SkillError::Unavailable));
            match outcome {
                Ok(scanned) => self
                    .state
                    .write()
                    .await
                    .skills
                    .set_scanned(&written, scanned),
                Err(error) => {
                    // A late write may still land: the old scan says nothing
                    // about the file now, so the skill reads missing until
                    // the next scan.
                    let mut guard = self.state.write().await;
                    undo(&mut guard.skills);
                    if let Some(previous) = prior_ledger {
                        guard.skill_ledger = previous;
                    }
                    guard.community_generation = prior_generation;
                    guard.skills.set_scanned(&written, None);
                    return Err(error);
                }
            }
        }
        let persist = {
            let mut guard = self.state.write().await;
            let bundles = guard.community_bundles.clone();
            let generation = guard.community_generation;
            let rows = guard.mcp_registry.snapshot();
            let skills = guard.skills.snapshot();
            let request = guard.control_plane_persist_request();
            if managed {
                let Some(request) =
                    request.with_bundle_candidate(bundles, generation, rows, skills, None)
                else {
                    undo(&mut guard.skills);
                    if let Some(previous) = prior_ledger {
                        guard.skill_ledger = previous;
                    }
                    guard.community_generation = prior_generation;
                    return Err(SkillError::conflict("Community bundle owner changed"));
                };
                request
            } else {
                request
            }
        };
        let saved = if managed {
            persist.save_mcp_candidate().await
        } else {
            persist.save().await
        };
        if let Err(error) = saved {
            let mut guard = self.state.write().await;
            if crate::control_plane_store::is_durability_uncertain(&error) {
                // A possibly durable snapshot retains its registry and undo candidate.
                if let Some(slug) = slug.as_ref().filter(|_| managed) {
                    if let Some(bundle) = guard
                        .community_bundles
                        .iter_mut()
                        .find(|bundle| bundle.skills().any(|part| &part.slug == slug))
                    {
                        bundle.state = "unavailable".into();
                    }
                }
            } else {
                undo(&mut guard.skills);
                if let Some(previous) = prior_ledger {
                    guard.skill_ledger = previous;
                }
                guard.community_generation = prior_generation;
                drop(guard);
                if let Some((target, previous)) = undo_preimage {
                    let workspace = root.to_path_buf();
                    let scanned_target = target.clone();
                    let restored = blocking(move || {
                        if let Some(owner) = &rollback_owner {
                            owner.ensure_owned().map_err(|error| error.to_string())?;
                        }
                        let current = super::package::read_package(&workspace, &target)?;
                        if current.as_ref().map(|package| package.hash()) != written_hash {
                            return Err(
                                "Skill changed externally while undo was saving".to_string()
                            );
                        }
                        match previous {
                            Some(package) => {
                                super::package::write_package(&workspace, &target, &package)?
                            }
                            None => {
                                disk::trash_skill_folder(&workspace, &target, now_millis())?;
                            }
                        }
                        disk::scan_skill(&workspace, &target, None)
                    })
                    .await;
                    match restored {
                        Ok(Ok(scanned)) => self
                            .state
                            .write()
                            .await
                            .skills
                            .set_scanned(&scanned_target, scanned),
                        _ => {
                            self.state
                                .write()
                                .await
                                .skills
                                .set_scanned(&scanned_target, None);
                            warn!(skill = %scanned_target, "undo preimage could not be restored after a failed save");
                        }
                    }
                }
            }
            return Err(SkillError::Unavailable(error.to_string()));
        }
        self.state
            .read()
            .await
            .publish_skill_updated(slug.as_deref(), draft_id.as_deref());
        Ok(value)
    }

    pub(super) async fn record(&self, slug: &str) -> Result<SkillRecord, SkillError> {
        self.state
            .read()
            .await
            .skills
            .get(slug)
            .cloned()
            .ok_or(SkillError::NotFound)
    }

    /// Where a read made before the transaction starts: the workspace and
    /// what the last scan holds for `slug`, read under one lock. A change
    /// that records the read checks `SkillRegistry::scan_moved` with them.
    pub(super) async fn read_start(
        &self,
        slug: &str,
    ) -> Result<(PathBuf, Option<Option<String>>), SkillError> {
        let guard = self.state.read().await;
        let root = guard
            .workspace
            .as_ref()
            .map(|workspace| workspace.root_path.clone())
            .ok_or(SkillError::NoWorkspace)?;
        Ok((root, guard.skills.scanned_hash(slug)))
    }

    /// Reads what `FileReview::check` needs, before the transaction is
    /// taken (a slow or stuck folder never holds it up): the skill's
    /// `SKILL.md` hash, unless the skill has a record now.
    pub(super) async fn review_file(&self, slug: &str) -> Result<FileReview, SkillError> {
        let (root, has_record) = {
            let guard = self.state.read().await;
            let root = guard
                .workspace
                .as_ref()
                .map(|workspace| workspace.root_path.clone())
                .ok_or(SkillError::NoWorkspace)?;
            (root, guard.skills.get(slug).is_some())
        };
        let read = if has_record {
            None
        } else {
            let (workspace, target) = (root.clone(), slug.to_string());
            let read = blocking(move || super::package::read_package(&workspace, &target)).await?;
            Some(
                read.map(|package| package.map(|package| package.hash()))
                    .map_err(|_| ()),
            )
        };
        Ok(FileReview {
            root,
            slug: slug.to_string(),
            read,
        })
    }

    /// Rescans the skills folder (spec §8.1); `true` when a status or the
    /// file drafts changed, which is announced as `skill.updated`.
    pub(crate) async fn scan(&self) -> Result<bool, SkillError> {
        let input = self.scan_input().await?;
        self.scan_with(input).await
    }

    /// What a scan starts from, read under one lock so the workspace and the
    /// generation always belong together.
    async fn scan_input(&self) -> Result<ScanInput, SkillError> {
        let guard = self.state.read().await;
        let root = guard
            .workspace
            .as_ref()
            .map(|workspace| workspace.root_path.clone())
            .ok_or(SkillError::NoWorkspace)?;
        Ok(ScanInput {
            root,
            generation: guard.skills.generation(),
            previous: guard.skills.scanned_files().clone(),
            registered: guard
                .skills
                .records()
                .iter()
                .map(|record| record.slug.clone())
                .collect::<BTreeSet<_>>(),
        })
    }

    async fn scan_with(&self, input: ScanInput) -> Result<bool, SkillError> {
        let ScanInput {
            root,
            generation,
            previous,
            registered,
        } = input;
        let scanned = blocking(move || disk::scan_skills_folder(&root, &previous, &registered))
            .await?
            .map_err(SkillError::Unavailable)?;
        let mut guard = self.state.write().await;
        let changed = guard.skills.apply_scan(generation, scanned) == Some(true);
        if changed {
            guard.publish_skill_updated(None, None);
        }
        Ok(changed)
    }

    /// Every skill, by slug, after a rescan (spec §8.1: the Skills page's
    /// requests rescan). A failed scan is logged; the last state is listed.
    pub(crate) async fn list(&self) -> Result<Vec<SkillRecord>, SkillError> {
        match self.scan().await {
            Ok(_) => {}
            Err(SkillError::NoWorkspace) => return Err(SkillError::NoWorkspace),
            Err(error) => {
                warn!(error = %error.message(), "skills scan failed; listing the last known state")
            }
        }
        Ok(self
            .state
            .read()
            .await
            .skills
            .records()
            .into_iter()
            .cloned()
            .collect())
    }

    /// A skill's record and current file; 404 when it has neither.
    pub(crate) async fn detail(&self, slug: &str) -> Result<SkillDetail, SkillError> {
        if !is_valid_slug(slug) {
            return Err(SkillError::NotFound);
        }
        let root = self.workspace().await?;
        let previous = self.state.read().await.skills.scanned(slug).cloned();
        let target = slug.to_string();
        let file = blocking(move || disk::scan_skill(&root, &target, previous.as_ref()))
            .await?
            .map_err(SkillError::Unavailable)?;
        let record = self.state.read().await.skills.get(slug).cloned();
        if record.is_none() && file.is_none() {
            return Err(SkillError::NotFound);
        }
        Ok(SkillDetail { record, file })
    }

    /// The owner creates or replaces a skill's content, which approves it
    /// (spec §8.4). Returns the record and whether it is new. A folder whose
    /// `SKILL.md` the owner has not reviewed is refused.
    pub(crate) async fn save(
        &self,
        slug: &str,
        content: SkillContent,
    ) -> Result<(SkillRecord, bool), SkillError> {
        if !is_valid_slug(slug) {
            return Err(SkillError::invalid(SKILL_SLUG_INVALID));
        }
        let file = SkillFile {
            name: validate_name(&content.name).map_err(SkillError::invalid)?,
            description: validate_description(&content.description).map_err(SkillError::invalid)?,
            body: content.body,
        };
        validate_body(&file.body).map_err(SkillError::invalid)?;
        let (slug, enabled) = (slug.to_string(), content.enabled);
        let review = self.review_file(&slug).await?;
        self.locked(move |service, root| async move {
            let bytes = compose_skill_file(&file.name, &file.description, &file.body).into_bytes();
            let files = match content.files {
                Some(files) => files,
                None => {
                    let (workspace, target) = (root.clone(), slug.clone());
                    let existing =
                        match blocking(move || super::package::read_package(&workspace, &target))
                            .await
                            .and_then(|result| result.map_err(SkillError::Unavailable))
                        {
                            Ok(existing) => existing,
                            Err(error) => {
                                // A preflight failure bypasses apply's write
                                // rollback; discard the old scan here too.
                                service.state.write().await.skills.set_scanned(&slug, None);
                                return Err(error);
                            }
                        };
                    let approved_hash = service
                        .state
                        .read()
                        .await
                        .skills
                        .get(&slug)
                        .map(|record| record.approved_hash.clone());
                    if let Some(approved_hash) = approved_hash {
                        if existing.as_ref().map(|package| package.hash()).as_deref()
                            != Some(approved_hash.as_str())
                        {
                            service.state.write().await.skills.set_scanned(&slug, None);
                            return Err(SkillError::conflict(SKILL_HASH_MISMATCH));
                        }
                    } else if existing
                        .as_ref()
                        .is_some_and(|package| !package.files.is_empty())
                    {
                        // Rejecting a file draft authorizes replacing it with
                        // explicit owner content; its supporting files remain
                        // unapproved and cannot be inherited implicitly.
                        return Err(SkillError::conflict(SKILL_FILE_UNREVIEWED));
                    }
                    if existing.is_none() {
                        let (workspace, target) = (root.clone(), slug.clone());
                        if blocking(move || super::package::folder_has_content(&workspace, &target))
                            .await?
                            .map_err(SkillError::Unavailable)?
                        {
                            return Err(SkillError::conflict(SKILL_FILE_UNREVIEWED));
                        }
                    }
                    existing.map(|package| package.files).unwrap_or_default()
                }
            };
            anima_core::skill_packages::validate_skill_package(&bytes, &files)
                .map_err(SkillError::invalid)?;
            let package = super::package::SkillPackage { bytes, files };
            crate::community_bundles::check_skill_edit(
                &*service.state.read().await,
                &slug,
                enabled.unwrap_or(true),
            )?;
            let hash = package.hash();
            let (target, written_root) = (slug.clone(), root.clone());
            let created = service
                .apply_with_history(
                    &root,
                    Some(super::ledger::LedgerChange::Supersede(target.clone())),
                    None,
                    move |skills, now_ms| {
                        review.check(skills, &written_root, &target)?;
                        let previous = skills.get(&target).cloned();
                        let mut record = SkillRecord::approved(&target, &file, hash, now_ms);
                        record.enabled = enabled
                            .unwrap_or_else(|| previous.as_ref().is_none_or(|known| known.enabled));
                        skills.put(record).map_err(SkillError::conflict)?;
                        let created = previous.is_none();
                        let undo_slug = target.clone();
                        Ok(Change {
                            value: created,
                            undo: Box::new(move |skills: &mut SkillRegistry| {
                                skills.restore(&undo_slug, previous)
                            }),
                            write: Some((target.clone(), package)),
                            slug: Some(target),
                            draft_id: None,
                        })
                    },
                )
                .await?;
            Ok((service.record(&slug).await?, created))
        })
        .await
    }

    /// Turns a skill on or off (spec §8.4 `PATCH`).
    pub(crate) async fn set_enabled(
        &self,
        slug: &str,
        enabled: bool,
    ) -> Result<SkillRecord, SkillError> {
        let slug = slug.to_string();
        self.locked(move |service, root| async move {
            crate::community_bundles::check_skill_edit(
                &*service.state.read().await,
                &slug,
                enabled,
            )?;
            let target = slug.clone();
            service
                .apply_with_history(
                    &root,
                    Some(super::ledger::LedgerChange::Supersede(target.clone())),
                    None,
                    move |skills, now_ms| {
                        let previous = skills.get(&target).cloned().ok_or(SkillError::NotFound)?;
                        let mut record = previous.clone();
                        record.enabled = enabled;
                        record.updated_at_ms = now_ms;
                        skills.put(record).map_err(SkillError::conflict)?;
                        let undo_slug = target.clone();
                        Ok(Change {
                            value: (),
                            undo: Box::new(move |skills: &mut SkillRegistry| {
                                skills.restore(&undo_slug, Some(previous))
                            }),
                            write: None,
                            slug: Some(target),
                            draft_id: None,
                        })
                    },
                )
                .await?;
            service.record(&slug).await
        })
        .await
    }

    /// Deletes a skill (spec §8.2): its folder moves to the workspace trash
    /// and its record goes. Returns the workspace-relative trash path
    /// (`None` when it had no folder). A failed save puts the record back
    /// and, when it can, the folder.
    pub(crate) async fn delete(&self, slug: &str) -> Result<Option<String>, SkillError> {
        if !is_valid_slug(slug) {
            return Err(SkillError::NotFound);
        }
        let slug = slug.to_string();
        self.locked(
            move |service, root| async move { service.delete_locked(root, slug, None).await },
        )
        .await
    }

    pub(super) async fn delete_locked(
        &self,
        root: PathBuf,
        slug: String,
        history: Option<super::ledger::LedgerChange>,
    ) -> Result<Option<String>, SkillError> {
        let service = self;
        let history =
            history.or_else(|| Some(super::ledger::LedgerChange::Supersede(slug.clone())));
        if crate::community_bundles::check_skill_edit(&*service.state.read().await, &slug, false)? {
            return Err(SkillError::conflict(
                "Remove this skill through its community bundle",
            ));
        }
        let (previous, owner, prior_ledger) = {
            let mut guard = service.state.write().await;
            guard
                .ensure_profile_owned()
                .map_err(|error| SkillError::Unavailable(error.to_string()))?;
            if let Some(history) = &history {
                history.check(&guard)?;
            }
            (
                guard.skills.remove(&slug),
                guard.profile_owner(),
                history.as_ref().map(|_| guard.skill_ledger.clone()),
            )
        };
        let (workspace, target) = (root.clone(), slug.clone());
        let move_owner = owner.clone();
        let moved = blocking(move || {
            if let Some(owner) = &move_owner {
                owner.ensure_owned().map_err(|error| error.to_string())?;
            }
            disk::trash_skill_folder(&workspace, &target, now_millis())
        })
        .await
        .and_then(|result| result.map_err(SkillError::Unavailable));
        let trashed = match moved {
            Ok(trashed) => trashed,
            Err(error) => {
                service.state.write().await.skills.restore(&slug, previous);
                return Err(error);
            }
        };
        if previous.is_none() && trashed.is_none() {
            return Err(SkillError::NotFound);
        }
        let persist = {
            let mut guard = service.state.write().await;
            guard.skills.set_scanned(&slug, None);
            guard.skill_evaluations.invalidate(&slug);
            if let Some(history) = history {
                history.apply(&mut guard.skill_ledger, now_millis());
            }
            guard.control_plane_persist_request()
        };
        if let Err(error) = persist.save().await {
            if crate::control_plane_store::is_durability_uncertain(&error) {
                return Err(SkillError::Unavailable(error.to_string()));
            }
            {
                let mut guard = service.state.write().await;
                guard.skills.restore(&slug, previous);
                if let Some(ledger) = prior_ledger {
                    guard.skill_ledger = ledger;
                }
            }
            // Put the folder back and read it again in one bounded call.
            let (workspace, target) = (root.clone(), slug.clone());
            let back = blocking(move || {
                if let Some(owner) = &owner {
                    owner.ensure_owned().map_err(|error| error.to_string())?;
                }
                let untrashed = match trashed {
                    Some(name) => disk::untrash_skill_folder(&workspace, &target, &name),
                    None => Ok(()),
                };
                Ok((untrashed, disk::scan_skill(&workspace, &target, None)))
            })
            .await
            .and_then(|result| result.map_err(SkillError::Unavailable));
            let untrashed = match back {
                Ok((untrashed, scanned)) => {
                    if let Ok(scanned) = scanned {
                        service
                            .state
                            .write()
                            .await
                            .skills
                            .set_scanned(&slug, scanned);
                    }
                    untrashed
                }
                Err(problem) => Err(problem.message()),
            };
            if let Err(problem) = untrashed {
                warn!(
                    skill = %slug,
                    problem = %problem,
                    "a deleted skill's folder stays in the trash after its save failed"
                );
            }
            return Err(SkillError::Unavailable(error.to_string()));
        }
        service
            .state
            .read()
            .await
            .publish_skill_updated(Some(&slug), None);
        Ok(trashed.map(|name| disk::trash_relative_path(&name)))
    }

    /// Approves a `changed` skill's current `SKILL.md` (spec §8.4), which the
    /// owner reviewed as `hash`. The file is read and checked before the
    /// transaction is taken; the hash pinned is that of the bytes read, so a
    /// later edit reads `changed` (fail closed).
    pub(crate) async fn approve_changed(
        &self,
        slug: &str,
        hash: &str,
    ) -> Result<SkillRecord, SkillError> {
        if !is_valid_slug(slug) {
            return Err(SkillError::NotFound);
        }
        let reviewed = hash.trim().to_ascii_lowercase();
        if !is_hex_hash(&reviewed) {
            return Err(SkillError::invalid(SKILL_HASH_REQUIRED));
        }
        let slug = slug.to_string();
        let (read_root, seen) = self.read_start(&slug).await?;
        let (workspace, target) = (read_root.clone(), slug.clone());
        let package = blocking(move || super::package::read_package(&workspace, &target))
            .await?
            .map_err(SkillError::Unavailable)?
            .ok_or(SkillError::NotFound)?;
        let current = package.hash();
        if current != reviewed {
            return Err(SkillError::conflict(SKILL_HASH_MISMATCH));
        }
        let file = parse_skill_file(&package.bytes).map_err(SkillError::invalid)?;
        let scanned = ScannedFile::package(&package, None);
        self.locked(move |service, root| async move {
            if root != read_root {
                return Err(SkillError::conflict(SKILL_HASH_MISMATCH));
            }
            crate::community_bundles::check_skill_edit(&*service.state.read().await, &slug, true)?;
            let target = slug.clone();
            service
                .apply_with_history(
                    &root,
                    Some(super::ledger::LedgerChange::Supersede(target.clone())),
                    None,
                    move |skills, now_ms| {
                        let previous = skills.get(&target).cloned().ok_or(SkillError::NotFound)?;
                        if previous.approved_hash == current {
                            return Err(SkillError::conflict(SKILL_NOT_CHANGED));
                        }
                        if skills.scan_moved(&target, &seen, &scanned.hash) {
                            return Err(SkillError::conflict(SKILL_HASH_MISMATCH));
                        }
                        let mut record = SkillRecord::approved(&target, &file, current, now_ms);
                        record.enabled = previous.enabled;
                        skills.put(record).map_err(SkillError::conflict)?;
                        skills.set_scanned(&target, Some(scanned));
                        let undo_slug = target.clone();
                        Ok(Change {
                            value: (),
                            undo: Box::new(move |skills: &mut SkillRegistry| {
                                skills.restore(&undo_slug, Some(previous))
                            }),
                            write: None,
                            slug: Some(target),
                            draft_id: None,
                        })
                    },
                )
                .await?;
            service.record(&slug).await
        })
        .await
    }

    /// An enabled skill's instructions (spec §8.3), found by slug (with or
    /// without `/`) or by name, as the model's `load_skill { name }` asks.
    pub(crate) async fn load(&self, name: &str) -> Result<LoadedSkill, String> {
        self.load_file(name, None).await
    }

    pub(crate) async fn load_file(
        &self,
        name: &str,
        file_path: Option<&str>,
    ) -> Result<LoadedSkill, String> {
        let (root, record) = {
            let guard = self.state.read().await;
            let Some(root) = guard
                .workspace
                .as_ref()
                .map(|workspace| workspace.root_path.clone())
            else {
                return Err(SKILLS_NEED_WORKSPACE.to_string());
            };
            (
                root,
                guard
                    .skills
                    .find(name)
                    .filter(|record| crate::community_bundles::runnable_skill(&guard, &record.slug))
                    .cloned(),
            )
        };
        let Some(record) = record else {
            return Err(skill_not_found(name.trim()));
        };
        load_record_file(root, record, file_path).await
    }

    /// An enabled skill's instructions by exact slug, with no name fallback
    /// (a `/skill` message names a slug).
    pub(crate) async fn load_slug(&self, slug: &str) -> Result<LoadedSkill, String> {
        let (root, record) = {
            let guard = self.state.read().await;
            let Some(root) = guard
                .workspace
                .as_ref()
                .map(|workspace| workspace.root_path.clone())
            else {
                return Err(SKILLS_NEED_WORKSPACE.to_string());
            };
            (
                root,
                guard
                    .skills
                    .get(slug)
                    .filter(|record| crate::community_bundles::runnable_skill(&guard, &record.slug))
                    .cloned(),
            )
        };
        let Some(record) = record else {
            return Err(skill_not_found(slug));
        };
        load_record(root, record).await
    }

    /// Whether `slug` may be sent with a message now (spec §4.2's 400s).
    pub(crate) async fn check_runnable(&self, slug: &str) -> Result<(), &'static str> {
        let guard = self.state.read().await;
        if guard.workspace.is_none() {
            return Err(UNKNOWN_SKILL);
        }
        if !crate::community_bundles::runnable_skill(&guard, slug) {
            return Err(SKILL_NOT_RUNNABLE);
        }
        match guard.skills.runnable(slug) {
            None => Err(UNKNOWN_SKILL),
            Some(false) => Err(SKILL_NOT_RUNNABLE),
            Some(true) => Ok(()),
        }
    }

    /// The skills a run lists (spec §8.3).
    pub(crate) async fn index(&self) -> Vec<SkillRecord> {
        let guard = self.state.read().await;
        if guard.workspace.is_none() {
            return Vec::new();
        }
        guard
            .skills
            .index()
            .into_iter()
            .filter(|record| crate::community_bundles::runnable_skill(&guard, &record.slug))
            .cloned()
            .collect()
    }
}

/// Reads `record`'s `SKILL.md` now and checks it against the approved hash:
/// a file changed since is refused, whatever the last scan said.
async fn load_record(root: PathBuf, record: SkillRecord) -> Result<LoadedSkill, String> {
    load_record_file(root, record, None).await
}
async fn load_record_file(
    root: PathBuf,
    record: SkillRecord,
    file_path: Option<&str>,
) -> Result<LoadedSkill, String> {
    if let Some(path) = file_path {
        anima_core::skill_packages::validate_supporting_path(path).map_err(str::to_string)?;
    }
    if !record.enabled {
        return Err(SKILL_DISABLED.to_string());
    }
    let slug = record.slug.clone();
    let read = blocking(move || super::package::read_package(&root, &slug))
        .await
        .map_err(|error| error.message())?;
    let package = match read {
        Ok(Some(package)) => package,
        Ok(None) => return Err(SKILL_MISSING.to_string()),
        Err(_) => return Err(SKILL_CHANGED.to_string()),
    };
    if package.hash() != record.approved_hash {
        return Err(SKILL_CHANGED.to_string());
    }
    let file = parse_skill_file(&package.bytes).map_err(|_| SKILL_CHANGED.to_string())?;
    let body = if let Some(path) = file_path {
        package
            .files
            .iter()
            .find(|f| f.path == path)
            .map(|f| format!("Reviewed supporting file: {path}\n\n{}", f.content))
            .ok_or_else(|| "No reviewed supporting file has this path".to_string())?
    } else if package.files.is_empty() {
        file.body
    } else {
        format!("{}\n\nSupporting files (reviewed; use load_skill with name and filePath to read one):\n{}\nScripts remain inert and require existing execution permissions to run.",file.body,package.files.iter().map(|f|format!("- {} ({} bytes)",f.path,f.content.len())).collect::<Vec<_>>().join("\n"))
    };
    Ok(LoadedSkill {
        slug: record.slug,
        name: record.name,
        body,
        approved_hash: record.approved_hash,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::RwLock;

    use super::*;
    use crate::agent_runs::test_support::{companion_config, next_event};
    use crate::control_plane_store::WorkspaceConfig;
    use crate::skills::test_support::{
        broken_store, content, service, skill_text, temp_workspace, with_workspace, write_skill,
    };
    use crate::skills::{
        skill_hash, SkillStatus, SKILL_BODY_EMPTY, SKILL_CHANGED, SKILL_DISABLED,
        SKILL_FILE_UNREVIEWED, SKILL_HASH_MISMATCH, SKILL_HASH_REQUIRED, SKILL_MISSING,
        SKILL_NOT_CHANGED, SKILL_SLUG_INVALID,
    };
    use crate::state::DaemonState;

    /// A daemon with a workspace and one companion, and the companion's id.
    async fn daemon(label: &str) -> (SharedDaemonState, std::path::PathBuf, String) {
        let root = temp_workspace(label);
        let mut state = with_workspace(DaemonState::new(), &root);
        let agent_id = state
            .create_agent(companion_config("companion"))
            .unwrap()
            .state
            .id;
        (Arc::new(RwLock::new(state)), root, agent_id)
    }

    #[tokio::test]
    async fn profile_owner_custody_refuses_skill_save_before_file_write() {
        let private = tempfile::tempdir().unwrap();
        let root = private.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let owner = crate::profile_owner::ProfileOwner::acquire([private
            .path()
            .join("control-plane.json")])
        .unwrap();
        let mut daemon = with_workspace(DaemonState::new(), &root);
        daemon.profile_owner = Some(owner.clone());
        let state = Arc::new(RwLock::new(daemon));
        owner.mark_lost();

        let result = service(&state).save("notes", content("notes")).await;

        assert!(result.is_err(), "a revoked profile accepted a skill save");
        assert!(
            !root.join("skills/notes/SKILL.md").exists(),
            "the guarded registry save failed after unauthorized skill bytes were written"
        );
        assert!(state.read().await.skills.get("notes").is_none());
    }

    #[tokio::test]
    async fn profile_owner_custody_rechecks_before_package_write() {
        let private = tempfile::tempdir().unwrap();
        let root = private.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let owner = crate::profile_owner::ProfileOwner::acquire([private
            .path()
            .join("control-plane.json")])
        .unwrap();
        let mut daemon = with_workspace(DaemonState::new(), &root);
        daemon.profile_owner = Some(owner.clone());
        let state = Arc::new(RwLock::new(daemon));
        let package = super::super::package::SkillPackage {
            bytes: skill_text("notes").into_bytes(),
            files: vec![],
        };

        let result = service(&state)
            .apply(&root, move |_, _| {
                // Simulate loss after admission and before the blocking write.
                owner.mark_lost();
                Ok(Change {
                    value: (),
                    undo: Box::new(|_| {}),
                    write: Some(("notes".into(), package)),
                    slug: Some("notes".into()),
                    draft_id: None,
                })
            })
            .await;

        assert!(result.is_err());
        assert!(
            !root.join("skills/notes/SKILL.md").exists(),
            "ownership was checked only before scheduling the package write"
        );
    }

    #[tokio::test]
    async fn package_supporting_change_invalidates_approval_even_without_rescan() {
        let (state, root, _) = daemon("package-change").await;
        let skills = service(&state);
        skills.save("notes", content("notes")).await.unwrap();
        std::fs::create_dir_all(root.join("skills/notes/templates")).unwrap();
        std::fs::write(
            root.join("skills/notes/templates/note.md"),
            "First template",
        )
        .unwrap();
        skills.scan().await.unwrap();
        assert_eq!(
            skills.record("notes").await.unwrap().status,
            SkillStatus::Changed
        );
        assert_eq!(skills.load("notes").await.unwrap_err(), SKILL_CHANGED);
    }

    #[tokio::test]
    async fn package_rejects_unsupported_files_instead_of_silently_approving_them() {
        let (state, root, _) = daemon("package-unsupported").await;
        write_skill(&root, "notes", &skill_text("notes"));
        std::fs::write(root.join("skills/notes/hidden.exe"), "inert test").unwrap();
        let detail = service(&state).detail("notes").await.unwrap();
        assert!(detail.file.unwrap().problem().is_some());
    }

    #[tokio::test]
    async fn package_save_load_review_and_restart_pin_all_files() {
        let (state, root, _) = daemon("package-roundtrip").await;
        let skills = service(&state);
        let mut input = content("notes");
        input.files = Some(vec![
            super::super::SkillSupportingFile {
                path: "templates/note.md".into(),
                content: "Reviewed template".into(),
            },
            super::super::SkillSupportingFile {
                path: "scripts/example.py".into(),
                content: "print('inert')".into(),
            },
        ]);
        let (record, _) = skills.save("notes", input).await.unwrap();
        let default = skills.load("notes").await.unwrap();
        assert!(default.body.contains("templates/note.md (17 bytes)"));
        assert!(!default.body.contains("Reviewed template"));
        let loaded = skills
            .load_file("notes", Some("templates/note.md"))
            .await
            .unwrap();
        assert!(loaded.body.ends_with("Reviewed template"));
        assert!(skills
            .load_file("notes", Some("../outside.md"))
            .await
            .is_err());
        assert!(skills
            .load_file("notes", Some("references/missing.md"))
            .await
            .is_err());
        let snapshot = state.read().await.control_plane_snapshot();
        let mut restarted = with_workspace(DaemonState::new(), &root);
        restarted.restore_control_plane_snapshot(snapshot).unwrap();
        let restored = service(&Arc::new(RwLock::new(restarted)));
        assert_eq!(
            restored
                .load_file("notes", Some("templates/note.md"))
                .await
                .unwrap(),
            loaded
        );
        std::fs::write(
            root.join("skills/notes/templates/note.md"),
            "Changed template",
        )
        .unwrap();
        assert_eq!(
            restored
                .load_file("notes", Some("scripts/example.py"))
                .await
                .unwrap_err(),
            SKILL_CHANGED
        );
        let detail = restored.detail("notes").await.unwrap();
        let new_hash = detail.file.unwrap().hash.unwrap();
        assert_ne!(new_hash, record.approved_hash);
        assert!(restored
            .approve_changed("notes", &record.approved_hash)
            .await
            .is_err());
        restored.approve_changed("notes", &new_hash).await.unwrap();
        assert!(restored
            .load_file("notes", Some("templates/note.md"))
            .await
            .unwrap()
            .body
            .ends_with("Changed template"));
        let mut remove = content("notes");
        remove.files = Some(Vec::new());
        let (single, _) = restored.save("notes", remove).await.unwrap();
        assert_eq!(
            single.approved_hash,
            skill_hash(&std::fs::read(root.join("skills/notes/SKILL.md")).unwrap())
        );
        assert!(!root.join("skills/notes/templates/note.md").exists());
        assert!(root.join(".anima-skill-transactions").exists());
    }

    #[tokio::test]
    async fn package_failed_persistence_keeps_new_package_unapproved() {
        let (state, root, _) = daemon("package-save-failure").await;
        let skills = service(&state);
        let (original, _) = skills.save("notes", content("notes")).await.unwrap();
        state
            .write()
            .await
            .set_control_plane_store(Some(broken_store()));
        let mut next = content("notes");
        next.files = Some(vec![super::super::SkillSupportingFile {
            path: "templates/note.md".into(),
            content: "Uncommitted template".into(),
        }]);
        assert!(matches!(
            skills.save("notes", next).await,
            Err(SkillError::Unavailable(_))
        ));
        assert_eq!(
            state
                .read()
                .await
                .skills
                .get("notes")
                .unwrap()
                .approved_hash,
            original.approved_hash
        );
        assert_eq!(skills.load("notes").await.unwrap_err(), SKILL_CHANGED);
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/templates/note.md")).unwrap(),
            "Uncommitted template"
        );
    }

    #[tokio::test]
    async fn owner_body_save_cannot_implicitly_approve_unreviewed_supporting_changes() {
        let (state, root, _) = daemon("package-implicit-review").await;
        let skills = service(&state);
        let mut original = content("notes");
        original.files = Some(vec![crate::skills::SkillSupportingFile {
            path: "templates/note.md".into(),
            content: "Reviewed template".into(),
        }]);
        let (record, _) = skills.save("notes", original).await.unwrap();
        std::fs::write(
            root.join("skills/notes/templates/note.md"),
            "Unreviewed replacement",
        )
        .unwrap();
        assert_eq!(
            skills.save("notes", content("notes")).await.unwrap_err(),
            SkillError::Conflict(SKILL_HASH_MISMATCH.into())
        );
        assert_eq!(
            state
                .read()
                .await
                .skills
                .get("notes")
                .unwrap()
                .approved_hash,
            record.approved_hash
        );
        assert_eq!(skills.load("notes").await.unwrap_err(), SKILL_CHANGED);
    }

    #[tokio::test]
    async fn implicit_create_preserves_an_unregistered_partial_package_until_explicit_save() {
        let (state, root, _) = daemon("package-partial-create").await;
        let skills = service(&state);
        std::fs::create_dir_all(root.join("skills/notes/templates")).unwrap();
        std::fs::write(
            root.join("skills/notes/templates/note.md"),
            "Existing local template",
        )
        .unwrap();
        assert_eq!(
            skills.save("notes", content("notes")).await.unwrap_err(),
            SkillError::Conflict(SKILL_FILE_UNREVIEWED.into())
        );
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/templates/note.md")).unwrap(),
            "Existing local template"
        );
        let mut explicit = content("notes");
        explicit.files = Some(Vec::new());
        skills.save("notes", explicit).await.unwrap();
        assert!(skills.load("notes").await.is_ok());
        assert!(root.join(".anima-skill-transactions").exists());
    }

    #[tokio::test]
    async fn rejected_complete_package_cannot_supply_implicit_supporting_files() {
        let (state, root, _) = daemon("package-rejected-implicit").await;
        let skills = service(&state);
        write_skill(&root, "notes", &skill_text("Unregistered notes"));
        std::fs::create_dir_all(root.join("skills/notes/templates")).unwrap();
        std::fs::write(
            root.join("skills/notes/templates/note.md"),
            "Rejected supporting instructions",
        )
        .unwrap();
        skills.reject_draft("file:notes").await.unwrap();

        assert_eq!(
            skills.save("notes", content("notes")).await.unwrap_err(),
            SkillError::Conflict(SKILL_FILE_UNREVIEWED.into())
        );
        assert!(state.read().await.skills.get("notes").is_none());
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/templates/note.md")).unwrap(),
            "Rejected supporting instructions"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/SKILL.md")).unwrap(),
            skill_text("Unregistered notes")
        );

        let mut explicit = content("notes");
        explicit.files = Some(vec![crate::skills::SkillSupportingFile {
            path: "templates/note.md".into(),
            content: "Explicit owner replacement".into(),
        }]);
        skills.save("notes", explicit).await.unwrap();
        assert!(skills
            .load_file("notes", Some("templates/note.md"))
            .await
            .unwrap()
            .body
            .ends_with("Explicit owner replacement"));
    }

    #[tokio::test]
    async fn saving_writes_the_file_and_approves_its_hash() {
        let (state, root, _) = daemon("save").await;
        let skills = service(&state);

        let (record, created) = skills.save("notes", content("notes")).await.unwrap();

        assert!(created);
        let written = std::fs::read(root.join("skills/notes/SKILL.md")).unwrap();
        assert_eq!(written, skill_text("notes").into_bytes());
        assert_eq!(record.approved_hash, skill_hash(&written));
        assert_eq!(record.status, SkillStatus::Active);
        assert!(record.enabled);

        let mut off = content("notes");
        off.enabled = Some(false);
        let (record, created) = skills.save("notes", off).await.unwrap();
        assert!(!created);
        assert!(!record.enabled);
        let (record, _) = skills.save("notes", content("notes")).await.unwrap();
        assert!(!record.enabled, "a save without `enabled` keeps the switch");
    }

    #[tokio::test]
    async fn saving_refuses_bad_input_and_needs_a_workspace() {
        let (state, _, _) = daemon("save-bad").await;
        let skills = service(&state);
        assert_eq!(
            skills.save("Bad Slug", content("x")).await,
            Err(SkillError::Invalid(SKILL_SLUG_INVALID.into()))
        );
        let mut empty = content("x");
        empty.body = "  ".into();
        assert_eq!(
            skills.save("x", empty).await,
            Err(SkillError::Invalid(SKILL_BODY_EMPTY.into()))
        );
        state.write().await.workspace = None;
        assert_eq!(
            skills.save("x", content("x")).await,
            Err(SkillError::NoWorkspace)
        );
        assert_eq!(skills.list().await, Err(SkillError::NoWorkspace));
    }

    #[tokio::test]
    async fn saving_over_a_file_the_owner_has_not_reviewed_is_refused() {
        let (state, root, _) = daemon("save-unreviewed").await;
        let skills = service(&state);
        let path = write_skill(&root, "notes", &skill_text("Written by hand"));

        assert_eq!(
            skills.save("notes", content("notes")).await,
            Err(SkillError::Conflict(SKILL_FILE_UNREVIEWED.into()))
        );
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            skill_text("Written by hand")
        );
        assert!(state.read().await.skills.get("notes").is_none());
    }

    #[tokio::test]
    async fn a_failed_save_puts_the_record_back_and_leaves_the_file_changed() {
        let (state, root, _) = daemon("save-fail").await;
        let skills = service(&state);
        let (first, _) = skills.save("notes", content("notes")).await.unwrap();
        state
            .write()
            .await
            .set_control_plane_store(Some(broken_store()));

        let mut edited = content("notes");
        edited.body = "Do it differently.".into();
        let refused = skills.save("notes", edited).await;

        assert!(
            matches!(refused, Err(SkillError::Unavailable(_))),
            "{refused:?}"
        );
        let guard = state.read().await;
        let record = guard.skills.get("notes").unwrap();
        assert_eq!(record.approved_hash, first.approved_hash);
        assert_eq!(
            record.status,
            SkillStatus::Changed,
            "the new file stays on disk and is not trusted"
        );
        drop(guard);
        assert!(std::fs::read_to_string(root.join("skills/notes/SKILL.md"))
            .unwrap()
            .contains("Do it differently."));
        assert_eq!(skills.load("notes").await, Err(SKILL_CHANGED.to_string()));
    }

    #[tokio::test]
    async fn turning_a_skill_off_keeps_it_from_loading() {
        let (state, _, _) = daemon("enable").await;
        let skills = service(&state);
        skills.save("notes", content("notes")).await.unwrap();

        let record = skills.set_enabled("notes", false).await.unwrap();
        assert!(!record.enabled);
        assert_eq!(skills.load("notes").await, Err(SKILL_DISABLED.to_string()));
        assert_eq!(
            skills.check_runnable("notes").await,
            Err(crate::skills::SKILL_NOT_RUNNABLE)
        );
        assert!(skills.set_enabled("notes", true).await.unwrap().enabled);
        assert_eq!(skills.check_runnable("notes").await, Ok(()));
        assert_eq!(
            skills.check_runnable("ghost").await,
            Err(crate::skills::UNKNOWN_SKILL)
        );
        assert_eq!(
            skills.set_enabled("ghost", true).await,
            Err(SkillError::NotFound)
        );
    }

    #[tokio::test]
    async fn a_rescan_marks_an_edited_file_changed_and_announces_it() {
        let (state, root, agent_id) = daemon("rescan").await;
        let mut stream = state.read().await.live.subscribe(&agent_id).unwrap();
        let skills = service(&state);
        skills.save("notes", content("notes")).await.unwrap();
        let saved = next_event(&mut stream).await.to_json(1);
        assert_eq!(saved["type"], "skill.updated");
        assert_eq!(saved["slug"], "notes");

        write_skill(&root, "notes", &skill_text("notes, edited by hand"));
        assert_eq!(skills.scan().await, Ok(true));
        assert_eq!(
            state.read().await.skills.get("notes").unwrap().status,
            SkillStatus::Changed
        );
        let scanned = next_event(&mut stream).await.to_json(2);
        assert_eq!(scanned["type"], "skill.updated");
        assert_eq!(scanned["slug"], serde_json::Value::Null);
        assert_eq!(skills.scan().await, Ok(false), "nothing new");
    }

    #[tokio::test]
    async fn load_refuses_an_edited_file_even_before_a_rescan() {
        let (state, root, _) = daemon("load").await;
        let skills = service(&state);
        skills.save("notes", content("Notes")).await.unwrap();

        let loaded = skills.load("/notes").await.unwrap();
        assert_eq!(loaded.body, "Do Notes.");
        assert_eq!(skills.load("NOTES").await.unwrap().slug, "notes", "by name");
        assert_eq!(skills.index().await.len(), 1);

        write_skill(&root, "notes", &skill_text("Injected"));
        assert_eq!(
            state.read().await.skills.get("notes").unwrap().status,
            SkillStatus::Active,
            "no scan has run yet"
        );
        assert_eq!(skills.load("notes").await, Err(SKILL_CHANGED.to_string()));

        std::fs::remove_file(root.join("skills/notes/SKILL.md")).unwrap();
        assert_eq!(skills.load("notes").await, Err(SKILL_MISSING.to_string()));
        assert_eq!(
            skills.load("ghost").await,
            Err(crate::skills::skill_not_found("ghost"))
        );
    }

    #[tokio::test]
    async fn load_slug_does_not_fall_back_to_a_name() {
        let (state, _, _) = daemon("load-slug").await;
        let skills = service(&state);
        skills.save("plan", content("notes")).await.unwrap();

        assert_eq!(
            skills.load_slug("notes").await,
            Err(crate::skills::skill_not_found("notes"))
        );
        assert_eq!(skills.load("notes").await.unwrap().slug, "plan");
        assert_eq!(skills.load_slug("plan").await.unwrap().slug, "plan");
    }

    #[tokio::test]
    async fn approving_a_changed_skill_needs_the_reviewed_hash() {
        let (state, root, _) = daemon("approve").await;
        let skills = service(&state);
        skills.save("notes", content("notes")).await.unwrap();
        skills.set_enabled("notes", false).await.unwrap();
        write_skill(&root, "notes", &skill_text("Notes v2"));
        let reviewed = skill_hash(skill_text("Notes v2").as_bytes());

        let short = "0".repeat(63);
        for malformed in ["", "   ", "not-a-hash", short.as_str()] {
            assert_eq!(
                skills.approve_changed("notes", malformed).await,
                Err(SkillError::Invalid(SKILL_HASH_REQUIRED.into())),
                "{malformed:?}"
            );
        }
        assert_eq!(
            skills.approve_changed("notes", &"0".repeat(64)).await,
            Err(SkillError::Conflict(SKILL_HASH_MISMATCH.into()))
        );
        let approved = skills.approve_changed("notes", &reviewed).await.unwrap();
        assert_eq!(approved.approved_hash, reviewed);
        assert_eq!(approved.name, "Notes v2");
        assert_eq!(approved.status, SkillStatus::Active);
        assert!(!approved.enabled, "approving keeps the switch");
        assert_eq!(
            skills.approve_changed("notes", &reviewed).await,
            Err(SkillError::Conflict(SKILL_NOT_CHANGED.into()))
        );
        assert_eq!(
            skills.approve_changed("ghost", &reviewed).await,
            Err(SkillError::NotFound)
        );
    }

    #[tokio::test]
    async fn approving_a_changed_skill_reads_the_file_outside_the_transaction() {
        let (state, root, _) = daemon("approve-outside").await;
        let skills = service(&state);
        skills.save("notes", content("notes")).await.unwrap();
        write_skill(&root, "notes", &skill_text("Notes v2"));
        let _held = skills.transactions.clone().lock_owned().await;

        let refused = tokio::time::timeout(
            Duration::from_secs(10),
            skills.approve_changed("notes", &"0".repeat(64)),
        )
        .await;

        assert_eq!(
            refused,
            Ok(Err(SkillError::Conflict(SKILL_HASH_MISMATCH.into())))
        );
    }

    /// Waits (bounded) until a change has finished its read and reached
    /// `locked`, which clones the service and its transaction: the test
    /// holds the transaction, so the change then waits there. `held` is the
    /// count with every test-side clone already made.
    async fn until_waiting_for_the_transaction(skills: &SkillService, held: usize) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while Arc::strong_count(&skills.transactions) <= held {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the change reached the transaction");
    }

    #[tokio::test]
    async fn saving_reads_the_file_outside_the_transaction() {
        let (state, root, _) = daemon("save-outside").await;
        let skills = service(&state);
        let path = write_skill(&root, "notes", &skill_text("Written by hand"));
        let held = skills.transactions.clone().lock_owned().await;
        let saver = skills.clone();
        let held_count = Arc::strong_count(&skills.transactions);

        let saving = tokio::spawn(async move { saver.save("notes", content("notes")).await });
        until_waiting_for_the_transaction(&skills, held_count).await;
        // The file goes while the save waits: only a read made before the
        // transaction still saw it.
        std::fs::remove_file(&path).unwrap();
        drop(held);

        let refused = tokio::time::timeout(Duration::from_secs(10), saving)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            refused,
            Err(SkillError::Conflict(SKILL_FILE_UNREVIEWED.into()))
        );
        assert!(state.read().await.skills.get("notes").is_none());
    }

    #[tokio::test]
    async fn an_approval_is_not_recorded_over_a_newer_scan() {
        let (state, root, _) = daemon("approve-newer-scan").await;
        let skills = service(&state);
        let (first, _) = skills.save("notes", content("notes")).await.unwrap();
        let v2 = skill_text("Notes v2");
        write_skill(&root, "notes", &v2);
        let held = skills.transactions.clone().lock_owned().await;
        let (approver, hash) = (skills.clone(), skill_hash(v2.as_bytes()));
        let held_count = Arc::strong_count(&skills.transactions);

        let approving = tokio::spawn(async move { approver.approve_changed("notes", &hash).await });
        until_waiting_for_the_transaction(&skills, held_count).await;
        write_skill(&root, "notes", &skill_text("Notes v3"));
        assert_eq!(skills.scan().await, Ok(true));
        drop(held);

        let refused = tokio::time::timeout(Duration::from_secs(10), approving)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            refused,
            Err(SkillError::Conflict(SKILL_HASH_MISMATCH.into())),
            "the read may be older than the scan, so it is not pinned"
        );
        let guard = state.read().await;
        let record = guard.skills.get("notes").unwrap();
        assert_eq!(record.approved_hash, first.approved_hash);
        assert_eq!(record.status, SkillStatus::Changed);
    }

    #[tokio::test]
    async fn approving_reports_a_file_that_cannot_be_read_as_unavailable() {
        let (state, root, _) = daemon("approve-unreadable").await;
        let skills = service(&state);
        skills.save("notes", content("notes")).await.unwrap();
        let file = root.join("skills/notes/SKILL.md");
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();

        let refused = skills.approve_changed("notes", &"0".repeat(64)).await;

        assert!(
            matches!(refused, Err(SkillError::Unavailable(_))),
            "{refused:?}"
        );
        assert_eq!(
            skills.approve_changed("notes", "nope").await,
            Err(SkillError::Invalid(SKILL_HASH_REQUIRED.into())),
            "malformed input stays a 400"
        );
    }

    #[tokio::test]
    async fn a_failed_write_does_not_leave_the_skill_reading_active() {
        let (state, root, _) = daemon("write-fail").await;
        let skills = service(&state);
        skills.save("notes", content("notes")).await.unwrap();
        assert_eq!(
            state.read().await.skills.get("notes").unwrap().status,
            SkillStatus::Active
        );
        let file = root.join("skills/notes/SKILL.md");
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();

        let mut edited = content("notes");
        edited.body = "Do it differently.".into();
        let refused = skills.save("notes", edited).await;

        assert!(
            matches!(refused, Err(SkillError::Unavailable(_))),
            "{refused:?}"
        );
        let guard = state.read().await;
        assert!(
            guard.skills.scanned("notes").is_none(),
            "the cache is stale"
        );
        assert_ne!(
            guard.skills.get("notes").unwrap().status,
            SkillStatus::Active,
            "a late write may still land, so the old scan cannot vouch for it"
        );
    }

    #[tokio::test]
    async fn a_scan_that_began_before_a_workspace_switch_is_dropped() {
        let (state, old_root, _) = daemon("scan-switch").await;
        let skills = service(&state);
        write_skill(&old_root, "old-skill", &skill_text("old-skill"));
        let input = skills.scan_input().await.unwrap();
        assert_eq!(input.root, old_root);

        let new_root = temp_workspace("scan-switch-new");
        state.write().await.set_workspace(Some(WorkspaceConfig {
            root_path: new_root,
            company_name: "Acme".into(),
            mission: "Ship carefully".into(),
            values: vec![],
        }));

        assert_eq!(skills.scan_with(input).await, Ok(false));
        let guard = state.read().await;
        assert!(
            guard.skills.file_drafts().is_empty(),
            "no old-workspace drafts"
        );
        assert!(guard.skills.scanned_files().is_empty());
    }

    #[tokio::test]
    async fn deleting_moves_the_folder_to_the_trash() {
        let (state, root, _) = daemon("delete").await;
        let skills = service(&state);
        skills.save("notes", content("notes")).await.unwrap();

        let trashed = skills.delete("notes").await.unwrap().unwrap();

        assert!(trashed.starts_with(".anima-trash/skills/notes-"));
        assert!(root.join(&trashed).join("SKILL.md").exists());
        assert!(!root.join("skills/notes").exists());
        assert!(state.read().await.skills.get("notes").is_none());
        assert_eq!(skills.delete("notes").await, Err(SkillError::NotFound));
    }

    #[tokio::test]
    async fn a_failed_delete_save_puts_the_record_and_the_folder_back() {
        let (state, root, _) = daemon("delete-fail").await;
        let skills = service(&state);
        skills.save("notes", content("notes")).await.unwrap();
        state
            .write()
            .await
            .set_control_plane_store(Some(broken_store()));

        assert!(matches!(
            skills.delete("notes").await,
            Err(SkillError::Unavailable(_))
        ));
        assert!(root.join("skills/notes/SKILL.md").exists());
        assert_eq!(
            state.read().await.skills.get("notes").unwrap().status,
            SkillStatus::Active
        );
    }

    #[tokio::test]
    async fn list_and_detail_rescan_and_show_the_file() {
        let (state, root, _) = daemon("detail").await;
        let skills = service(&state);
        skills.save("notes", content("notes")).await.unwrap();
        write_skill(&root, "found", &skill_text("found"));

        let listed = skills.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            state.read().await.skills.file_drafts()[0].draft.slug,
            "found",
            "the list rescanned"
        );
        let detail = skills.detail("notes").await.unwrap();
        assert_eq!(detail.record.unwrap().slug, "notes");
        assert_eq!(detail.file.unwrap().parsed.unwrap().body, "Do notes.");
        let found = skills.detail("found").await.unwrap();
        assert!(found.record.is_none());
        assert!(found.file.is_some());
        assert_eq!(
            skills.detail("ghost").await.err(),
            Some(SkillError::NotFound)
        );
        assert_eq!(
            skills.detail("../x").await.err(),
            Some(SkillError::NotFound)
        );
    }

    #[tokio::test]
    async fn the_scanner_rescans_until_shut_down() {
        let (state, root, _) = daemon("scanner").await;
        let skills = service(&state);
        skills.save("notes", content("notes")).await.unwrap();
        let scanner = crate::skills::SkillScanner::new(skills.clone())
            .with_interval(std::time::Duration::from_millis(20));
        scanner.start();

        write_skill(&root, "notes", &skill_text("notes, but longer"));
        let mut changed = false;
        for _ in 0..250 {
            if state.read().await.skills.get("notes").unwrap().status == SkillStatus::Changed {
                changed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(changed, "a scan ran within five seconds");
        tokio::time::timeout(std::time::Duration::from_secs(15), scanner.shutdown())
            .await
            .expect("the scanner stops");
    }
}
