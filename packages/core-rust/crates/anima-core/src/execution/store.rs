//! Portable execution-store contracts and bounded persistence requests.

mod conformance;

pub use conformance::{
    assert_execution_store_conformance, execution_store_conformance_manifest_inventory,
};

use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::Arc;

use uuid::Uuid;

use super::{
    ApprovalResumeClaim, CheckpointV1, CommandReceipt, CompletedInvocationRecord, ExecutionError,
    ExecutionLease, GrantAuthorityBinding, GrantAuthorityKey, InvocationAttemptRecord, Run,
    RuntimeCommand, RuntimeEvent, Session, SessionConcurrencyPolicy, Step,
};
use crate::{
    ApprovalDecision, AutonomyGrant, DurableCapabilityResult, GrantConsumption, GrantEffect,
    GrantStatus, PolicyContext, PolicyDecision, PolicyEngine, PolicyEvaluation, RiskLevel,
};

pub const MAX_DURABLE_RUN_INPUT_BYTES: usize = 32 * 1024;

/// The owner/run/session-bound user input that starts one durable run.
///
/// The payload is deliberately bounded and text-only. Hosts that persist non-empty inputs must
/// use a persistence protection mode that permits model payload surfaces.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableRunInput {
    owner_id: Uuid,
    session_id: Uuid,
    run_id: Uuid,
    text: String,
}

impl DurableRunInput {
    pub fn text(
        owner_id: Uuid,
        session_id: Uuid,
        run_id: Uuid,
        text: impl Into<String>,
    ) -> Result<Self, ExecutionStoreError> {
        let value = Self {
            owner_id,
            session_id,
            run_id,
            text: text.into(),
        };
        value.validate()?;
        Ok(value)
    }

    fn empty(owner_id: Uuid, session_id: Uuid, run_id: Uuid) -> Self {
        Self {
            owner_id,
            session_id,
            run_id,
            text: String::new(),
        }
    }

    fn validate(&self) -> Result<(), ExecutionStoreError> {
        if self.owner_id.is_nil()
            || self.session_id.is_nil()
            || self.run_id.is_nil()
            || self.text.len() > MAX_DURABLE_RUN_INPUT_BYTES
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        Ok(())
    }

