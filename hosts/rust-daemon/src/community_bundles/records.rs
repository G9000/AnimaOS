use std::collections::BTreeMap;

use anima_core::community_bundles::{CommunityManifest, ManifestPermissions};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::control_plane_store::WorkspaceConfig;
use crate::mcp::McpStdioSpec;

pub(crate) const MAX_BUNDLES: usize = 16;
pub(crate) const MAX_REVIEWS: usize = 10;
pub(crate) const REVIEW_TTL_MS: u64 = 600_000;
pub(crate) const MAX_VERSION: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ManagedDraftSource {
    pub(crate) bundle_id: String,
    pub(crate) bundle_version: u64,
    pub(crate) component_key: String,
    pub(crate) canonical_hash: String,
    pub(crate) expected_base_hash: Option<String>,
    pub(crate) workspace_identity: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OwnedMcp {
    pub(crate) key: String,
    pub(crate) kind: String,
    pub(crate) connection_id: String,
    pub(crate) configuration_digest: String,
    pub(crate) resume_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OwnedSkill {
    pub(crate) key: String,
    pub(crate) slug: String,
    pub(crate) source_hash: String,
    pub(crate) canonical_hash: String,
    pub(crate) canonical_skill_file: String,
    pub(crate) draft_id: String,
    pub(crate) base_hash: Option<String>,
    pub(crate) resume_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum OwnedComponent {
    Mcp { member: OwnedMcp },
    Skill { member: OwnedSkill },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settlement {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) phase: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) trash: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettlementView {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) phase: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BundleRecord {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) manifest: CommunityManifest,
    pub(crate) manifest_digest: String,
    pub(crate) review_digest: String,
    pub(crate) version: u64,
    pub(crate) enabled: bool,
    pub(crate) state: String,
    pub(crate) workspace: WorkspaceConfig,
    pub(crate) components: Vec<OwnedComponent>,
    pub(crate) predecessors: Vec<OwnedMcp>,
    pub(crate) settlement: Option<Settlement>,
}

impl BundleRecord {
    pub(crate) fn mcp(&self) -> impl Iterator<Item = &OwnedMcp> {
        self.components
            .iter()
            .filter_map(|part| match part {
                OwnedComponent::Mcp { member } => Some(member),
                _ => None,
            })
            .chain(self.predecessors.iter())
    }
    pub(crate) fn skills(&self) -> impl Iterator<Item = &OwnedSkill> {
        self.components.iter().filter_map(|part| match part {
            OwnedComponent::Skill { member } => Some(member),
            _ => None,
        })
    }
    pub(crate) fn available(&self) -> bool {
        self.enabled && self.state == "installed" && self.settlement.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewRequest {
    pub(crate) manifest_text: String,
    #[serde(deserialize_with = "bindings")]
    pub(crate) program_bindings: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "non_null")]
    pub(crate) bundle_id: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    pub(crate) expected_bundle_version: Option<u64>,
}

fn bindings<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct Visitor;
    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("unique bounded program bindings")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                if result.len() >= 2 || result.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("Invalid program bindings"));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Visitor)
}

