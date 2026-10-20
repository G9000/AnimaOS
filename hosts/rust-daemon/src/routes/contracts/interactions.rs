use crate::clarifications::ClarificationRequest;
use anima_core::interaction::ClarificationAnswer;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SideQuestionInput {
    pub(crate) question: String,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ClarificationAnswerInput {
    pub(crate) revision: u64,
    #[schema(value_type = Vec<Object>)]
    pub(crate) answers: Vec<ClarificationAnswer>,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct ClarificationsEnvelope {
    pub(crate) clarifications: Vec<ClarificationRequest>,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct ClarificationEnvelope {
    pub(crate) clarification: ClarificationRequest,
}