    pub fn owner_id(&self) -> Uuid {
        self.owner_id
    }

    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    pub fn text_value(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

pub const MAX_COMMIT_EVENTS: usize = 256;
pub const MAX_COMMIT_STEPS: usize = 128;
pub const MAX_COMMIT_ATTEMPTS: usize = 256;
pub const MAX_COMMIT_RESULTS: usize = 128;
pub const MAX_COMMIT_BATCH_ITEMS: usize = 512;
pub const MAX_STORE_READ_PAGE_SIZE: u32 = 256;
pub const MAX_STORE_READ_CURSOR_BYTES: usize = 256;
const MAX_COMMIT_CHECKPOINT_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreReadCursor(String);

impl StoreReadCursor {
    pub fn from_opaque(value: impl Into<String>) -> Result<Self, ExecutionStoreError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_STORE_READ_CURSOR_BYTES || !value.is_ascii() {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::BoundsExceeded,
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreReadPage {
    cursor: Option<StoreReadCursor>,
    limit: u32,
}

impl StoreReadPage {
    pub fn new(cursor: Option<StoreReadCursor>, limit: u32) -> Result<Self, ExecutionStoreError> {
        if limit == 0 || limit > MAX_STORE_READ_PAGE_SIZE {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::BoundsExceeded,
            ));
        }
        Ok(Self { cursor, limit })
    }

    pub fn first(limit: u32) -> Result<Self, ExecutionStoreError> {
        Self::new(None, limit)
    }

    pub fn after(cursor: StoreReadCursor, limit: u32) -> Result<Self, ExecutionStoreError> {
        Self::new(Some(cursor), limit)
    }

    pub fn cursor(&self) -> Option<&StoreReadCursor> {
        self.cursor.as_ref()
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreHistoryPage<T> {
    items: Vec<T>,
    next_cursor: Option<StoreReadCursor>,
}

impl<T> StoreHistoryPage<T> {
    pub fn new(
        items: Vec<T>,
        next_cursor: Option<StoreReadCursor>,
    ) -> Result<Self, ExecutionStoreError> {
        if items.len() > MAX_STORE_READ_PAGE_SIZE as usize
            || items.is_empty() && next_cursor.is_some()
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::BoundsExceeded,
            ));
        }
        Ok(Self { items, next_cursor })
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn into_items(self) -> Vec<T> {
        self.items
    }

    pub fn next_cursor(&self) -> Option<&StoreReadCursor> {
        self.next_cursor.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventReplayPage {
    events: Vec<RuntimeEvent>,
    next_cursor: Option<StoreReadCursor>,
}

impl EventReplayPage {
    pub fn new(
        events: Vec<RuntimeEvent>,
        next_cursor: Option<StoreReadCursor>,
    ) -> Result<Self, ExecutionStoreError> {
        if events.len() > MAX_STORE_READ_PAGE_SIZE as usize
            || events.is_empty() && next_cursor.is_some()
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::BoundsExceeded,
            ));
        }
        if let Some(first) = events.first() {
            RuntimeEvent::validate_batch(first.sequence(), &events)
                .map_err(|_| ExecutionStoreError::new(ExecutionStoreErrorCode::EventConflict))?;
        }
        Ok(Self {
            events,
            next_cursor,
        })
    }

    pub fn events(&self) -> &[RuntimeEvent] {
        &self.events
    }

    pub fn into_events(self) -> Vec<RuntimeEvent> {
        self.events
    }

    pub fn next_cursor(&self) -> Option<&StoreReadCursor> {
        self.next_cursor.as_ref()
    }
}

pub type AuthoritativeGrantStatus = GrantStatus;

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritativeGrantState {
    owner_id: Uuid,
    authority_key: GrantAuthorityKey,
    full_grant_digest: String,
    scope_digest: String,
    revision: u32,
    status: GrantStatus,
    effect: GrantEffect,
    maximum_risk: RiskLevel,
    valid_from_ms: i64,
    valid_until_ms: Option<i64>,
    remaining_uses: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoritativeGrantStateWire {
    owner_id: Uuid,
    authority_key: String,
    full_grant_digest: String,
    scope_digest: String,
    revision: u32,
    status: GrantStatus,
    effect: GrantEffect,
    maximum_risk: RiskLevel,
    valid_from_ms: i64,
    valid_until_ms: Option<i64>,
    remaining_uses: Option<u32>,
}

impl AuthoritativeGrantState {
    pub fn from_grant(owner_id: Uuid, grant: &AutonomyGrant) -> Result<Self, ExecutionStoreError> {
        if owner_id.is_nil() {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        let binding =
            GrantAuthorityBinding::from_grant(grant).map_err(ExecutionStoreError::from)?;
        Self::from_binding(owner_id, binding)
    }

    fn from_binding(
        owner_id: Uuid,
        binding: GrantAuthorityBinding,
    ) -> Result<Self, ExecutionStoreError> {
        if owner_id.is_nil() {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        Ok(Self {
            owner_id,
            authority_key: binding.authority_key().clone(),
            full_grant_digest: binding.full_grant_digest().to_owned(),
            scope_digest: binding.scope_digest().to_owned(),
            revision: binding.revision(),
            status: binding.status(),
            effect: binding.effect(),
            maximum_risk: binding.maximum_risk(),
            valid_from_ms: binding.valid_from_ms(),
            valid_until_ms: binding.valid_until_ms(),
            remaining_uses: binding.remaining_uses(),
        })
    }

    fn binding(&self) -> Result<GrantAuthorityBinding, ExecutionStoreError> {
        GrantAuthorityBinding::from_parts(
            self.authority_key.as_str().to_owned(),
            self.full_grant_digest.clone(),
            self.scope_digest.clone(),
            self.revision,
            self.status,
            self.effect,
            self.maximum_risk,
            self.valid_from_ms,
            self.valid_until_ms,
            self.remaining_uses,
        )
        .map_err(ExecutionStoreError::from)
    }

    pub fn owner_id(&self) -> Uuid {
        self.owner_id
    }
    pub fn authority_key(&self) -> &GrantAuthorityKey {
        &self.authority_key
    }
    pub fn authority_key_encoded(&self) -> &str {
        self.authority_key.as_str()
    }
    /// Digest of immutable grant authority fields; the live use count is bound separately.
    pub fn full_grant_digest(&self) -> &str {
        &self.full_grant_digest
    }
    pub fn scope_digest(&self) -> &str {
        &self.scope_digest
    }
    pub fn revision(&self) -> u32 {
        self.revision
    }
    pub fn status(&self) -> GrantStatus {
        self.status
    }
    pub fn effect(&self) -> GrantEffect {
        self.effect
    }
    pub fn maximum_risk(&self) -> RiskLevel {
        self.maximum_risk
    }
    pub fn valid_from_ms(&self) -> i64 {
        self.valid_from_ms
    }
    pub fn valid_until_ms(&self) -> Option<i64> {
        self.valid_until_ms
    }
    pub fn remaining_uses(&self) -> Option<u32> {
        self.remaining_uses
    }

    fn as_revoked(&self) -> Self {
        let mut revoked = self.clone();
        revoked.status = GrantStatus::Revoked;
        revoked
    }

    /// Validates that this exact authoritative snapshot is active at adapter time.
    pub fn validate_binding(
        &self,
        binding: &GrantAuthorityBinding,
        now_ms: u64,
    ) -> Result<(), ExecutionStoreError> {
        let now_ms = i64::try_from(now_ms)
            .map_err(|_| ExecutionStoreError::new(ExecutionStoreErrorCode::GrantConflict))?;
        if self.binding()? != *binding
            || self.status != GrantStatus::Active
            || now_ms < self.valid_from_ms
            || self.valid_until_ms.is_some_and(|until| now_ms >= until)
            || self.remaining_uses == Some(0)
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::GrantConflict,
            ));
        }
        Ok(())
    }

    /// Returns the next counted-grant snapshot after validating the exact immutable binding.
    pub fn consume(
        &self,
        binding: &GrantAuthorityBinding,
        now_ms: u64,
    ) -> Result<Self, ExecutionStoreError> {
        self.validate_binding(binding, now_ms)?;
        let remaining = self
            .remaining_uses
            .ok_or_else(|| ExecutionStoreError::new(ExecutionStoreErrorCode::GrantConflict))?;
        let mut consumed = self.clone();
        consumed.remaining_uses = Some(
            remaining
                .checked_sub(1)
                .ok_or_else(|| ExecutionStoreError::new(ExecutionStoreErrorCode::GrantConflict))?,
        );
        Ok(consumed)
    }
}

impl<'de> Deserialize<'de> for AuthoritativeGrantState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = AuthoritativeGrantStateWire::deserialize(deserializer)?;
        let binding = GrantAuthorityBinding::from_parts(
            wire.authority_key,
            wire.full_grant_digest,
            wire.scope_digest,
            wire.revision,
            wire.status,
            wire.effect,
            wire.maximum_risk,
            wire.valid_from_ms,
            wire.valid_until_ms,
            wire.remaining_uses,
        )
        .map_err(serde::de::Error::custom)?;
        Self::from_binding(wire.owner_id, binding).map_err(serde::de::Error::custom)
    }
}