fn non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InstallRequest {
    pub(crate) review_id: String,
    pub(crate) review_digest: String,
}
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UpdateRequest {
    pub(crate) expected_version: u64,
    pub(crate) review_id: String,
    pub(crate) review_digest: String,
}
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeRequest {
    pub(crate) expected_version: u64,
    pub(crate) reviewed_component_state_digest: String,
    pub(crate) enabled: bool,
}
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RemoveRequest {
    pub(crate) expected_version: u64,
    pub(crate) reviewed_component_state_digest: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompatibilityReport {
    pub(crate) host: String,
    pub(crate) version: String,
    pub(crate) capabilities: Vec<String>,
    pub(crate) supported: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ComponentReview {
    #[serde(rename = "mcpHttp", rename_all = "camelCase")]
    Http {
        key: String,
        action: String,
        connection_id: String,
        previous_connection_id: Option<String>,
        expected_version: Option<u64>,
        label: String,
        endpoint: Option<String>,
        #[schema(value_type=Option<Object>,nullable)]
        stdio: Option<McpStdioSpec>,
        configuration_digest: String,
    },
    #[serde(rename = "mcpStdio", rename_all = "camelCase")]
    Stdio {
        key: String,
        action: String,
        connection_id: String,
        previous_connection_id: Option<String>,
        expected_version: Option<u64>,
        label: String,
        endpoint: Option<String>,
        #[schema(value_type=Option<Object>,nullable)]
        stdio: Option<McpStdioSpec>,
        configuration_digest: String,
    },
    #[serde(rename_all = "camelCase")]
    Skill {
        key: String,
        action: String,
        slug: String,
        draft_id: Option<String>,
        source_hash: String,
        canonical_hash: String,
        canonical_skill_file: String,
        base_hash: Option<String>,
        actual_hash: Option<String>,
        workspace_global: bool,
    },
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BundleReview {
    pub(crate) id: String,
    pub(crate) review_digest: String,
    pub(crate) manifest_digest: String,
    pub(crate) mode: String,
    pub(crate) bundle_id: String,
    pub(crate) expected_bundle_version: Option<u64>,
    pub(crate) agent_id: String,
    pub(crate) expires_at_ms: u64,
    #[schema(value_type=Object)]
    pub(crate) manifest: CommunityManifest,
    pub(crate) compatibility: CompatibilityReport,
    #[schema(value_type=Object)]
    pub(crate) permissions: ManifestPermissions,
    pub(crate) components: Vec<ComponentReview>,
    pub(crate) reviewed_component_state_digest: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BundleSummary {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) manifest_id: String,
    pub(crate) manifest_version: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) manifest_digest: String,
    pub(crate) review_digest: String,
    pub(crate) version: u64,
    pub(crate) enabled: bool,
    pub(crate) state: String,
    pub(crate) reviewed_component_state_digest: String,
    pub(crate) mcp_count: usize,
    pub(crate) skill_count: usize,
    pub(crate) settlement: Option<SettlementView>,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpComponentView {
    pub(crate) key: String,
    pub(crate) kind: String,
    pub(crate) connection_id: String,
    pub(crate) connection_version: u64,
    pub(crate) configuration_digest: String,
    pub(crate) enabled: bool,
    pub(crate) runtime_state: String,
    pub(crate) reviewed_alias_names: Vec<String>,
    pub(crate) granted_alias_names: Vec<String>,
    pub(crate) physical_calls_active: bool,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillComponentView {
    pub(crate) key: String,
    pub(crate) kind: String,
    pub(crate) slug: String,
    pub(crate) source_hash: String,
    pub(crate) canonical_hash: String,
    pub(crate) approved_hash: Option<String>,
    pub(crate) actual_hash: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) status: String,
    pub(crate) draft_id: Option<String>,
    pub(crate) draft_status: Option<String>,
    pub(crate) base_hash: Option<String>,
    pub(crate) workspace_global: bool,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum BundleComponentView {
    Mcp(McpComponentView),
    Skill(SkillComponentView),
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BundleView {
    #[serde(flatten)]
    pub(crate) summary: BundleSummary,
    #[schema(value_type=Object)]
    pub(crate) manifest: CommunityManifest,
    pub(crate) components: Vec<BundleComponentView>,
    pub(crate) predecessors: Vec<McpComponentView>,
    pub(crate) tool_names: Vec<String>,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct ReviewEnvelope {
    pub(crate) review: BundleReview,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct BundleEnvelope {
    pub(crate) bundle: BundleView,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct BundlesEnvelope {
    pub(crate) bundles: Vec<BundleSummary>,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoveOutcome {
    pub(crate) bundle_id: String,
    pub(crate) removed: bool,
    pub(crate) trash_paths: Vec<String>,
    pub(crate) tool_names: Vec<String>,
}
