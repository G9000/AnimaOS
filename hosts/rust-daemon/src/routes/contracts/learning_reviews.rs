use crate::learning::{LearningDecision, LearningReviewRecord};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LearningReviewRequest {}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LearningPolicyRequest {
    pub(crate) auto_enabled: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LearningDecisionRequest {
    pub(crate) decision: LearningDecision,
    pub(crate) expected_version: u64,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LearningUndoRequest {
    pub(crate) expected_version: u64,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct LearningReviewEnvelope {
    pub(crate) review: LearningReviewRecord,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct LearningReviewsEnvelope {
    pub(crate) reviews: Vec<LearningReviewRecord>,
}

pub(crate) use crate::learning::LearningSkillDraftRequest;