impl fmt::Debug for AuthoritativeGrantState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthoritativeGrantState")
            .field("owner_id", &"REDACTED")
            .field("authority_key", &"REDACTED")
            .field("full_grant_digest", &"REDACTED")
            .field("scope_digest", &"REDACTED")
            .field("revision", &self.revision)
            .field("status", &self.status)
            .field("effect", &self.effect)
            .field("maximum_risk", &self.maximum_risk)
            .field("valid_from_ms", &self.valid_from_ms)
            .field("valid_until_ms", &self.valid_until_ms)
            .field("remaining_uses", &self.remaining_uses)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoritativeGrantChange {
    kind: AuthoritativeGrantChangeKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthoritativeGrantChangeKind {
    Create(AuthoritativeGrantState),
    Update {
        expected_revision: u32,
        state: AuthoritativeGrantState,
    },
    Revoke {
        authority_key: GrantAuthorityKey,
        expected_revision: u32,
    },
}

impl AuthoritativeGrantChange {
    pub fn create(state: AuthoritativeGrantState) -> Self {
        Self {
            kind: AuthoritativeGrantChangeKind::Create(state),
        }
    }

    pub fn update(
        expected_revision: u32,
        state: AuthoritativeGrantState,
    ) -> Result<Self, ExecutionStoreError> {
        if expected_revision == 0 || state.revision() <= expected_revision {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        Ok(Self {
            kind: AuthoritativeGrantChangeKind::Update {
                expected_revision,
                state,
            },
        })
    }

    pub fn revoke(
        authority_key: GrantAuthorityKey,
        expected_revision: u32,
    ) -> Result<Self, ExecutionStoreError> {
        if expected_revision == 0 {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        Ok(Self {
            kind: AuthoritativeGrantChangeKind::Revoke {
                authority_key,
                expected_revision,
            },
        })
    }

    pub fn kind(&self) -> &AuthoritativeGrantChangeKind {
        &self.kind
    }

    pub fn expected_revision(&self) -> Option<u32> {
        match &self.kind {
            AuthoritativeGrantChangeKind::Create(_) => None,
            AuthoritativeGrantChangeKind::Update {
                expected_revision, ..
            }
            | AuthoritativeGrantChangeKind::Revoke {
                expected_revision, ..
            } => Some(*expected_revision),
        }
    }

    pub fn new_state(&self) -> Option<&AuthoritativeGrantState> {
        match &self.kind {
            AuthoritativeGrantChangeKind::Create(state)
            | AuthoritativeGrantChangeKind::Update { state, .. } => Some(state),
            AuthoritativeGrantChangeKind::Revoke { .. } => None,
        }
    }

    pub fn authority_key(&self) -> &GrantAuthorityKey {
        match &self.kind {
            AuthoritativeGrantChangeKind::Create(state)
            | AuthoritativeGrantChangeKind::Update { state, .. } => state.authority_key(),
            AuthoritativeGrantChangeKind::Revoke { authority_key, .. } => authority_key,
        }
    }

    /// Applies a validated grant mutation as a pure CAS transition for durable adapters.
    pub fn apply_to(
        &self,
        current: Option<&AuthoritativeGrantState>,
    ) -> Result<AuthoritativeGrantState, ExecutionStoreError> {
        match (&self.kind, current) {
            (AuthoritativeGrantChangeKind::Create(next), None) => Ok(next.clone()),
            (AuthoritativeGrantChangeKind::Create(_), Some(_)) => Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::VersionConflict,
            )),
            (
                AuthoritativeGrantChangeKind::Update {
                    expected_revision,
                    state: next,
                },
                Some(current),
            ) if current.owner_id == next.owner_id
                && current.authority_key == next.authority_key
                && current.revision == *expected_revision
                && next.revision > current.revision =>
            {
                Ok(next.clone())
            }
            (
                AuthoritativeGrantChangeKind::Revoke {
                    authority_key,
                    expected_revision,
                },
                Some(current),
            ) if current.authority_key == *authority_key
                && current.revision == *expected_revision =>
            {
                Ok(current.as_revoked())
            }
            _ => Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::VersionConflict,
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoritativePolicyStatus {
    Active,
    Revoked,
}

/// Store-authoritative high-water mark for one owner's pinned definition policy.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritativePolicyState {
    owner_id: Uuid,
    agent_definition_id: String,
    agent_definition_version: u32,
    revision: u32,
    status: AuthoritativePolicyStatus,
    valid_until_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoritativePolicyStateWire {
    owner_id: Uuid,
    agent_definition_id: String,
    agent_definition_version: u32,
    revision: u32,
    status: AuthoritativePolicyStatus,
    valid_until_ms: Option<u64>,
}

impl AuthoritativePolicyState {
    pub fn active(
        owner_id: Uuid,
        agent_definition_id: impl Into<String>,
        agent_definition_version: u32,
        revision: u32,
        valid_until_ms: Option<u64>,
    ) -> Result<Self, ExecutionStoreError> {
        let value = Self {
            owner_id,
            agent_definition_id: agent_definition_id.into(),
            agent_definition_version,
            revision,
            status: AuthoritativePolicyStatus::Active,
            valid_until_ms,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), ExecutionStoreError> {
        if self.owner_id.is_nil()
            || self.agent_definition_id.trim().is_empty()
            || self.agent_definition_id.len() > crate::MAX_CAPABILITY_ID_BYTES
            || self.agent_definition_version == 0
            || self.revision == 0
            || self.valid_until_ms == Some(0)
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        Ok(())
    }

    pub fn owner_id(&self) -> Uuid {
        self.owner_id
    }
    pub fn agent_definition_id(&self) -> &str {
        &self.agent_definition_id
    }
    pub fn agent_definition_version(&self) -> u32 {
        self.agent_definition_version
    }
    pub fn revision(&self) -> u32 {
        self.revision
    }
    pub fn status(&self) -> AuthoritativePolicyStatus {
        self.status
    }
    pub fn valid_until_ms(&self) -> Option<u64> {
        self.valid_until_ms
    }

    fn key(&self) -> (String, u32) {
        (
            self.agent_definition_id.clone(),
            self.agent_definition_version,
        )
    }

    fn revoked(&self) -> Self {
        let mut value = self.clone();
        value.status = AuthoritativePolicyStatus::Revoked;
        value
    }

    pub(super) fn validates(&self, guard: &DispatchPolicyGuard, now_ms: u64) -> bool {
        self.owner_id == guard.owner_id
            && self.agent_definition_id == guard.agent_definition_id
            && self.agent_definition_version == guard.agent_definition_version
            && self.revision == guard.policy_revision
            && self.status == AuthoritativePolicyStatus::Active
            && self.valid_until_ms.is_none_or(|until| now_ms < until)
    }
}

impl<'de> Deserialize<'de> for AuthoritativePolicyState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = AuthoritativePolicyStateWire::deserialize(deserializer)?;
        let value = Self {
            owner_id: wire.owner_id,
            agent_definition_id: wire.agent_definition_id,
            agent_definition_version: wire.agent_definition_version,
            revision: wire.revision,
            status: wire.status,
            valid_until_ms: wire.valid_until_ms,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl fmt::Debug for AuthoritativePolicyState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthoritativePolicyState")
            .field("owner_id", &"REDACTED")
            .field("agent_definition_id", &"REDACTED")
            .field("agent_definition_version", &self.agent_definition_version)
            .field("revision", &self.revision)
            .field("status", &self.status)
            .field("valid_until_ms", &self.valid_until_ms)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthoritativePolicyChange {
    Create(AuthoritativePolicyState),
    Update {
        expected_revision: u32,
        state: AuthoritativePolicyState,
    },
    Revoke {
        agent_definition_id: String,
        agent_definition_version: u32,
        expected_revision: u32,
    },
}

impl AuthoritativePolicyChange {
    pub fn create(state: AuthoritativePolicyState) -> Self {
        Self::Create(state)
    }

    pub fn update(
        expected_revision: u32,
        state: AuthoritativePolicyState,
    ) -> Result<Self, ExecutionStoreError> {
        if expected_revision == 0 || state.revision <= expected_revision {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        Ok(Self::Update {
            expected_revision,
            state,
        })
    }

    pub fn revoke(
        agent_definition_id: impl Into<String>,
        agent_definition_version: u32,
        expected_revision: u32,
    ) -> Result<Self, ExecutionStoreError> {
        let agent_definition_id = agent_definition_id.into();
        if agent_definition_id.trim().is_empty()
            || agent_definition_version == 0
            || expected_revision == 0
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        Ok(Self::Revoke {
            agent_definition_id,
            agent_definition_version,
            expected_revision,
        })
    }

    pub fn key(&self) -> (String, u32) {
        match self {
            Self::Create(state) | Self::Update { state, .. } => state.key(),
            Self::Revoke {
                agent_definition_id,
                agent_definition_version,
                ..
            } => (agent_definition_id.clone(), *agent_definition_version),
        }
    }

    pub fn apply_to(
        &self,
        current: Option<&AuthoritativePolicyState>,
    ) -> Result<AuthoritativePolicyState, ExecutionStoreError> {
        match (self, current) {
            (Self::Create(next), None) => Ok(next.clone()),
            (Self::Create(_), Some(_)) => Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::VersionConflict,
            )),
            (
                Self::Update {
                    expected_revision,
                    state: next,
                },
                Some(current),
            ) if current.owner_id == next.owner_id
                && current.key() == next.key()
                && current.revision == *expected_revision
                && next.revision > current.revision =>
            {
                Ok(next.clone())
            }
            (
                Self::Revoke {
                    agent_definition_id,
                    agent_definition_version,
                    expected_revision,
                },
                Some(current),
            ) if current.agent_definition_id == *agent_definition_id
                && current.agent_definition_version == *agent_definition_version
                && current.revision == *expected_revision =>
            {
                Ok(current.revoked())
            }
            _ => Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::VersionConflict,
            )),
        }
    }
}

/// Opaque exact-policy proof required on every external dispatch preparation.
#[derive(Clone)]
pub struct DispatchPolicyGuard {
    owner_id: Uuid,
    run_id: Uuid,
    agent_definition_id: String,
    agent_definition_version: u32,
    logical_invocation_id: Uuid,
    capability_id: String,
    manifest_version: u32,
    canonical_argument_digest: Uuid,
    policy_revision: u32,
    decision_digest: String,
}

impl DispatchPolicyGuard {
    pub fn from_current_policy(
        owner_id: Uuid,
        context: &PolicyContext,
        grants: &[AutonomyGrant],
        approval: Option<&ApprovalDecision>,
        evaluation: &PolicyEvaluation,
    ) -> Result<Self, ExecutionStoreError> {
        if owner_id.is_nil()
            || context.owner_id != owner_id.to_string()
            || !matches!(evaluation.decision, PolicyDecision::Allow(_))
            || PolicyEngine::evaluate_with_approval(context, grants, approval)
                .ok()
                .as_ref()
                != Some(evaluation)
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::PolicyConflict,
            ));
        }
        let digest = serde_jcs::to_vec(evaluation)
            .map_err(|_| ExecutionStoreError::new(ExecutionStoreErrorCode::PolicyConflict))?;
        Ok(Self {
            owner_id,
            run_id: context.run_id,
            agent_definition_id: context.agent_definition_id.clone(),
            agent_definition_version: context.agent_definition_version,
            logical_invocation_id: context.logical_invocation_id,
            capability_id: context.capability_id.clone(),
            manifest_version: context.manifest_version,
            canonical_argument_digest: context.canonical_argument_digest,
            policy_revision: context.policy_revision,
            decision_digest: format!("sha256:{}", hex::encode(Sha256::digest(digest))),
        })
    }

    pub fn policy_revision(&self) -> u32 {
        self.policy_revision
    }

    pub(super) fn agent_definition_id(&self) -> &str {
        &self.agent_definition_id
    }

    pub(super) fn agent_definition_version(&self) -> u32 {
        self.agent_definition_version
    }

    pub fn matches_attempt(&self, owner_id: Uuid, attempt: &InvocationAttemptRecord) -> bool {
        self.owner_id == owner_id
            && self.run_id == attempt.invocation().run_id()
            && self.logical_invocation_id == attempt.invocation().id()
            && self.capability_id == attempt.invocation().capability_id()
            && self.manifest_version == attempt.invocation().manifest_version()
            && self.canonical_argument_digest == attempt.invocation().canonical_argument_digest()
            && !self.decision_digest.is_empty()
    }
}

