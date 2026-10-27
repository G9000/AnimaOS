use anima_service_adapters::browser::{
    scope_digest, validate_scope, BrowserScope, MAX_BROWSER_SAFE_INTEGER,
};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashSet};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowserServiceError {
    Invalid,
    NotFound,
    Conflict,
    Busy,
    Unavailable,
}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}
pub(super) fn positive<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let value = u64::deserialize(deserializer)?;
    if value == 0 || value > MAX_BROWSER_SAFE_INTEGER {
        return Err(serde::de::Error::custom("invalid version"));
    }
    Ok(value)
}
fn optional_positive<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    positive(deserializer).map(Some)
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SaveBrowserScope {
    #[serde(default)]
    pub(crate) profile_id: Option<String>,
    pub(crate) policy_id: String,
    pub(crate) selected_input_ids: Vec<String>,
    pub(crate) enabled: bool,
    #[serde(default, deserialize_with = "optional_positive")]
    pub(crate) expected_version: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserSelection {
    #[serde(default)]
    pub(crate) profile_id: Option<String>,
    pub(crate) policy_id: String,
    pub(crate) selected_input_ids: Vec<String>,
}
pub(crate) fn optional_selection<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<BrowserSelection>, D::Error> {
    BrowserSelection::deserialize(d).map(Some)
}
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StopBrowserScope {
    #[serde(deserialize_with = "positive")]
    pub(crate) expected_version: u64,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateBrowserDestination {
    pub(crate) start_url: String,
    pub(crate) allowed_origins: Vec<String>,
    pub(crate) authenticated: bool,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserSetupView {
    pub(crate) available: bool,
    pub(crate) reason: Option<String>,
    pub(crate) setup_command: String,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct BrowserDestinationEnvelope {
    pub(crate) policy: BrowserPolicyView,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct BrowserDestinationRevoked {
    pub(crate) revoked: bool,
}

/// Operator-selected actual installation witness. No HTTP/model constructor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowserInstallation {
    pub(crate) id: String,
    pub(crate) digest: String,
}
#[derive(Clone)]
pub(crate) struct InstalledBrowserPolicy {
    pub(crate) id: String,
    pub(crate) template: BrowserScope,
}
#[derive(Clone, Default)]
pub(crate) struct InstalledBrowserPolicies {
    pub(super) policies: BTreeMap<String, InstalledBrowserPolicy>,
    pub(super) installation: Option<BrowserInstallation>,
}
impl InstalledBrowserPolicies {
    pub(crate) fn new(
        installation: BrowserInstallation,
        policies: Vec<InstalledBrowserPolicy>,
    ) -> Result<Self, BrowserServiceError> {
        if !valid_id(&installation.id) || !digest_valid(&installation.digest) || policies.len() > 16
        {
            return Err(BrowserServiceError::Invalid);
        }
        let mut table = BTreeMap::new();
        let mut public_policies = 0usize;
        for policy in policies {
            if !valid_id(&policy.id)
                || !policy.template.input_ids.is_empty()
                || validate_scope(&policy.template).is_err()
                || table.contains_key(&policy.id)
            {
                return Err(BrowserServiceError::Invalid);
            }
            if policy.template.public_ipv4_pin.is_some() {
                if !policy.template.authenticated {
                    public_policies += 1;
                }
                if (public_policies > 1 && !policy.template.authenticated)
                    || !policy.template.origin.starts_with("https://")
                    || !policy.template.input_ids.is_empty()
                    || (!policy.template.authenticated
                        && policy.template.routes.iter().any(|route| {
                            route.method != anima_service_adapters::browser::BrowserMethod::Get
                        }))
                {
                    return Err(BrowserServiceError::Invalid);
                }
            } else {
                // Preserve the exact numeric-loopback operator boundary.
                let port = policy
                    .template
                    .origin
                    .strip_prefix("http://127.0.0.1:")
                    .and_then(|p| p.parse::<u16>().ok());
                if port.is_none_or(|p| p == 0)
                    || policy.template.origin != format!("http://127.0.0.1:{}", port.unwrap())
                {
                    return Err(BrowserServiceError::Invalid);
                }
            }
            scope_digest(&policy.template).map_err(|_| BrowserServiceError::Invalid)?;
            table.insert(policy.id.clone(), policy);
        }
        Ok(Self {
            policies: table,
            installation: Some(installation),
        })
    }
}
pub(super) fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub(super) fn validate_save(input: &SaveBrowserScope) -> Result<(), BrowserServiceError> {
    let mut ids = HashSet::new();
    if !valid_id(&input.policy_id)
        || input.profile_id.as_ref().is_some_and(|id| !valid_id(id))
        || input.selected_input_ids.len() > 16
        || input
            .selected_input_ids
            .iter()
            .any(|id| !valid_id(id) || !ids.insert(id))
        || input
            .expected_version
            .is_some_and(|n| n == 0 || n > MAX_BROWSER_SAFE_INTEGER)
    {
        return Err(BrowserServiceError::Invalid);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // Active/Stopping/Unavailable belong to the held actor lane.
pub(crate) enum BrowserScopeState {
    Disabled,
    Scoped,
    Active,
    OwnerControl,
    Stopping,
    Closed,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserDocumentView {
    pub(crate) document_id: String,
    pub(crate) document_generation: u64,
    pub(crate) observation_digest: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum BrowserArtifactKindView {
    Screenshot,
    Csv,
    Text,
    Json,
    Pdf,
    Png,
    Jpeg,
}
impl From<anima_service_adapters::browser::BrowserArtifactKind> for BrowserArtifactKindView {
    fn from(value: anima_service_adapters::browser::BrowserArtifactKind) -> Self {
        use anima_service_adapters::browser::BrowserArtifactKind as Kind;
        match value {
            Kind::Screenshot => Self::Screenshot,
            Kind::Csv => Self::Csv,
            Kind::Text => Self::Text,
            Kind::Json => Self::Json,
            Kind::Pdf => Self::Pdf,
            Kind::Png => Self::Png,
            Kind::Jpeg => Self::Jpeg,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserArtifactView {
    pub(crate) id: String,
    pub(crate) kind: BrowserArtifactKindView,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserScopeView {
    pub(crate) owner_control: bool,
    pub(crate) profile: Option<super::profiles::BrowserProfileView>,
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) run_id: String,
    pub(crate) session_id: String,
    pub(crate) version: u64,
    pub(crate) enabled: bool,
    pub(crate) state: BrowserScopeState,
    pub(crate) policy_id: String,
    pub(crate) origin: String,
    pub(crate) policy_digest: String,
    pub(crate) installation_id: String,
    pub(crate) installation_digest: String,
    pub(crate) selected_input_ids: Vec<String>,
    pub(crate) expires_at_ms: u64,
    pub(crate) actions_used: u16,
    pub(crate) requests_used: u16,
    pub(crate) wire_bytes_used: u32,
    pub(crate) physical_lease_held: bool,
    pub(crate) document: Option<BrowserDocumentView>,
    pub(crate) artifacts: Vec<BrowserArtifactView>,
    pub(crate) tool_names: Vec<String>,
    pub(crate) error: Option<String>,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserPolicyRouteView {
    pub(crate) id: String,
    pub(crate) method: String,
    pub(crate) path: String,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserLimitsView {
    pub(crate) actions: u16,
    pub(crate) requests: u16,
    pub(crate) scope_ms: u32,
    pub(crate) frame_bytes: u32,
    pub(crate) wire_bytes: u32,
    pub(crate) observation_nodes: u16,
    pub(crate) observation_text_bytes: u32,
    pub(crate) input_bytes: u32,
    pub(crate) output_bytes: u32,
    pub(crate) screenshot_bytes: u32,
}
impl From<&anima_service_adapters::browser::BrowserLimits> for BrowserLimitsView {
    fn from(l: &anima_service_adapters::browser::BrowserLimits) -> Self {
        Self {
            actions: l.actions,
            requests: l.requests,
            scope_ms: l.scope_ms,
            frame_bytes: l.frame_bytes,
            wire_bytes: l.wire_bytes,
            observation_nodes: l.observation_nodes,
            observation_text_bytes: l.observation_text_bytes,
            input_bytes: l.input_bytes,
            output_bytes: l.output_bytes,
            screenshot_bytes: l.screenshot_bytes,
        }
    }
}
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserPolicyView {
    pub(crate) interactive: bool,
    pub(crate) allowed_origins: Vec<String>,
    pub(crate) version: u64,
    pub(crate) authenticated: bool,
    pub(crate) id: String,
    pub(crate) origin: String,
    pub(crate) routes: Vec<BrowserPolicyRouteView>,
    pub(crate) policy_digest: String,
    pub(crate) installation_id: String,
    pub(crate) installation_digest: String,
    pub(crate) limits: BrowserLimitsView,
    pub(crate) worker_available: bool,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct BrowserScopeEnvelope {
    pub(crate) scope: Option<BrowserScopeView>,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct BrowserSavedScopeEnvelope {
    pub(crate) scope: BrowserScopeView,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct BrowserScopesEnvelope {
    pub(crate) scopes: Vec<BrowserScopeView>,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct BrowserPoliciesEnvelope {
    pub(crate) policies: Vec<BrowserPolicyView>,
}
