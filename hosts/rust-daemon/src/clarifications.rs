//! Durable human input, separate from tool authorization and tool effects.
use anima_core::interaction::{
    normalize_answers, normalize_questions, ClarificationAnswer, ClarificationQuestion,
    ClarificationQuestionInput,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use utoipa::ToSchema;

pub(crate) const MAX_CLARIFICATION_RECORDS: usize = 200;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClarificationStatus {
    Pending,
    Answered,
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClarificationResponse {
    #[schema(value_type = Vec<Object>)]
    pub(crate) answers: Vec<ClarificationAnswer>,
    pub(crate) answered_at_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClarificationRequest {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    pub(crate) run_id: String,
    pub(crate) tool_call_id: String,
    #[schema(value_type = Vec<Object>)]
    pub(crate) questions: Vec<ClarificationQuestion>,
    pub(crate) revision: u64,
    pub(crate) created_at_ms: u64,
    pub(crate) status: ClarificationStatus,
    pub(crate) response: Option<ClarificationResponse>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ClarificationError {
    NotFound,
    Conflict(&'static str),
    Invalid(String),
}
#[derive(Clone, Default)]
pub(crate) struct ClarificationRegistry {
    records: BTreeMap<String, ClarificationRequest>,
}
impl ClarificationRegistry {
    pub(crate) fn snapshot(&self) -> Vec<ClarificationRequest> {
        self.records.values().cloned().collect()
    }
    pub(crate) fn restored(
        records: Vec<ClarificationRequest>,
        live_agents: &HashSet<String>,
    ) -> Self {
        Self {
            records: records
                .into_iter()
                .filter(|request| {
                    live_agents.contains(&request.agent_id)
                        && Self::validate(std::slice::from_ref(request)).is_ok()
                })
                .take(MAX_CLARIFICATION_RECORDS)
                .map(|request| (request.id.clone(), request))
                .collect(),
        }
    }
    pub(crate) fn retain(&mut self, predicate: impl Fn(&ClarificationRequest) -> bool) {
        self.records.retain(|_, request| predicate(request));
    }
    pub(crate) fn get(&self, id: &str) -> Option<&ClarificationRequest> {
        self.records.get(id)
    }
    pub(crate) fn remove(&mut self, id: &str) {
        self.records.remove(id);
    }
    pub(crate) fn put(&mut self, request: ClarificationRequest) {
        self.records.insert(request.id.clone(), request);
    }
    pub(crate) fn for_session(&self, agent: &str, session: &str) -> Vec<ClarificationRequest> {
        let mut records: Vec<_> = self
            .records
            .values()
            .filter(|r| r.agent_id == agent && r.session_id == session)
            .cloned()
            .collect();
        records.sort_by_key(|r| (r.created_at_ms, r.id.clone()));
        records
    }
    pub(crate) fn answered_for_run(&self, run: &str) -> Vec<ClarificationRequest> {
        self.records
            .values()
            .filter(|r| r.run_id == run && r.status == ClarificationStatus::Answered)
            .cloned()
            .collect()
    }
    pub(crate) fn cancel_run(&mut self, run: &str, _now_ms: u64) {
        for record in self
            .records
            .values_mut()
            .filter(|r| r.run_id == run && r.status == ClarificationStatus::Pending)
        {
            record.status = ClarificationStatus::Cancelled;
            record.revision += 1;
        }
    }
    pub(crate) fn open(
        &mut self,
        agent: &str,
        session: &str,
        run: &str,
        call: &str,
        questions: Vec<ClarificationQuestion>,
        now_ms: u64,
    ) -> Result<(ClarificationRequest, bool), ClarificationError> {
        if let Some(record) = self
            .records
            .values()
            .find(|r| r.agent_id == agent && r.run_id == run && r.tool_call_id == call)
        {
            return if record.session_id == session && record.questions == questions {
                Ok((record.clone(), false))
            } else {
                Err(ClarificationError::Conflict(
                    "This question identity was already used",
                ))
            };
        }
        if self.records.len() >= MAX_CLARIFICATION_RECORDS {
            return Err(ClarificationError::Conflict(
                "The saved question limit was reached",
            ));
        }
        if self
            .records
            .values()
            .filter(|r| r.agent_id == agent && r.status == ClarificationStatus::Pending)
            .count()
            >= 32
        {
            return Err(ClarificationError::Conflict(
                "This companion already has 32 questions waiting",
            ));
        }
        let record = ClarificationRequest {
            id: format!("ask_{}", uuid::Uuid::new_v4()),
            agent_id: agent.into(),
            session_id: session.into(),
            run_id: run.into(),
            tool_call_id: call.into(),
            questions,
            revision: 1,
            created_at_ms: now_ms,
            status: ClarificationStatus::Pending,
            response: None,
        };
        Self::validate(std::slice::from_ref(&record)).map_err(ClarificationError::Invalid)?;
        self.put(record.clone());
        Ok((record, true))
    }
    pub(crate) fn answer(
        &mut self,
        id: &str,
        revision: u64,
        answers: Vec<ClarificationAnswer>,
        now_ms: u64,
    ) -> Result<ClarificationRequest, ClarificationError> {
        let record = self
            .records
            .get_mut(id)
            .ok_or(ClarificationError::NotFound)?;
        let answers =
            normalize_answers(&record.questions, answers).map_err(ClarificationError::Invalid)?;
        if record.status == ClarificationStatus::Answered
            && revision.checked_add(1) == Some(record.revision)
            && record
                .response
                .as_ref()
                .is_some_and(|response| response.answers == answers)
        {
            return Ok(record.clone());
        }
        if record.status != ClarificationStatus::Pending || record.revision != revision {
            return Err(ClarificationError::Conflict(
                "This question changed; reload it before answering",
            ));
        }
        record.response = Some(ClarificationResponse {
            answers,
            answered_at_ms: now_ms.max(record.created_at_ms),
        });
        record.status = ClarificationStatus::Answered;
        record.revision += 1;
        Ok(record.clone())
    }
    pub(crate) fn validate(records: &[ClarificationRequest]) -> Result<(), String> {
        if records.len() > MAX_CLARIFICATION_RECORDS {
            return Err("Too many saved clarification requests".into());
        }
        let mut ids = HashSet::new();
        let mut calls = HashSet::new();
        for record in records {
            let valid_id =
                |id: &str| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control);
            if [
                &record.id,
                &record.agent_id,
                &record.session_id,
                &record.run_id,
                &record.tool_call_id,
            ]
            .iter()
            .any(|id| !valid_id(id))
                || !ids.insert(&record.id)
                || !calls.insert((&record.agent_id, &record.run_id, &record.tool_call_id))
            {
                return Err("Invalid or duplicate clarification request identity".into());
            }
            let input = record
                .questions
                .iter()
                .map(|question| ClarificationQuestionInput {
                    question: question.question.clone(),
                    choices: question
                        .choices
                        .iter()
                        .map(|choice| choice.label.clone())
                        .collect(),
                    multi_select: question.multi_select,
                })
                .collect();
            if normalize_questions(input)? != record.questions {
                return Err("Saved question IDs or labels are invalid".into());
            }
            match (&record.status, &record.response, record.revision) {
                (ClarificationStatus::Pending, None, 1)
                | (ClarificationStatus::Cancelled, None, 2) => {}
                (ClarificationStatus::Answered, Some(response), 2)
                    if response.answered_at_ms >= record.created_at_ms =>
                {
                    if normalize_answers(&record.questions, response.answers.clone())?
                        != response.answers
                    {
                        return Err("Saved answers are not canonical".into());
                    }
                }
                _ => return Err("Inconsistent saved clarification outcome".into()),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anima_core::interaction::{normalize_questions, ClarificationQuestionInput};
    fn questions() -> Vec<ClarificationQuestion> {
        normalize_questions(vec![ClarificationQuestionInput {
            question: "Format?".into(),
            choices: vec!["Markdown".into(), "PDF".into()],
            multi_select: false,
        }])
        .unwrap()
    }
    #[test]
    fn interaction_clarification_identity_and_versioned_answers_are_idempotent() {
        let mut registry = ClarificationRegistry::default();
        let (request, created) = registry.open("a", "s", "r", "tc", questions(), 1).unwrap();
        assert!(created);
        let (retry, created) = registry.open("a", "s", "r", "tc", questions(), 2).unwrap();
        assert!(!created);
        assert_eq!(request, retry);
        let answer = vec![ClarificationAnswer {
            question_id: "q0".into(),
            choice_ids: vec!["c1".into()],
            text: None,
        }];
        let answered = registry.answer(&request.id, 1, answer.clone(), 3).unwrap();
        assert_eq!(answered.revision, 2);
        assert_eq!(answered.status, ClarificationStatus::Answered);
        assert_eq!(
            registry.answer(&request.id, 1, answer, 4).unwrap(),
            answered
        );
        assert!(registry.answer(&request.id, 1, vec![], 4).is_err());
        assert!(registry.answer(&request.id, 7, vec![], 4).is_err());
    }
    #[test]
    fn interaction_clarifications_survive_restart_without_expiring_and_stop_cancels() {
        let mut registry = ClarificationRegistry::default();
        let (pending, _) = registry.open("a", "s", "r", "tc", questions(), 1).unwrap();
        let encoded = serde_json::to_vec(&registry.snapshot()).unwrap();
        let records = serde_json::from_slice(&encoded).unwrap();
        let mut restored = ClarificationRegistry::restored(records, &HashSet::from(["a".into()]));
        assert_eq!(
            restored.get(&pending.id).unwrap().status,
            ClarificationStatus::Pending
        );
        restored.cancel_run("r", 2);
        assert_eq!(
            restored.get(&pending.id).unwrap().status,
            ClarificationStatus::Cancelled
        );
        assert!(restored.answer(&pending.id, 1, vec![], 3).is_err());
        assert!(
            ClarificationRegistry::restored(registry.snapshot(), &HashSet::new())
                .snapshot()
                .is_empty()
        );
    }
}