impl fmt::Debug for DispatchPolicyGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DispatchPolicyGuard")
            .field("owner_id", &"REDACTED")
            .field("run_id", &self.run_id)
            .field("logical_invocation_id", &self.logical_invocation_id)
            .field("policy_revision", &self.policy_revision)
            .field("decision_digest", &"REDACTED")
            .finish()
    }
}

/// Input for atomically creating a durable run and claiming its session when required.
#[derive(Clone, Debug)]
pub struct CreateRun {
    owner_id: Uuid,
    session: Session,
    run: Run,
    expected_session_version: u64,
    concurrency_policy: SessionConcurrencyPolicy,
    initial_input: DurableRunInput,
}

impl CreateRun {
    pub fn new_for_owner(
        owner_id: Uuid,
        session: Session,
        run: Run,
        expected_session_version: u64,
        concurrency_policy: SessionConcurrencyPolicy,
    ) -> Self {
        let initial_input = DurableRunInput::empty(owner_id, session.id(), run.id());
        Self {
            owner_id,
            session,
            run,
            expected_session_version,
            concurrency_policy,
            initial_input,
        }
    }

    pub fn with_initial_input(
        mut self,
        initial_input: DurableRunInput,
    ) -> Result<Self, ExecutionStoreError> {
        initial_input.validate()?;
        if initial_input.owner_id() != self.owner_id
            || initial_input.session_id() != self.session.id()
            || initial_input.run_id() != self.run.id()
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        self.initial_input = initial_input;
        Ok(self)
    }

