use super::*;
use crate::app::SharedDaemonState;
use crate::control_plane_store::WorkspaceConfig;
use crate::mcp::{McpConnectionRecord, McpManager, McpRegistry, McpStdioSpec};
use crate::skills::{
    compose_skill_file, parse_skill_file, skill_hash, DraftSource, SkillDraft, SkillRegistry,
};
use anima_service_adapters::community_bundles::{
    parse_manifest, validate_manifest, ManifestComponent, RuntimeCompatibility,
};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

pub(super) fn runtime() -> RuntimeCompatibility {
    let mut capabilities = vec!["mcp.owner.v1".into(), "skills.import-draft.v1".into()];
    if cfg!(windows) {
        capabilities.push("mcp.stdio.windows.v1".into());
    }
    RuntimeCompatibility {
        host: "rust-daemon".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities,
    }
}
fn now() -> u64 {
    anima_core::primitives::now_millis()
}

#[derive(Clone)]
pub(crate) struct ReviewReceipt {
    pub(super) view: BundleReview,
    record: BundleRecord,
    mcp: Vec<McpConnectionRecord>,
    drafts: Vec<SkillDraft>,
    observation: String,
}

#[derive(Clone)]
pub(crate) struct BundleService {
    state: SharedDaemonState,
    transactions: Arc<Mutex<()>>,
    mcp: McpManager,
}
impl BundleService {
    pub(crate) fn new(state: SharedDaemonState, mcp: McpManager) -> Self {
        Self {
            state,
            transactions: mcp.control_plane_transactions(),
            mcp,
        }
    }
    async fn owned<T: Send + 'static>(
        &self,
        work: impl std::future::Future<Output = Result<T, BundleError>> + Send + 'static,
    ) -> Result<T, BundleError> {
        tokio::spawn(work)
            .await
            .map_err(|_| BundleError::Unavailable)?
    }
    fn owner(
        state: &crate::state::DaemonState,
        agent: &str,
    ) -> Result<WorkspaceConfig, BundleError> {
        if !state
            .agents
            .get(agent)
            .is_some_and(|row| !crate::agent_runs::is_helper_config(row.config()))
        {
            return Err(BundleError::NotFound);
        }
        state.workspace.clone().ok_or(BundleError::Conflict)
    }
    async fn record(&self, agent: &str, id: &str) -> Result<BundleRecord, BundleError> {
        let state = self.state.read().await;
        Self::owner(&state, agent)?;
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(BundleError::Invalid);
        }
        state
            .community_bundles
            .iter()
            .find(|row| row.id == id && row.agent_id == agent)
            .cloned()
            .ok_or(BundleError::NotFound)
    }
    async fn hashes(
        &self,
        record: Option<&BundleRecord>,
    ) -> Result<BTreeMap<String, Option<String>>, BundleError> {
        let mut hashes = BTreeMap::new();
        if let Some(record) = record {
            for part in record.skills() {
                let root = record.workspace.root_path.clone();
                let slug = part.slug.clone();
                let bytes = crate::skills::service::blocking(move || {
                    crate::skills::disk::read_skill_bytes(&root, &slug)
                })
                .await
                .map_err(|_| BundleError::Unavailable)?
                .map_err(|_| BundleError::Conflict)?;
                hashes.insert(part.slug.clone(), bytes.as_deref().map(skill_hash));
            }
        }
        Ok(hashes)
    }
    fn observation(
        state: &crate::state::DaemonState,
        agent: &str,
        hashes: &BTreeMap<String, Option<String>>,
    ) -> Result<String, BundleError> {
        let workspace = Self::owner(state, agent)?;
        let skills = state.skills.snapshot();
        // Scan-attempt generation is an internal CAS fence; exact semantic
        // records and drafts below determine whether an owner review changed.
        digest(&(
            workspace,
            state.community_generation,
            state.mcp_registry.snapshot(),
            skills.skills,
            skills.drafts,
            state.agents[agent].config().tools.clone(),
            hashes,
        ))
    }
    fn check_members(
        state: &crate::state::DaemonState,
        record: &BundleRecord,
        hashes: &BTreeMap<String, Option<String>>,
    ) -> Result<(), BundleError> {
        if Self::owner(state, &record.agent_id)? != record.workspace {
            return Err(BundleError::Conflict);
        }
        for part in record.mcp() {
            let row = state
                .mcp_registry
                .connections
                .get(&part.connection_id)
                .filter(|row| row.agent_id == record.agent_id)
                .ok_or(BundleError::Conflict)?;
            if configuration_digest(row)? != part.configuration_digest {
                return Err(BundleError::Conflict);
            }
        }
        for part in record.skills() {
            let approved = state
                .skills
                .get(&part.slug)
                .map(|row| row.approved_hash.clone());
            if approved.as_ref().is_some_and(|hash| {
                hash != &part.canonical_hash && Some(hash) != part.base_hash.as_ref()
            }) || hashes.get(&part.slug).cloned().flatten() != approved
            {
                return Err(BundleError::Conflict);
            }
        }
        Ok(())
    }
    pub(crate) async fn review(
        &self,
        agent: &str,
        input: ReviewRequest,
    ) -> Result<BundleReview, BundleError> {
        Self::owner(&*self.state.read().await, agent)?;
        if input.bundle_id.is_some() != input.expected_bundle_version.is_some()
            || input
                .expected_bundle_version
                .is_some_and(|v| v == 0 || v > MAX_VERSION)
        {
            return Err(BundleError::Invalid);
        }
        if input
            .bundle_id
            .as_ref()
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_err())
        {
            return Err(BundleError::Invalid);
        }
        let validated = validate_manifest(
            &parse_manifest(input.manifest_text.as_bytes()).map_err(|_| BundleError::Invalid)?,
            &runtime(),
        )
        .map_err(|error| {
            if error
                == anima_service_adapters::community_bundles::ManifestError::IncompatibleRuntime
            {
                BundleError::Conflict
            } else {
                BundleError::Invalid
            }
        })?;
        let old = match input.bundle_id.as_deref() {
            Some(id) => Some(self.record(agent, id).await?),
            None => None,
        };
        if let Some(old) = &old {
            if Some(old.version) != input.expected_bundle_version
                || old.manifest.id != validated.manifest.id
                || old.settlement.is_some()
                || !matches!(old.state.as_str(), "installed" | "disabled")
            {
                return Err(BundleError::Conflict);
            }
            // Skill topology cannot be retired invisibly by a metadata-only update.
            for part in old.skills() {
                if !validated
                    .manifest
                    .components
                    .iter()
                    .any(|c| matches!(c,ManifestComponent::Skill{key,..} if key==&part.key))
                {
                    return Err(BundleError::Conflict);
                }
            }
        }
        let hashes = self.hashes(old.as_ref()).await?;
        let _transaction = self.transactions.lock().await;
        let mut state = self.state.write().await;
        let workspace = Self::owner(&state, agent)?;
        if let Some(old) = &old {
            Self::check_members(&state, old, &hashes)?;
            if state.community_bundles.iter().find(|r| r.id == old.id) != Some(old) {
                return Err(BundleError::Conflict);
            }
        }
        state
            .community_reviews
            .retain(|_, receipt| receipt.view.expires_at_ms > now());
        if state.community_reviews.len() >= MAX_REVIEWS
            || (old.is_none() && state.community_bundles.len() >= MAX_BUNDLES)
        {
            return Err(BundleError::Conflict);
        }
        let bundle_id = old
            .as_ref()
            .map(|r| r.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let version = old
            .as_ref()
            .map(|r| next_version(r.version))
            .transpose()?
            .unwrap_or(1);
        let mut components = Vec::new();
        let mut preview = Vec::new();
        let mut mcp = Vec::new();
        let mut drafts = Vec::new();
        let mut predecessors = old
            .as_ref()
            .map(|r| r.predecessors.clone())
            .unwrap_or_default();
        let mut used_bindings = HashSet::new();
        for part in &validated.manifest.components {
            match part {
                ManifestComponent::Skill { key, skill_file } => {
                    let file = parse_skill_file(skill_file.as_bytes())
                        .map_err(|_| BundleError::Invalid)?;
                    let canonical = compose_skill_file(&file.name, &file.description, &file.body);
                    let canonical_hash = skill_hash(canonical.as_bytes());
                    let previous = old
                        .as_ref()
                        .and_then(|r| r.skills().find(|s| s.key == *key));
                    let slug = previous.map(|s| s.slug.clone()).unwrap_or_else(|| {
                        format!(
                            "cb-{}-{}",
                            uuid::Uuid::parse_str(&bundle_id).unwrap().simple(),
                            &skill_hash(key.as_bytes())[..24]
                        )
                    });
                    if !crate::skills::is_valid_slug(&slug) {
                        return Err(BundleError::Invalid);
                    }
                    if previous.is_none()
                        && (state.skills.get(&slug).is_some()
                            || state.skills.pending_drafts().iter().any(|d| d.slug == slug)
                            || state
                                .community_bundles
                                .iter()
                                .any(|r| r.skills().any(|s| s.slug == slug)))
                    {
                        return Err(BundleError::Conflict);
                    }
                    let base_hash = state.skills.get(&slug).map(|r| r.approved_hash.clone());
                    let keep = previous.is_some_and(|p| {
                        p.canonical_hash == canonical_hash
                            && (base_hash.as_deref() == Some(canonical_hash.as_str())
                                || state
                                    .skills
                                    .draft(&p.draft_id)
                                    .is_some_and(SkillDraft::is_pending))
                    });
                    let mut draft = if keep {
                        state.skills.draft(&previous.unwrap().draft_id).cloned()
                    } else {
                        Some(SkillDraft::new(
                            &slug,
                            file,
                            DraftSource::Import,
                            None,
                            base_hash.clone(),
                            now(),
                        ))
                    };
                    if let Some(draft) = &mut draft {
                        if draft.is_pending() {
                            draft.bundle_source = Some(ManagedDraftSource {
                                bundle_id: bundle_id.clone(),
                                bundle_version: version,
                                component_key: key.clone(),
                                canonical_hash: canonical_hash.clone(),
                                expected_base_hash: draft.base_hash.clone(),
                                workspace_identity: digest(&workspace)?,
                            });
                            drafts.push(draft.clone());
                        }
                    }
                    let draft_id = draft
                        .as_ref()
                        .map(|d| d.id.clone())
                        .or_else(|| previous.map(|p| p.draft_id.clone()))
                        .ok_or(BundleError::Conflict)?;
                    let source_hash = skill_hash(skill_file.as_bytes());
                    preview.push(ComponentReview::Skill {
                        key: key.clone(),
                        action: if keep {
                            "keep"
                        } else if previous.is_some() {
                            "replace"
                        } else {
                            "create"
                        }
                        .into(),
                        slug: slug.clone(),
                        draft_id: (!keep).then_some(draft_id.clone()),
                        source_hash: source_hash.clone(),
                        canonical_hash: canonical_hash.clone(),
                        canonical_skill_file: canonical.clone(),
                        base_hash: base_hash.clone(),
                        actual_hash: hashes.get(&slug).cloned().flatten(),
                        workspace_global: true,
                    });
                    components.push(OwnedComponent::Skill {
                        member: OwnedSkill {
                            key: key.clone(),
                            slug,
                            source_hash,
                            canonical_hash,
                            canonical_skill_file: canonical,
                            draft_id,
                            base_hash,
                            resume_enabled: previous.is_none_or(|p| p.resume_enabled),
                        },
                    });
                }
                _ => {
                    let (key, label, endpoint, stdio, kind) = match part {
                        ManifestComponent::McpHttp {
                            key,
                            label,
                            endpoint,
                        } => (
                            key,
                            label,
                            crate::mcp::normalized_endpoint(endpoint)
                                .map_err(|_| BundleError::Invalid)?,
                            None,
                            "mcpHttp",
                        ),
                        ManifestComponent::McpStdio {
                            key,
                            label,
                            program_ref,
                            args,
                        } => {
                            if !cfg!(windows) {
                                return Err(BundleError::Conflict);
                            }
                            used_bindings.insert(program_ref.clone());
                            let spec = McpStdioSpec {
                                executable: input
                                    .program_bindings
                                    .get(program_ref)
                                    .cloned()
                                    .ok_or(BundleError::Invalid)?,
                                args: args.clone(),
                            };
                            spec.validate().map_err(|_| BundleError::Invalid)?;
                            (key, label, String::new(), Some(spec), "mcpStdio")
                        }
                        _ => unreachable!(),
                    };
                    let label =
                        crate::mcp::normalized_label(label).map_err(|_| BundleError::Invalid)?;
                    let previous = old.as_ref().and_then(|r| {
                        r.components.iter().find_map(|p| match p {
                            OwnedComponent::Mcp { member } if member.key == *key => Some(member),
                            _ => None,
                        })
                    });
                    let mut row = McpConnectionRecord {
                        id: uuid::Uuid::new_v4().to_string(),
                        agent_id: agent.into(),
                        label,
                        endpoint,
                        stdio,
                        bearer_credential_ref: None,
                        oauth_credential_ref: None,
                        enabled: false,
                        version: 1,
                        reviewed_tools: Vec::new(),
                    };
                    let config = configuration_digest(&row)?;
                    let keep = previous.is_some_and(|p| p.configuration_digest == config);
                    if keep {
                        row = state.mcp_registry.connections[&previous.unwrap().connection_id]
                            .clone();
                    } else if let Some(previous) = previous {
                        predecessors.push(previous.clone());
                    }
                    let args = (
                        key.clone(),
                        if keep {
                            "keep"
                        } else if previous.is_some() {
                            "replace"
                        } else {
                            "create"
                        }
                        .to_string(),
                        row.id.clone(),
                        previous.map(|p| p.connection_id.clone()),
                        previous.map(|p| state.mcp_registry.connections[&p.connection_id].version),
                        row.label.clone(),
                        (!row.endpoint.is_empty()).then_some(row.endpoint.clone()),
                        row.stdio.clone(),
                        config.clone(),
                    );
                    preview.push(if kind == "mcpHttp" {
                        ComponentReview::Http {
                            key: args.0,
                            action: args.1,
                            connection_id: args.2,
                            previous_connection_id: args.3,
                            expected_version: args.4,
                            label: args.5,
                            endpoint: args.6,
                            stdio: args.7,
                            configuration_digest: args.8,
                        }
                    } else {
                        ComponentReview::Stdio {
                            key: args.0,
                            action: args.1,
                            connection_id: args.2,
                            previous_connection_id: args.3,
                            expected_version: args.4,
                            label: args.5,
                            endpoint: args.6,
                            stdio: args.7,
                            configuration_digest: args.8,
                        }
                    });
                    components.push(OwnedComponent::Mcp {
                        member: OwnedMcp {
                            key: key.clone(),
                            kind: kind.into(),
                            connection_id: row.id.clone(),
                            configuration_digest: config,
                            resume_enabled: keep
                                && if old.as_ref().is_some_and(|bundle| !bundle.enabled) {
                                    previous.is_some_and(|member| member.resume_enabled)
                                } else {
                                    row.enabled
                                },
                        },
                    });
                    if !keep {
                        mcp.push(row);
                    }
                }
            }
        }
        if used_bindings != input.program_bindings.keys().cloned().collect() {
            return Err(BundleError::Invalid);
        }
        if let Some(old) = &old {
            for part in old.components.iter().filter_map(|p| match p {
                OwnedComponent::Mcp { member } => Some(member),
                _ => None,
            }) {
                if !components.iter().any(|p|matches!(p,OwnedComponent::Mcp{member}if member.connection_id==part.connection_id))&&!predecessors.iter().any(|p|p.connection_id==part.connection_id){predecessors.push(part.clone())}
            }
        }
        let record = BundleRecord {
            id: bundle_id.clone(),
            agent_id: agent.into(),
            manifest: validated.manifest.clone(),
            manifest_digest: validated.manifest_digest.clone(),
            review_digest: String::new(),
            version,
            enabled: old.as_ref().is_none_or(|r| r.enabled),
            state: if old.as_ref().is_some_and(|r| !r.enabled) {
                "disabled"
            } else {
                "installed"
            }
            .into(),
            workspace,
            components,
            predecessors,
            settlement: None,
        };
        let observation = Self::observation(&state, agent, &hashes)?;
        let compatibility = runtime();
        let mut view = BundleReview {
            id: uuid::Uuid::new_v4().to_string(),
            review_digest: String::new(),
            manifest_digest: validated.manifest_digest,
            mode: if old.is_some() { "update" } else { "install" }.into(),
            bundle_id,
            expected_bundle_version: input.expected_bundle_version,
            agent_id: agent.into(),
            expires_at_ms: now() + REVIEW_TTL_MS,
            manifest: validated.manifest.clone(),
            compatibility: CompatibilityReport {
                host: compatibility.host,
                version: compatibility.version,
                capabilities: compatibility.capabilities,
                supported: true,
            },
            permissions: validated.manifest.permissions,
            components: preview,
            reviewed_component_state_digest: observation.clone(),
        };
        view.review_digest = digest(&(&view, &record, &observation))?;
        let mut record = record;
        record.review_digest = view.review_digest.clone();
        state.community_reviews.insert(
            view.id.clone(),
            ReviewReceipt {
                view: view.clone(),
                record,
                mcp,
                drafts,
                observation,
            },
        );
        Ok(view)
    }

    async fn commit(
        &self,
        mut bundles: Vec<BundleRecord>,
        mut mcp: McpRegistry,
        mut skills: SkillRegistry,
        agent: &str,
        removed: Vec<String>,
        affected: &str,
        retire: Vec<String>,
    ) -> Result<(), BundleError> {
        let mut state = self.state.write().await;
        Self::owner(&state, agent)?;
        if state.control_plane_store.is_none() {
            return Err(BundleError::Unavailable);
        }
        let generation = state
            .community_generation
            .checked_add(1)
            .ok_or(BundleError::Conflict)?;
        let request = state
            .control_plane_persist_request()
            .with_bundle_candidate(
                bundles.clone(),
                generation,
                mcp.snapshot(),
                skills.snapshot(),
                (!removed.is_empty()).then_some((agent.to_string(), removed.clone())),
            )
            .ok_or(BundleError::Unavailable)?;
        drop(state);
        let uncertain = match request.save_mcp_candidate().await {
            Ok(()) => false,
            Err(error) if crate::control_plane_store::is_durability_uncertain(&error) => true,
            Err(_) => return Err(BundleError::Unavailable),
        };
        let mut state = self.state.write().await;
        Self::owner(&state, agent)?;
        if uncertain {
            if let Some(bundle) = bundles.iter_mut().find(|r| r.id == affected) {
                bundle.state = "unavailable".into();
                if let Some(settlement) = &mut bundle.settlement {
                    settlement.phase = "reconciliation_required".into();
                } else {
                    bundle.settlement = Some(Settlement {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: "update".into(),
                        phase: "reconciliation_required".into(),
                        trash: BTreeMap::new(),
                    });
                }
            } else {
                // An uncertain final delete retains its already durable cleanup
                // intention in memory. Removed authority stays removed; the next
                // snapshot preserves this observed intent rather than replaying.
                if let Some(mut retained) = state
                    .community_bundles
                    .iter()
                    .find(|r| r.id == affected)
                    .cloned()
                {
                    retained.state = "removing".into();
                    if let Some(settlement) = &mut retained.settlement {
                        settlement.phase = "reconciliation_required".into();
                    }
                    bundles.push(retained);
                    mcp = state.mcp_registry.clone();
                    skills = state.skills.clone();
                }
            }
        }
        state.community_bundles = bundles;
        state.community_generation = generation;
        state.mcp_registry = mcp;
        state.skills = skills;
        if !removed.is_empty() {
            let row = state.agents.get_mut(agent).ok_or(BundleError::NotFound)?;
            let mut config = row.config().clone();
            if let Some(tools) = &mut config.tools {
                tools.retain(|tool| !removed.contains(&tool.name));
            }
            row.replace_config(config);
        }
        if !removed.is_empty() {
            self.mcp.note_agent_permissions_changed(agent);
        }
        self.mcp
            .publish_bundle_catalog(&state, &retire, agent)
            .map_err(|_| BundleError::Unavailable)?;
        if uncertain {
            let ids = state
                .community_bundles
                .iter()
                .find(|r| r.id == affected)
                .map(|r| r.mcp().map(|p| p.connection_id.clone()).collect::<Vec<_>>())
                .unwrap_or_default();
            self.mcp
                .retire_bundle_connections(&ids)
                .map_err(|_| BundleError::Unavailable)?;
            return Err(BundleError::Unavailable);
        }
        Ok(())
    }

    pub(crate) async fn install(
        &self,
        agent: &str,
        input: InstallRequest,
    ) -> Result<BundleView, BundleError> {
        let service = self.clone();
        let agent = agent.to_string();
        self.owned(async move { service.install_owned(&agent, None, input).await })
            .await
    }
    pub(crate) async fn update(
        &self,
        agent: &str,
        id: &str,
        input: UpdateRequest,
    ) -> Result<BundleView, BundleError> {
        if !valid_version(input.expected_version) {
            return Err(BundleError::Invalid);
        }
        let service = self.clone();
        let agent = agent.to_string();
        let id = id.to_string();
        self.owned(async move {
            service
                .install_owned(
                    &agent,
                    Some((id, input.expected_version)),
                    InstallRequest {
                        review_id: input.review_id,
                        review_digest: input.review_digest,
                    },
                )
                .await
        })
        .await
    }
    async fn install_owned(
        &self,
        agent: &str,
        target: Option<(String, u64)>,
        input: InstallRequest,
    ) -> Result<BundleView, BundleError> {
        Self::owner(&*self.state.read().await, agent)?;
        if uuid::Uuid::parse_str(&input.review_id).is_err() || !valid_hash(&input.review_digest) {
            return Err(BundleError::Invalid);
        }
        let receipt = self
            .state
            .read()
            .await
            .community_reviews
            .get(&input.review_id)
            .cloned()
            .ok_or(BundleError::Conflict)?;
        if receipt.view.agent_id != agent
            || receipt.view.review_digest != input.review_digest
            || receipt.view.expires_at_ms <= now()
            || target.as_ref().map(|(id, v)| (id.as_str(), *v))
                != receipt
                    .view
                    .expected_bundle_version
                    .map(|v| (receipt.view.bundle_id.as_str(), v))
        {
            return Err(BundleError::Conflict);
        }
        let old = match &target {
            Some((id, _)) => Some(self.record(agent, id).await?),
            None => None,
        };
        let hashes = self.hashes(old.as_ref()).await?;
        let _transaction = self.transactions.lock().await;
        let state = self.state.read().await;
        if receipt.view.expires_at_ms <= now()
            || Self::observation(&state, agent, &hashes)? != receipt.observation
            || !state.community_reviews.contains_key(&input.review_id)
        {
            return Err(BundleError::Conflict);
        }
        if let Some(old) = &old {
            Self::check_members(&state, old, &hashes)?;
        }
        if old.is_none() && state.community_bundles.len() >= MAX_BUNDLES {
            return Err(BundleError::Conflict);
        }
        let mut bundles = state.community_bundles.clone();
        let mut mcp = state.mcp_registry.clone();
        let mut skills = state.skills.clone();
        if let Some(old) = &old {
            invalidate_pending(&mut skills, old, now());
        }
        let mut retire = Vec::new();
        for part in &receipt.record.predecessors {
            let row = mcp
                .connections
                .get_mut(&part.connection_id)
                .ok_or(BundleError::Conflict)?;
            if row.enabled {
                row.enabled = false;
                row.version = next_version(row.version)?;
                retire.push(row.id.clone());
            }
        }
        for row in &receipt.mcp {
            if mcp
                .connections
                .insert(row.id.clone(), row.clone())
                .is_some()
            {
                return Err(BundleError::Conflict);
            }
        }
        for draft in &receipt.drafts {
            skills.put_draft(draft.clone());
        }
        if mcp.connections.len() > crate::mcp::MAX_CONNECTIONS
            || skills.pending_imports() > crate::skills::MAX_PENDING_IMPORT_DRAFTS
            || skills.records().len() > crate::skills::MAX_SKILLS
        {
            return Err(BundleError::Conflict);
        }
        McpRegistry::validate(&mcp.snapshot()).map_err(|_| BundleError::Conflict)?;
        let skill_snapshot = skills.snapshot();
        SkillRegistry::validate(&skill_snapshot.skills, &skill_snapshot.drafts)
            .map_err(|_| BundleError::Conflict)?;
        bundles.retain(|r| r.id != receipt.record.id);
        bundles.push(receipt.record.clone());
        drop(state);
        self.commit(
            bundles,
            mcp,
            skills,
            agent,
            Vec::new(),
            &receipt.record.id,
            retire,
        )
        .await?;
        self.state
            .write()
            .await
            .community_reviews
            .remove(&input.review_id);
        drop(_transaction);
        self.get(agent, &receipt.record.id).await
    }

    fn summary(record: &BundleRecord, state_digest: String) -> BundleSummary {
        BundleSummary {
            id: record.id.clone(),
            agent_id: record.agent_id.clone(),
            manifest_id: record.manifest.id.clone(),
            manifest_version: record.manifest.version.clone(),
            name: record.manifest.name.clone(),
            description: record.manifest.description.clone(),
            manifest_digest: record.manifest_digest.clone(),
            review_digest: record.review_digest.clone(),
            version: record.version,
            enabled: record.enabled,
            state: record.state.clone(),
            reviewed_component_state_digest: state_digest,
            mcp_count: record.mcp().count(),
            skill_count: record.skills().count(),
            settlement: record.settlement.as_ref().map(|settlement| SettlementView {
                id: settlement.id.clone(),
                kind: settlement.kind.clone(),
                phase: settlement.phase.clone(),
            }),
        }
    }
    pub(crate) async fn list(&self, agent: &str) -> Result<Vec<BundleSummary>, BundleError> {
        let records = {
            let state = self.state.read().await;
            Self::owner(&state, agent)?;
            state
                .community_bundles
                .iter()
                .filter(|r| r.agent_id == agent)
                .cloned()
                .collect::<Vec<_>>()
        };
        let mut result = Vec::new();
        for record in records {
            result.push(self.get(agent, &record.id).await?.summary);
        }
        Ok(result)
    }
    pub(crate) async fn get(&self, agent: &str, id: &str) -> Result<BundleView, BundleError> {
        let record = self.record(agent, id).await?;
        let hashes = self.hashes(Some(&record)).await?;
        let state = self.state.read().await;
        if state.community_bundles.iter().find(|row| row.id == id) != Some(&record) {
            return Err(BundleError::Conflict);
        }
        let state_digest = Self::observation(&state, agent, &hashes)?;
        let skills = state.skills.clone();
        let tools = state.agents[agent]
            .config()
            .tools
            .clone()
            .unwrap_or_default();
        drop(state);
        let mut components = Vec::new();
        let mut predecessors = Vec::new();
        for part in &record.components {
            match part {
                OwnedComponent::Mcp { member } => components.push(BundleComponentView::Mcp(
                    self.mcp_view(agent, member, &tools).await?,
                )),
                OwnedComponent::Skill { member } => {
                    let approved = skills.get(&member.slug);
                    let draft = skills.draft(&member.draft_id);
                    let actual = hashes.get(&member.slug).cloned().flatten();
                    let status = if record.settlement.is_some()
                        || !matches!(record.state.as_str(), "installed" | "disabled")
                    {
                        "unavailable"
                    } else if let Some(approved) = approved {
                        if actual.as_deref() != Some(approved.approved_hash.as_str()) {
                            if actual.is_none() {
                                "missing"
                            } else {
                                "changed"
                            }
                        } else if !record.enabled || !approved.enabled {
                            "disabled"
                        } else {
                            "active"
                        }
                    } else {
                        "pending"
                    };
                    components.push(BundleComponentView::Skill(SkillComponentView {
                        key: member.key.clone(),
                        kind: "skill".into(),
                        slug: member.slug.clone(),
                        source_hash: member.source_hash.clone(),
                        canonical_hash: member.canonical_hash.clone(),
                        approved_hash: approved.map(|r| r.approved_hash.clone()),
                        actual_hash: actual,
                        enabled: approved.is_some_and(|r| r.enabled) && record.available(),
                        status: status.into(),
                        draft_id: draft.map(|d| d.id.clone()),
                        draft_status: draft.map(|d| d.status.as_str().into()),
                        base_hash: member.base_hash.clone(),
                        workspace_global: true,
                    }));
                }
            }
        }
        for part in &record.predecessors {
            predecessors.push(self.mcp_view(agent, part, &tools).await?);
        }
        Ok(BundleView {
            summary: Self::summary(&record, state_digest),
            manifest: record.manifest,
            components,
            predecessors,
            tool_names: tools.into_iter().map(|t| t.name).collect(),
        })
    }
    async fn mcp_view(
        &self,
        agent: &str,
        member: &OwnedMcp,
        tools: &[anima_core::ToolDescriptor],
    ) -> Result<McpComponentView, BundleError> {
        let view = self
            .mcp
            .get(agent, &member.connection_id)
            .await
            .map_err(|_| BundleError::Conflict)?;
        let aliases = view
            .record
            .reviewed_tools
            .iter()
            .map(|t| t.alias.clone())
            .collect::<Vec<_>>();
        Ok(McpComponentView {
            key: member.key.clone(),
            kind: member.kind.clone(),
            connection_id: member.connection_id.clone(),
            connection_version: view.record.version,
            configuration_digest: configuration_digest(&view.record)?,
            enabled: view.record.enabled,
            runtime_state: view.state.as_str().into(),
            reviewed_alias_names: aliases.clone(),
            granted_alias_names: aliases
                .into_iter()
                .filter(|a| tools.iter().any(|t| &t.name == a))
                .collect(),
            physical_calls_active: view.physical_calls_active,
        })
    }

    pub(crate) async fn set_enabled(
        &self,
        agent: &str,
        id: &str,
        input: ChangeRequest,
    ) -> Result<BundleView, BundleError> {
        if !valid_version(input.expected_version)
            || !valid_hash(&input.reviewed_component_state_digest)
        {
            return Err(BundleError::Invalid);
        }
        let service = self.clone();
        let agent = agent.to_string();
        let id = id.to_string();
        self.owned(async move { service.change_owned(&agent, &id, input).await })
            .await
    }
    async fn change_owned(
        &self,
        agent: &str,
        id: &str,
        input: ChangeRequest,
    ) -> Result<BundleView, BundleError> {
        let record = self.record(agent, id).await?;
        let hashes = self.hashes(Some(&record)).await?;
        let _transaction = self.transactions.lock().await;
        let state = self.state.read().await;
        Self::check_members(&state, &record, &hashes)?;
        if state.community_bundles.iter().find(|row| row.id == id) != Some(&record) {
            return Err(BundleError::Conflict);
        }
        if record.version != input.expected_version
            || Self::observation(&state, agent, &hashes)? != input.reviewed_component_state_digest
            || record.settlement.is_some()
            || !matches!(record.state.as_str(), "installed" | "disabled")
        {
            return Err(BundleError::Conflict);
        }
        if record.enabled == input.enabled {
            drop(state);
            drop(_transaction);
            return self.get(agent, id).await;
        }
        let mut changed = record.clone();
        changed.version = next_version(record.version)?;
        changed.enabled = input.enabled;
        changed.state = if input.enabled {
            "installed"
        } else {
            "disabled"
        }
        .into();
        let mut mcp = state.mcp_registry.clone();
        let mut skills = state.skills.clone();
        let mut retire = if input.enabled {
            Vec::new()
        } else {
            record
                .mcp()
                .map(|part| part.connection_id.clone())
                .collect::<Vec<_>>()
        };
        for part in &mut changed.components {
            match part {
                OwnedComponent::Mcp { member } => {
                    let row = mcp
                        .connections
                        .get_mut(&member.connection_id)
                        .ok_or(BundleError::Conflict)?;
                    if !input.enabled {
                        member.resume_enabled = row.enabled;
                        retire.push(row.id.clone());
                    }
                    let enabled = input.enabled && member.resume_enabled;
                    if row.enabled != enabled {
                        row.enabled = enabled;
                        row.version = next_version(row.version)?;
                    }
                }
                OwnedComponent::Skill { member } => {
                    if let Some(previous) = skills.get(&member.slug).cloned() {
                        let mut row = previous;
                        if !input.enabled {
                            member.resume_enabled = row.enabled;
                        }
                        row.enabled = input.enabled && member.resume_enabled;
                        row.updated_at_ms = now();
                        skills.put(row).map_err(|_| BundleError::Conflict)?;
                    }
                }
            }
        }
        retire.sort();
        retire.dedup();
        if input.enabled {
            for member in changed.skills() {
                if let Some(mut draft) = skills
                    .draft(&member.draft_id)
                    .filter(|draft| draft.is_pending())
                    .cloned()
                {
                    if let Some(source) = &mut draft.bundle_source {
                        source.bundle_version = changed.version;
                    }
                    skills.put_draft(draft);
                }
            }
        }
        if !input.enabled {
            invalidate_pending(&mut skills, &record, now());
            changed.settlement = Some(Settlement {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "disable".into(),
                phase: "cleanup_pending".into(),
                trash: BTreeMap::new(),
            });
            changed.state = "unavailable".into();
        }
        let mut bundles = state.community_bundles.clone();
        *bundles
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or(BundleError::NotFound)? = changed.clone();
        drop(state);
        self.commit(bundles, mcp, skills, agent, Vec::new(), id, retire.clone())
            .await?;
        drop(_transaction);
        if !input.enabled {
            self.mcp
                .join_bundle_connections(&retire)
                .await
                .map_err(|_| BundleError::Unavailable)?;
            self.finish_intent(agent, id, &changed).await?;
        }
        self.get(agent, id).await
    }
    async fn finish_intent(
        &self,
        agent: &str,
        id: &str,
        expected: &BundleRecord,
    ) -> Result<(), BundleError> {
        let _transaction = self.transactions.lock().await;
        let state = self.state.read().await;
        let current = state
            .community_bundles
            .iter()
            .find(|r| r.id == id)
            .ok_or(BundleError::NotFound)?;
        if current != expected {
            return Err(BundleError::Conflict);
        }
        let mut bundles = state.community_bundles.clone();
        let changed = bundles.iter_mut().find(|r| r.id == id).unwrap();
        changed.settlement = None;
        changed.state = if changed.enabled {
            "installed"
        } else {
            "disabled"
        }
        .into();
        let mcp = state.mcp_registry.clone();
        let skills = state.skills.clone();
        drop(state);
        self.commit(bundles, mcp, skills, agent, Vec::new(), id, Vec::new())
            .await
    }

    pub(crate) async fn remove(
        &self,
        agent: &str,
        id: &str,
        input: RemoveRequest,
    ) -> Result<RemoveOutcome, BundleError> {
        if !valid_version(input.expected_version)
            || !valid_hash(&input.reviewed_component_state_digest)
        {
            return Err(BundleError::Invalid);
        }
        let service = self.clone();
        let agent = agent.to_string();
        let id = id.to_string();
        self.owned(async move { service.remove_owned(&agent, &id, input).await })
            .await
    }
    async fn remove_owned(
        &self,
        agent: &str,
        id: &str,
        input: RemoveRequest,
    ) -> Result<RemoveOutcome, BundleError> {
        let record = self.record(agent, id).await?;
        let hashes = self.hashes(Some(&record)).await?;
        let _transaction = self.transactions.lock().await;
        let state = self.state.read().await;
        Self::check_members(&state, &record, &hashes)?;
        if state.community_bundles.iter().find(|row| row.id == id) != Some(&record) {
            return Err(BundleError::Conflict);
        }
        if record.version != input.expected_version
            || Self::observation(&state, agent, &hashes)? != input.reviewed_component_state_digest
            || record.settlement.is_some()
            || !matches!(record.state.as_str(), "installed" | "disabled")
        {
            return Err(BundleError::Conflict);
        }
        let mut removing = record.clone();
        removing.version = next_version(record.version)?;
        removing.enabled = false;
        removing.state = "removing".into();
        removing.settlement = Some(Settlement {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "remove".into(),
            phase: "cleanup_pending".into(),
            trash: BTreeMap::new(),
        });
        // Reserve exact bounded trash destinations before the durable intent.
        // Restart exposes them for reconciliation and never repeats the move.
        for part in record.skills() {
            removing.settlement.as_mut().unwrap().trash.insert(
                part.slug.clone(),
                format!("{}-{}-{}", part.slug, now(), uuid::Uuid::new_v4().as_u128()),
            );
        }
        let mut mcp = state.mcp_registry.clone();
        let mut skills = state.skills.clone();
        let ids = record
            .mcp()
            .map(|p| p.connection_id.clone())
            .collect::<Vec<_>>();
        for id in &ids {
            let row = mcp.connections.get_mut(id).ok_or(BundleError::Conflict)?;
            if row.enabled {
                row.enabled = false;
                row.version = next_version(row.version)?;
            }
        }
        for part in record.skills() {
            if let Some(mut row) = skills.get(&part.slug).cloned() {
                row.enabled = false;
                skills.put(row).map_err(|_| BundleError::Conflict)?;
            }
        }
        invalidate_pending(&mut skills, &record, now());
        let mut bundles = state.community_bundles.clone();
        *bundles.iter_mut().find(|r| r.id == id).unwrap() = removing.clone();
        drop(state);
        self.commit(bundles, mcp, skills, agent, Vec::new(), id, ids.clone())
            .await?;
        drop(_transaction);
        self.mcp
            .join_bundle_connections(&ids)
            .await
            .map_err(|_| BundleError::Unavailable)?;
        let mut trash = Vec::new();
        // Durable intent fences normal daemon edits. External editors remain
        // outside this lock: preserve observed trash on mismatch, never untrash
        // over a new live folder or claim atomic filesystem CAS.
        for part in record.skills() {
            let root = record.workspace.root_path.clone();
            let slug = part.slug.clone();
            let expected = hashes.get(&slug).cloned().flatten();
            let destination = removing.settlement.as_ref().unwrap().trash[&slug].clone();
            let moved = crate::skills::service::blocking(move || {
                let before = crate::skills::disk::read_skill_bytes(&root, &slug)?
                    .as_deref()
                    .map(skill_hash);
                if before != expected {
                    return Err("Community skill changed".to_string());
                }
                let moved = crate::skills::disk::trash_skill_folder_to(&root, &slug, &destination)?;
                if moved.is_some()
                    && crate::skills::disk::read_trashed_skill_bytes(&root, &slug, &destination)?
                        .as_deref()
                        .map(skill_hash)
                        != expected
                {
                    return Err("Community skill changed during trash".into());
                }
                if crate::skills::disk::read_skill_bytes(&root, &slug)?.is_some() {
                    return Err("A new live community skill appeared".into());
                }
                Ok(moved)
            })
            .await
            .map_err(|_| BundleError::Unavailable)?
            .map_err(|_| BundleError::Conflict)?;
            if let Some(name) = moved {
                trash.push(crate::skills::disk::trash_relative_path(&name));
            }
        }
        let _transaction = self.transactions.lock().await;
        let state = self.state.read().await;
        if state.community_bundles.iter().find(|r| r.id == id) != Some(&removing) {
            return Err(BundleError::Conflict);
        }
        let removing_rows = ids
            .iter()
            .map(|id| {
                state
                    .mcp_registry
                    .connections
                    .get(id)
                    .cloned()
                    .ok_or(BundleError::Conflict)
            })
            .collect::<Result<Vec<_>, _>>()?;
        drop(state);
        self.mcp
            .delete_retired_bundle_credentials(agent, id, &removing_rows)
            .await
            .map_err(|_| BundleError::Unavailable)?;
        let state = self.state.read().await;
        if state.community_bundles.iter().find(|r| r.id == id) != Some(&removing)
            || removing_rows
                .iter()
                .any(|row| state.mcp_registry.connections.get(&row.id) != Some(row))
        {
            return Err(BundleError::Conflict);
        }
        let mut bundles = state.community_bundles.clone();
        bundles.retain(|r| r.id != id);
        let mut mcp = state.mcp_registry.clone();
        let mut skills = state.skills.clone();
        let mut aliases = Vec::new();
        for id in ids {
            let row = mcp.connections.remove(&id).ok_or(BundleError::Conflict)?;
            aliases.extend(row.reviewed_tools.into_iter().map(|t| t.alias));
        }
        for part in record.skills() {
            skills.remove(&part.slug);
            skills.set_scanned(&part.slug, None);
            let drafts = skills
                .snapshot()
                .drafts
                .into_iter()
                .filter(|d| {
                    d.bundle_source
                        .as_ref()
                        .is_some_and(|s| s.bundle_id == record.id)
                })
                .map(|d| d.id)
                .collect::<Vec<_>>();
            for id in drafts {
                skills.remove_draft(&id);
            }
        }
        drop(state);
        self.commit(bundles, mcp, skills, agent, aliases, id, Vec::new())
            .await?;
        let state = self.state.read().await;
        let tool_names = state
            .agents
            .get(agent)
            .ok_or(BundleError::NotFound)?
            .config()
            .tools
            .iter()
            .flatten()
            .map(|t| t.name.clone())
            .collect();
        Ok(RemoveOutcome {
            bundle_id: id.into(),
            removed: true,
            trash_paths: trash,
            tool_names,
        })
    }
}
