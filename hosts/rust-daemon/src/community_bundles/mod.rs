//! Reviewed declarative content/configuration. No installer or execution engine.
mod records;
mod service;
#[cfg(test)]
mod tests;
pub(crate) use records::*;
pub(crate) use service::{BundleService, ReviewReceipt};

use crate::skills::{DraftStatus, SkillDraft, SkillError};
use crate::state::DaemonState;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BundleError {
    Invalid,
    NotFound,
    Conflict,
    Unavailable,
}
impl BundleError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Invalid => "Community bundle input is invalid",
            Self::NotFound => "Community bundle was not found",
            Self::Conflict => "Community bundle state changed; review its current state",
            Self::Unavailable => {
                "Community bundle settlement is unavailable; inspect its current state"
            }
        }
    }
}
pub(crate) fn digest(value: &impl Serialize) -> Result<String, BundleError> {
    serde_json::to_vec(value)
        .map(|bytes| crate::skills::skill_hash(&bytes))
        .map_err(|_| BundleError::Invalid)
}
pub(crate) fn next_version(version: u64) -> Result<u64, BundleError> {
    version
        .checked_add(1)
        .filter(|value| *value <= MAX_VERSION)
        .ok_or(BundleError::Conflict)
}
pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
pub(crate) fn valid_version(version: u64) -> bool {
    version > 0 && version <= MAX_VERSION
}
pub(crate) fn configuration_digest(
    row: &crate::mcp::McpConnectionRecord,
) -> Result<String, BundleError> {
    digest(&(&row.label, &row.endpoint, &row.stdio))
}
pub(crate) fn managed_skill<'a>(
    state: &'a DaemonState,
    slug: &str,
) -> Option<(&'a BundleRecord, &'a OwnedSkill)> {
    state.community_bundles.iter().find_map(|bundle| {
        bundle
            .skills()
            .find(|part| part.slug == slug)
            .map(|part| (bundle, part))
    })
}
pub(crate) fn check_skill_edit(
    state: &DaemonState,
    slug: &str,
    activation: bool,
) -> Result<bool, SkillError> {
    if let Some((bundle, _)) = managed_skill(state, slug) {
        if bundle.settlement.is_some()
            || !matches!(bundle.state.as_str(), "installed" | "disabled")
            || state.workspace.as_ref() != Some(&bundle.workspace)
            || (activation && !bundle.available())
        {
            return Err(SkillError::conflict(
                "Community bundle is unavailable or disabled",
            ));
        }
        return Ok(true);
    }
    Ok(false)
}
pub(crate) fn check_draft(
    state: &DaemonState,
    draft: &SkillDraft,
    canonical: Option<&str>,
) -> Result<(), SkillError> {
    let Some(source) = &draft.bundle_source else {
        return Ok(());
    };
    let (bundle, member) = managed_skill(state, &draft.slug)
        .ok_or_else(|| SkillError::conflict("Community bundle draft ownership changed"))?;
    let workspace = state.workspace.as_ref().ok_or(SkillError::NoWorkspace)?;
    if !bundle.available()
        || bundle.id != source.bundle_id
        || bundle.version != source.bundle_version
        || member.key != source.component_key
        || member.draft_id != draft.id
        || member.canonical_hash != source.canonical_hash
        || &bundle.workspace != workspace
        || digest(workspace)
            .map_err(|_| SkillError::conflict("Community bundle workspace changed"))?
            != source.workspace_identity
        || draft.base_hash != source.expected_base_hash
        || state
            .skills
            .get(&draft.slug)
            .map(|row| row.approved_hash.clone())
            != source.expected_base_hash
        || canonical.is_some_and(|hash| hash != source.canonical_hash)
    {
        return Err(SkillError::conflict(
            "Community bundle draft changed; review an exact bundle update",
        ));
    }
    Ok(())
}
pub(crate) fn check_mcp_edit(
    state: &DaemonState,
    id: &str,
    activation: bool,
    deleting: bool,
) -> Result<(), crate::mcp::McpServiceError> {
    if let Some(bundle) = state
        .community_bundles
        .iter()
        .find(|bundle| bundle.mcp().any(|part| part.connection_id == id))
    {
        let predecessor = bundle
            .predecessors
            .iter()
            .any(|part| part.connection_id == id);
        let member = bundle
            .mcp()
            .find(|part| part.connection_id == id)
            .ok_or(crate::mcp::McpServiceError::Conflict)?;
        let matches = state.mcp_registry.connections.get(id).is_some_and(|row| {
            configuration_digest(row).ok().as_ref() == Some(&member.configuration_digest)
        });
        if !matches
            || state.workspace.as_ref() != Some(&bundle.workspace)
            || bundle.settlement.is_some()
            || !matches!(bundle.state.as_str(), "installed" | "disabled")
            || (activation && (!bundle.available() || predecessor))
            || (deleting && !predecessor)
        {
            return Err(crate::mcp::McpServiceError::Conflict);
        }
    }
    Ok(())
}
pub(crate) fn runnable_skill(state: &DaemonState, slug: &str) -> bool {
    managed_skill(state, slug).is_none_or(|(bundle, _)| {
        bundle.available() && state.workspace.as_ref() == Some(&bundle.workspace)
    })
}