    pub fn owner_id(&self) -> Uuid {
        self.owner_id
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn run(&self) -> &Run {
        &self.run
    }

    pub fn expected_session_version(&self) -> u64 {
        self.expected_session_version
    }

    pub fn concurrency_policy(&self) -> SessionConcurrencyPolicy {
        self.concurrency_policy
    }

    pub fn initial_input(&self) -> &DurableRunInput {
        &self.initial_input
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryRun {
    command_id: Uuid,
    source_run_id: Uuid,
    new_run_id: Uuid,
}

impl RetryRun {
    pub fn new(
        command_id: Uuid,
        source_run_id: Uuid,
        new_run_id: Uuid,
    ) -> Result<Self, ExecutionStoreError> {
        if command_id.is_nil()
            || source_run_id.is_nil()
            || new_run_id.is_nil()
            || source_run_id == new_run_id
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        Ok(Self {
            command_id,
            source_run_id,
            new_run_id,
        })
    }

    pub fn command_id(&self) -> Uuid {
        self.command_id
    }

    pub fn source_run_id(&self) -> Uuid {
        self.source_run_id
    }

    pub fn new_run_id(&self) -> Uuid {
        self.new_run_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryRunOutcome {
    source_run_id: Uuid,
    run: StoredRun,
    receipt: CommandReceipt,
}

impl RetryRunOutcome {
    pub fn new(
        source_run_id: Uuid,
        run: StoredRun,
        receipt: CommandReceipt,
    ) -> Result<Self, ExecutionStoreError> {
        if source_run_id.is_nil() || source_run_id == run.run().id() {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        Ok(Self {
            source_run_id,
            run,
            receipt,
        })
    }

    pub fn source_run_id(&self) -> Uuid {
        self.source_run_id
    }

    pub fn run(&self) -> &StoredRun {
        &self.run
    }

    pub fn receipt(&self) -> &CommandReceipt {
        &self.receipt
    }
}

/// A versioned durable run returned by an execution-store adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredRun {
    owner_id: Uuid,
    run: Run,
    run_version: u64,
    session_version: u64,
    initial_input: DurableRunInput,
}

/// A validated approval claim and its one-time grant consumption, committed together with a run.
#[derive(Clone, Debug)]
pub struct ApprovalGrantMutation {
    claim: ApprovalResumeClaim,
}

/// Exact counted AutoAllow authority consumed atomically with one dispatch-preparation commit.
#[derive(Clone)]
pub struct DispatchGrantMutation {
    owner_id: Uuid,
    run_id: Uuid,
    logical_invocation_id: Uuid,
    capability_id: String,
    canonical_argument_digest: Uuid,
    policy_revision: u32,
    consumption: GrantConsumption,
    authority_binding: GrantAuthorityBinding,
    remaining_uses: u32,
}

impl DispatchGrantMutation {
    pub fn from_current_policy(
        owner_id: Uuid,
        context: &PolicyContext,
        grants: &[AutonomyGrant],
        evaluation: &PolicyEvaluation,
    ) -> Result<Option<Self>, ExecutionStoreError> {
        let Some(consumption) = evaluation.consumption.as_ref() else {
            return Ok(None);
        };
        if owner_id.is_nil()
            || context.owner_id != owner_id.to_string()
            || context.run_id.is_nil()
            || context.logical_invocation_id != consumption.logical_invocation_id
            || context.canonical_argument_digest.is_nil()
            || context.policy_revision == 0
            || PolicyEngine::evaluate(context, grants).ok().as_ref() != Some(evaluation)
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::GrantConflict,
            ));
        }
        let grant = grants
            .iter()
            .find(|grant| {
                grant.id == consumption.grant_id && grant.revision == consumption.grant_revision
            })
            .ok_or_else(|| ExecutionStoreError::new(ExecutionStoreErrorCode::GrantConflict))?;
        if grant.effect != GrantEffect::AutoAllow {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::GrantConflict,
            ));
        }
        let remaining_uses = grant
            .remaining_uses
            .filter(|remaining| *remaining > 0)
            .ok_or_else(|| ExecutionStoreError::new(ExecutionStoreErrorCode::GrantConflict))?;
        let authority_binding =
            GrantAuthorityBinding::from_grant(grant).map_err(ExecutionStoreError::from)?;
        if !authority_binding.matches_consumption(consumption) {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::GrantConflict,
            ));
        }
        Ok(Some(Self {
            owner_id,
            run_id: context.run_id,
            logical_invocation_id: context.logical_invocation_id,
            capability_id: context.capability_id.clone(),
            canonical_argument_digest: context.canonical_argument_digest,
            policy_revision: context.policy_revision,
            consumption: consumption.clone(),
            authority_binding,
            remaining_uses,
        }))
    }

    pub fn consumption(&self) -> &GrantConsumption {
        &self.consumption
    }
    pub fn authority_binding(&self) -> &GrantAuthorityBinding {
        &self.authority_binding
    }
    pub fn remaining_uses(&self) -> u32 {
        self.remaining_uses
    }
    pub fn matches_attempt(&self, owner_id: Uuid, attempt: &InvocationAttemptRecord) -> bool {
        self.owner_id == owner_id
            && self.run_id == attempt.invocation().run_id()
            && self.logical_invocation_id == attempt.invocation().id()
            && self.capability_id == attempt.invocation().capability_id()
            && self.canonical_argument_digest == attempt.invocation().canonical_argument_digest()
            && self.policy_revision > 0
    }
}

impl fmt::Debug for DispatchGrantMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DispatchGrantMutation")
            .field("owner_id", &"REDACTED")
            .field("run_id", &self.run_id)
            .field("logical_invocation_id", &self.logical_invocation_id)
            .field("capability_id", &"REDACTED")
            .field("canonical_argument_digest", &self.canonical_argument_digest)
            .field("policy_revision", &self.policy_revision)
            .field("consumption", &self.consumption)
            .field("authority_binding", &"REDACTED")
            .field("remaining_uses", &self.remaining_uses)
            .finish()
    }
}

impl ApprovalGrantMutation {
    /// Preserves Task 4's validated approval claim and its exact consumption as one commit input.
    pub fn from_claim(claim: ApprovalResumeClaim) -> Self {
        Self { claim }
    }

    pub fn claim(&self) -> &ApprovalResumeClaim {
        &self.claim
    }

    pub fn grant_consumption(&self) -> Option<&GrantConsumption> {
        self.claim.grant_consumption()
    }

    pub fn remaining_uses(&self) -> Option<u32> {
        self.claim
            .grant_consumption_snapshot()
            .map(|snapshot| snapshot.remaining_uses())
    }

    pub fn grant_id(&self) -> Option<&str> {
        self.claim.grant_id()
    }

    pub fn grant_revision(&self) -> Option<u32> {
        self.claim.grant_revision()
    }

    pub fn grant_remaining_uses(&self) -> Option<u32> {
        self.claim.grant_remaining_uses()
    }
}

/// A durable result is keyed by a logical invocation and can only be recorded identically once.
#[derive(Clone, Debug)]
pub struct DurableResultMutation {
    completed: CompletedInvocationRecord,
    result: DurableCapabilityResult,
}

/// Explicit checkpoint freshness decision for one atomic execution commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckpointMutation {
    /// Retain the current checkpoint only when the post-commit aggregate is still identical to it.
    Unchanged,
    /// Replace the current checkpoint with an exact snapshot of the post-commit aggregate.
    Replace(Box<CheckpointV1>),
    /// Invalidate any current checkpoint while advancing the checkpoint generation.
    Clear,
}

impl DurableResultMutation {
    pub fn new(completed: CompletedInvocationRecord, result: DurableCapabilityResult) -> Self {
        Self { completed, result }
    }

    pub fn completed(&self) -> &CompletedInvocationRecord {
        &self.completed
    }

    pub fn result(&self) -> &DurableCapabilityResult {
        &self.result
    }
}

/// All state mutations which must succeed or fail together for one leased execution command.
#[derive(Clone, Debug)]
pub struct ExecutionCommit {
    expected_run_version: u64,
    expected_checkpoint_version: u64,
    lease: ExecutionLease,
    command: RuntimeCommand,
    events: Vec<RuntimeEvent>,
    steps: Vec<Step>,
    attempts: Vec<InvocationAttemptRecord>,
    results: Vec<DurableResultMutation>,
    approval: Option<ApprovalGrantMutation>,
    policy_guard: Option<DispatchPolicyGuard>,
    dispatch_grant: Option<DispatchGrantMutation>,
    checkpoint: CheckpointMutation,
    target_run: Run,
}

impl ExecutionCommit {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        expected_run_version: u64,
        expected_checkpoint_version: u64,
        lease: ExecutionLease,
        command: RuntimeCommand,
        events: Vec<RuntimeEvent>,
        steps: Vec<Step>,
        attempts: Vec<InvocationAttemptRecord>,
        results: Vec<DurableResultMutation>,
        approval: Option<ApprovalGrantMutation>,
        target_run: Run,
    ) -> Self {
        Self {
            expected_run_version,
            expected_checkpoint_version,
            lease,
            command,
            events,
            steps,
            attempts,
            results,
            approval,
            policy_guard: None,
            dispatch_grant: None,
            checkpoint: CheckpointMutation::Clear,
            target_run,
        }
    }

    pub fn with_checkpoint(mut self, checkpoint: CheckpointV1) -> Self {
        self.checkpoint = CheckpointMutation::Replace(Box::new(checkpoint));
        self
    }

    pub fn with_dispatch_grant(mut self, mutation: DispatchGrantMutation) -> Self {
        self.dispatch_grant = Some(mutation);
        self
    }

    pub fn with_policy_guard(mut self, guard: DispatchPolicyGuard) -> Self {
        self.policy_guard = Some(guard);
        self
    }

    pub fn with_checkpoint_mutation(mut self, mutation: CheckpointMutation) -> Self {
        self.checkpoint = mutation;
        self
    }

    pub fn expected_run_version(&self) -> u64 {
        self.expected_run_version
    }

    pub fn expected_checkpoint_version(&self) -> u64 {
        self.expected_checkpoint_version
    }

    pub fn lease(&self) -> &ExecutionLease {
        &self.lease
    }

    pub fn command(&self) -> &RuntimeCommand {
        &self.command
    }

    pub fn events(&self) -> &[RuntimeEvent] {
        &self.events
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub fn attempts(&self) -> &[InvocationAttemptRecord] {
        &self.attempts
    }

    pub fn results(&self) -> &[DurableResultMutation] {
        &self.results
    }

    pub fn approval(&self) -> Option<&ApprovalGrantMutation> {
        self.approval.as_ref()
    }

    pub fn dispatch_grant(&self) -> Option<&DispatchGrantMutation> {
        self.dispatch_grant.as_ref()
    }

    pub fn policy_guard(&self) -> Option<&DispatchPolicyGuard> {
        self.policy_guard.as_ref()
    }

    pub fn checkpoint(&self) -> Option<&CheckpointV1> {
        match &self.checkpoint {
            CheckpointMutation::Replace(checkpoint) => Some(checkpoint.as_ref()),
            CheckpointMutation::Unchanged | CheckpointMutation::Clear => None,
        }
    }

    pub fn checkpoint_mutation(&self) -> &CheckpointMutation {
        &self.checkpoint
    }

    pub(super) fn into_checkpoint_mutation(self) -> CheckpointMutation {
        self.checkpoint
    }

    pub fn target_run(&self) -> &Run {
        &self.target_run
    }

    pub fn validate_bounds(&self) -> Result<(), ExecutionStoreError> {
        if self.events.len() > MAX_COMMIT_EVENTS
            || self.steps.len() > MAX_COMMIT_STEPS
            || self.attempts.len() > MAX_COMMIT_ATTEMPTS
            || self.results.len() > MAX_COMMIT_RESULTS
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::BoundsExceeded,
            ));
        }
        let total = self
            .events
            .len()
            .checked_add(self.steps.len())
            .and_then(|value| value.checked_add(self.attempts.len()))
            .and_then(|value| value.checked_add(self.results.len()))
            .ok_or_else(|| ExecutionStoreError::new(ExecutionStoreErrorCode::BoundsExceeded))?;
        if total > MAX_COMMIT_BATCH_ITEMS
            || self.checkpoint().is_some_and(|checkpoint| {
                serde_jcs::to_vec(checkpoint)
                    .map_or(true, |bytes| bytes.len() > MAX_COMMIT_CHECKPOINT_BYTES)
            })
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::BoundsExceeded,
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionCommitOutcome {
    stored_run: StoredRun,
    receipt: CommandReceipt,
    checkpoint_version: u64,
    checkpoint: Option<Arc<CheckpointV1>>,
    grant_consumption: Option<GrantConsumption>,
}