/// Pure restore checks, without workspace reads, launches or draft adoption.
pub(crate) fn validate_snapshot(
    snapshot: &crate::control_plane_store::ControlPlaneSnapshot,
) -> Result<(), String> {
    let invalid = || "Invalid community bundle snapshot".to_string();
    let hash = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    if snapshot.community_bundles.len() > MAX_BUNDLES {
        return Err(invalid());
    }
    let mut ids = HashSet::new();
    let mut connections = HashSet::new();
    let mut slugs = HashSet::new();
    for bundle in &snapshot.community_bundles {
        if uuid::Uuid::parse_str(&bundle.id).is_err()
            || !ids.insert(&bundle.id)
            || bundle.version == 0
            || bundle.version > MAX_VERSION
            || !matches!(
                bundle.state.as_str(),
                "installed" | "disabled" | "unavailable" | "removing"
            )
            || !snapshot.agents.iter().any(|row| {
                row.state.id == bundle.agent_id
                    && !crate::agent_runs::is_helper_config(&row.state.config)
            })
            || !hash(&bundle.manifest_digest)
            || !hash(&bundle.review_digest)
            || bundle.components.len() != bundle.manifest.components.len()
            || bundle.components.len() > 4
            || bundle.predecessors.len() > 4
        {
            return Err(invalid());
        }
        let saved_runtime = anima_service_adapters::community_bundles::RuntimeCompatibility {
            host: bundle.manifest.compatibility.host.clone(),
            version: bundle.manifest.compatibility.min_version.clone(),
            capabilities: bundle.manifest.compatibility.required_capabilities.clone(),
        };
        let valid = anima_service_adapters::community_bundles::validate_manifest(
            &bundle.manifest,
            &saved_runtime,
        )
        .map_err(|_| invalid())?;
        if valid.manifest_digest != bundle.manifest_digest {
            return Err(invalid());
        }
        let mut keys = HashSet::new();
        for member in &bundle.components {
            let (key, kind) = match member {
                OwnedComponent::Mcp { member } => (member.key.as_str(), member.kind.as_str()),
                OwnedComponent::Skill { member } => (member.key.as_str(), "skill"),
            };
            if !keys.insert(key)
                || !bundle.manifest.components.iter().any(|part| match part {
                    anima_service_adapters::community_bundles::ManifestComponent::Skill {
                        key: k,
                        ..
                    } => k == key && kind == "skill",
                    anima_service_adapters::community_bundles::ManifestComponent::McpHttp {
                        key: k,
                        ..
                    } => k == key && kind == "mcpHttp",
                    anima_service_adapters::community_bundles::ManifestComponent::McpStdio {
                        key: k,
                        ..
                    } => k == key && kind == "mcpStdio",
                })
            {
                return Err(invalid());
            }
        }
        if let Some(settlement) = &bundle.settlement {
            if uuid::Uuid::parse_str(&settlement.id).is_err()
                || !matches!(
                    settlement.kind.as_str(),
                    "install" | "update" | "enable" | "disable" | "remove"
                )
                || !matches!(
                    settlement.phase.as_str(),
                    "cleanup_pending" | "reconciliation_required"
                )
                || settlement.trash.len() > 2
            {
                return Err(invalid());
            }
            for (slug, name) in &settlement.trash {
                if !bundle.skills().any(|part| &part.slug == slug)
                    || !crate::skills::disk::valid_bundle_trash_name(slug, name)
                {
                    return Err(invalid());
                }
            }
        }
        for part in bundle.mcp() {
            if !hash(&part.configuration_digest)
                || !matches!(part.kind.as_str(), "mcpHttp" | "mcpStdio")
                || !connections.insert(&part.connection_id)
                || !snapshot
                    .mcp_connections
                    .iter()
                    .any(|row| row.id == part.connection_id && row.agent_id == bundle.agent_id)
            {
                return Err(invalid());
            }
        }
        for part in bundle.skills() {
            let source = bundle
                .manifest
                .components
                .iter()
                .find_map(|component| match component {
                    anima_service_adapters::community_bundles::ManifestComponent::Skill {
                        key,
                        skill_file,
                    } if key == &part.key => Some(skill_file),
                    _ => None,
                })
                .ok_or_else(invalid)?;
            let file = crate::skills::parse_skill_file(source.as_bytes()).map_err(|_| invalid())?;
            let expected_slug = format!(
                "cb-{}-{}",
                uuid::Uuid::parse_str(&bundle.id)
                    .map_err(|_| invalid())?
                    .simple(),
                &crate::skills::skill_hash(part.key.as_bytes())[..24]
            );
            if !slugs.insert(&part.slug)
                || part.slug != expected_slug
                || !crate::skills::is_valid_slug(&part.slug)
                || crate::skills::skill_hash(part.canonical_skill_file.as_bytes())
                    != part.canonical_hash
                || crate::skills::skill_hash(source.as_bytes()) != part.source_hash
                || crate::skills::compose_skill_file(&file.name, &file.description, &file.body)
                    != part.canonical_skill_file
            {
                return Err(invalid());
            }
        }
    }
    for draft in &snapshot.skill_drafts {
        if let Some(source) = &draft.bundle_source {
            let member = snapshot
                .community_bundles
                .iter()
                .find(|bundle| bundle.id == source.bundle_id)
                .and_then(|bundle| {
                    bundle
                        .skills()
                        .find(|part| part.slug == draft.slug)
                        .map(|part| (bundle, part))
                });
            if draft.source != crate::skills::DraftSource::Import
                || draft.learning_source.is_some()
                || draft.proposed_by.is_some()
                || !hash(&source.canonical_hash)
                || !hash(&source.workspace_identity)
                || source.bundle_version == 0
                || source.bundle_version > MAX_VERSION
                || source
                    .expected_base_hash
                    .as_ref()
                    .is_some_and(|value| !hash(value))
                || source.expected_base_hash != draft.base_hash
                || !member.is_some_and(|(bundle, _)| {
                    source.bundle_version <= bundle.version
                        && digest(&bundle.workspace).ok().as_ref()
                            == Some(&source.workspace_identity)
                })
            {
                return Err(invalid());
            }
            if draft.is_pending()
                && !member.is_some_and(|(bundle, part)| {
                    bundle.version == source.bundle_version
                        && part.draft_id == draft.id
                        && part.key == source.component_key
                        && part.canonical_hash == source.canonical_hash
                        && crate::skills::skill_hash(
                            crate::skills::compose_skill_file(
                                &draft.name,
                                &draft.description,
                                &draft.body,
                            )
                            .as_bytes(),
                        ) == part.canonical_hash
                })
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

pub(crate) fn restore_records(state: &mut DaemonState) {
    for bundle in &mut state.community_bundles {
        let compatible = anima_service_adapters::community_bundles::validate_manifest(
            &bundle.manifest,
            &service::runtime(),
        )
        .is_ok();
        let owned = bundle.mcp().all(|part| {
            state
                .mcp_registry
                .connections
                .get(&part.connection_id)
                .is_some_and(|row| {
                    configuration_digest(row).ok().as_ref() == Some(&part.configuration_digest)
                })
        });
        if !compatible
            || !owned
            || state.workspace.as_ref() != Some(&bundle.workspace)
            || bundle.settlement.is_some()
        {
            bundle.state = if bundle.state == "removing" {
                "removing"
            } else {
                "unavailable"
            }
            .into();
        }
    }
}

pub(crate) fn invalidate_pending(
    skills: &mut crate::skills::SkillRegistry,
    bundle: &BundleRecord,
    now: u64,
) {
    let pending = skills
        .pending_drafts()
        .into_iter()
        .filter(|draft| {
            draft
                .bundle_source
                .as_ref()
                .is_some_and(|source| source.bundle_id == bundle.id)
        })
        .cloned()
        .collect::<Vec<_>>();
    for mut draft in pending {
        draft.decide(DraftStatus::Rejected, now);
        skills.put_draft(draft);
    }
}