impl ExecutionCommitOutcome {
    pub fn new(
        stored_run: StoredRun,
        receipt: CommandReceipt,
        checkpoint_version: u64,
        checkpoint: Option<CheckpointV1>,
        grant_consumption: Option<GrantConsumption>,
    ) -> Self {
        Self::new_shared(
            stored_run,
            receipt,
            checkpoint_version,
            checkpoint.map(Arc::new),
            grant_consumption,
        )
    }

    pub(super) fn new_shared(
        stored_run: StoredRun,
        receipt: CommandReceipt,
        checkpoint_version: u64,
        checkpoint: Option<Arc<CheckpointV1>>,
        grant_consumption: Option<GrantConsumption>,
    ) -> Self {
        Self {
            stored_run,
            receipt,
            checkpoint_version,
            checkpoint,
            grant_consumption,
        }
    }

    pub fn stored_run(&self) -> &StoredRun {
        &self.stored_run
    }

    pub fn receipt(&self) -> &CommandReceipt {
        &self.receipt
    }

    pub fn checkpoint_version(&self) -> u64 {
        self.checkpoint_version
    }

    pub fn checkpoint(&self) -> Option<&CheckpointV1> {
        self.checkpoint.as_deref()
    }

    pub fn grant_consumption(&self) -> Option<&GrantConsumption> {
        self.grant_consumption.as_ref()
    }
}

impl StoredRun {
    pub fn new(
        owner_id: Uuid,
        run: Run,
        run_version: u64,
        session_version: u64,
    ) -> Result<Self, ExecutionStoreError> {
        let initial_input = DurableRunInput::empty(owner_id, run.session_id(), run.id());
        Self::new_with_initial_input(owner_id, run, run_version, session_version, initial_input)
    }

    pub fn new_with_initial_input(
        owner_id: Uuid,
        run: Run,
        run_version: u64,
        session_version: u64,
        initial_input: DurableRunInput,
    ) -> Result<Self, ExecutionStoreError> {
        if owner_id.is_nil() || run_version == 0 || session_version == 0 {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        initial_input.validate()?;
        if initial_input.owner_id() != owner_id
            || initial_input.session_id() != run.session_id()
            || initial_input.run_id() != run.id()
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        Ok(Self {
            owner_id,
            run,
            run_version,
            session_version,
            initial_input,
        })
    }

    pub fn owner_id(&self) -> Uuid {
        self.owner_id
    }

    pub fn run(&self) -> &Run {
        &self.run
    }

    pub fn run_version(&self) -> u64 {
        self.run_version
    }

    pub fn session_version(&self) -> u64 {
        self.session_version
    }

    pub fn initial_input(&self) -> &DurableRunInput {
        &self.initial_input
    }
}

/// Stable categories returned by an [`ExecutionStore`](crate::ExecutionStore).
///
/// New categories may be added during the crate's pre-1.0 evolution. Downstream
/// callers must include a wildcard arm when matching this enum.
///
/// ```compile_fail
/// use anima_core::ExecutionStoreErrorCode;
///
/// fn classify(code: ExecutionStoreErrorCode) -> &'static str {
///     match code {
///         ExecutionStoreErrorCode::NotFound => "not_found",
///         ExecutionStoreErrorCode::VersionConflict => "version_conflict",
///         ExecutionStoreErrorCode::ActiveRunConflict => "active_run_conflict",
///         ExecutionStoreErrorCode::LeaseConflict => "lease_conflict",
///         ExecutionStoreErrorCode::LeaseExpired => "lease_expired",
///         ExecutionStoreErrorCode::CommandConflict => "command_conflict",
///         ExecutionStoreErrorCode::EventConflict => "event_conflict",
///         ExecutionStoreErrorCode::CheckpointConflict => "checkpoint_conflict",
///         ExecutionStoreErrorCode::GrantAlreadyConsumed => "grant_already_consumed",
///         ExecutionStoreErrorCode::GrantConflict => "grant_conflict",
///         ExecutionStoreErrorCode::PolicyConflict => "policy_conflict",
///         ExecutionStoreErrorCode::LineageConflict => "lineage_conflict",
///         ExecutionStoreErrorCode::HistoryConflict => "history_conflict",
///         ExecutionStoreErrorCode::BoundsExceeded => "bounds_exceeded",
///         ExecutionStoreErrorCode::ArithmeticOverflow => "arithmetic_overflow",
///         ExecutionStoreErrorCode::ResultConflict => "result_conflict",
///         ExecutionStoreErrorCode::StorageUnavailable => "storage_unavailable",
///         ExecutionStoreErrorCode::CorruptState => "corrupt_state",
///         ExecutionStoreErrorCode::InvalidRequest => "invalid_request",
///     }
/// }
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionStoreErrorCode {
    NotFound,
    VersionConflict,
    ActiveRunConflict,
    LeaseConflict,
    LeaseExpired,
    CommandConflict,
    EventConflict,
    CheckpointConflict,
    GrantAlreadyConsumed,
    GrantConflict,
    PolicyConflict,
    LineageConflict,
    HistoryConflict,
    BoundsExceeded,
    ArithmeticOverflow,
    ResultConflict,
    StorageUnavailable,
    CorruptState,
    InvalidRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionStoreError {
    code: ExecutionStoreErrorCode,
}

impl ExecutionStoreError {
    pub const fn new(code: ExecutionStoreErrorCode) -> Self {
        Self { code }
    }

    pub const fn code(self) -> ExecutionStoreErrorCode {
        self.code
    }
}

impl From<ExecutionError> for ExecutionStoreError {
    fn from(_: ExecutionError) -> Self {
        Self::new(ExecutionStoreErrorCode::InvalidRequest)
    }
}

impl fmt::Display for ExecutionStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.code {
            ExecutionStoreErrorCode::NotFound => "execution run was not found",
            ExecutionStoreErrorCode::VersionConflict => "execution version did not match",
            ExecutionStoreErrorCode::ActiveRunConflict => "session already has an active run",
            ExecutionStoreErrorCode::LeaseConflict => "execution lease is held by another worker",
            ExecutionStoreErrorCode::LeaseExpired => "execution lease is expired or fenced",
            ExecutionStoreErrorCode::CommandConflict => {
                "command identifier conflicts with its receipt"
            }
            ExecutionStoreErrorCode::EventConflict => "execution event sequence is not contiguous",
            ExecutionStoreErrorCode::CheckpointConflict => {
                "execution checkpoint did not match commit"
            }
            ExecutionStoreErrorCode::GrantAlreadyConsumed => "autonomy grant was already consumed",
            ExecutionStoreErrorCode::GrantConflict => {
                "autonomy grant authority is missing, stale, or revoked"
            }
            ExecutionStoreErrorCode::PolicyConflict => {
                "policy authority is missing, stale, expired, or revoked"
            }
            ExecutionStoreErrorCode::LineageConflict => {
                "durable result does not match completed invocation lineage"
            }
            ExecutionStoreErrorCode::HistoryConflict => {
                "execution history key conflicts with its persisted value"
            }
            ExecutionStoreErrorCode::BoundsExceeded => "execution store batch or page is too large",
            ExecutionStoreErrorCode::ArithmeticOverflow => {
                "execution store version arithmetic overflowed"
            }
            ExecutionStoreErrorCode::ResultConflict => "durable invocation result conflicts",
            ExecutionStoreErrorCode::StorageUnavailable => {
                "execution store persistence is unavailable"
            }
            ExecutionStoreErrorCode::CorruptState => {
                "execution store durable state failed integrity validation"
            }
            ExecutionStoreErrorCode::InvalidRequest => "execution store request is invalid",
        })
    }
}

impl std::error::Error for ExecutionStoreError {}

/// Adapter port for durable execution state. Each operation is atomic.
#[async_trait]
pub trait ExecutionStore: Send + Sync {
    /// Current CAS version of an owner's session, without granting a new run.
    async fn load_session_version(
        &self,
        _owner_id: Uuid,
        _session_id: Uuid,
    ) -> Result<Option<u64>, ExecutionStoreError> {
        Ok(None)
    }

    /// Exact committed final output, separate from a resumable checkpoint.
    /// Legacy adapters may omit this until their profile passes replay parity.
    async fn load_completed_output(
        &self,
        _owner_id: Uuid,
        _run_id: Uuid,
    ) -> Result<Option<crate::Content>, ExecutionStoreError> {
        Ok(None)
    }

    async fn apply_authoritative_policy(
        &self,
        owner_id: Uuid,
        change: AuthoritativePolicyChange,
    ) -> Result<AuthoritativePolicyState, ExecutionStoreError>;

    async fn load_authoritative_policy(
        &self,
        owner_id: Uuid,
        agent_definition_id: &str,
        agent_definition_version: u32,
    ) -> Result<Option<AuthoritativePolicyState>, ExecutionStoreError>;

    async fn apply_authoritative_grant(
        &self,
        owner_id: Uuid,
        change: AuthoritativeGrantChange,
    ) -> Result<AuthoritativeGrantState, ExecutionStoreError>;

    async fn load_authoritative_grant(
        &self,
        owner_id: Uuid,
        authority_key: &GrantAuthorityKey,
    ) -> Result<Option<AuthoritativeGrantState>, ExecutionStoreError>;

    async fn create_run(
        &self,
        owner_id: Uuid,
        request: CreateRun,
    ) -> Result<StoredRun, ExecutionStoreError>;

    async fn retry_run(
        &self,
        _owner_id: Uuid,
        _request: RetryRun,
    ) -> Result<RetryRunOutcome, ExecutionStoreError> {
        Err(ExecutionStoreError::new(
            ExecutionStoreErrorCode::InvalidRequest,
        ))
    }

    /// Acquires a new fence only if no current lease is active under adapter-authoritative time.
    async fn acquire_lease(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
        expected_run_version: u64,
        duration_ms: u64,
    ) -> Result<ExecutionLease, ExecutionStoreError>;

    /// Renews precisely the supplied active fence; an expired lease is never resurrected.
    async fn renew_lease(
        &self,
        owner_id: Uuid,
        lease: ExecutionLease,
        duration_ms: u64,
    ) -> Result<ExecutionLease, ExecutionStoreError>;

    async fn commit_execution(
        &self,
        owner_id: Uuid,
        commit: ExecutionCommit,
    ) -> Result<ExecutionCommitOutcome, ExecutionStoreError>;

    async fn load_run(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<StoredRun>, ExecutionStoreError>;

    async fn load_checkpoint(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<(u64, CheckpointV1)>, ExecutionStoreError>;

    async fn load_steps_page(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
        page: StoreReadPage,
    ) -> Result<StoreHistoryPage<Step>, ExecutionStoreError>;

    async fn load_attempts_page(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
        page: StoreReadPage,
    ) -> Result<StoreHistoryPage<InvocationAttemptRecord>, ExecutionStoreError>;

    async fn load_durable_result(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
        logical_invocation_id: Uuid,
    ) -> Result<Option<DurableCapabilityResult>, ExecutionStoreError>;

    async fn replay_events(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
        page: StoreReadPage,
    ) -> Result<EventReplayPage, ExecutionStoreError>;
}

#[async_trait]
pub trait ExecutionClock: Send + Sync {
    fn now_ms(&self) -> u64;

    async fn wait_until_ms(&self, deadline_ms: u64) {
        let delay_ms = deadline_ms.saturating_sub(self.now_ms());
        super::clock::wait_for_wall_clock_ms(delay_ms).await;
    }
}

#[async_trait]
pub trait ExecutionStoreFactory: Send + Sync {
    type Store: ExecutionStore;

    async fn create_execution_store(&self) -> Result<Self::Store, ExecutionStoreError>;

    fn advance_clock(&self, duration_ms: u64) -> Result<(), ExecutionStoreError>;
}
