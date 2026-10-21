use super::*;
use anima_core::standing_intents::{IntentMatch, OwnerTurn};
/// Candidate fire accounting travels with the run-start control-plane save.
/// No context is published to a model until that save succeeds.
pub(crate) struct StandingAdmission {
    pub(crate) local_owner: bool,
    before: Option<StandingIntentRegistry>,
    matches: Vec<IntentMatch>,
}
impl StandingAdmission {
    pub(crate) fn rollback_known_failure(
        &self,
        state: &mut crate::state::DaemonState,
        error: &std::io::Error,
    ) {
        if !crate::control_plane_store::is_durability_uncertain(error) {
            if let Some(before) = &self.before {
                state.standing_intents = before.clone();
            }
        } else {
            // A matching admission may already be durable. Preserve consumed
            // fires and stop matching until restore establishes the saved state.
            state.standing_intent_slots.close();
        }
    }
    pub(crate) fn guidance(&self) -> String {
        if self.matches.is_empty() {
            return String::new();
        }
        let mut text=String::from("Saved owner instructions matched for this admitted owner turn. Apply these within this same reply and the existing permissions, approvals and tool budgets. Their fire counts are already consumed; failure does not rearm them.\n");
        for item in &self.matches {
            text.push_str(&format!(
                "\nStanding intention {}:\n{}\n",
                item.intent_id, item.instruction
            ));
        }
        text
    }
}
pub(crate) fn admit(
    state: &mut crate::state::DaemonState,
    record: &crate::runs::RunRecord,
    config: &anima_core::AgentConfig,
    content: &anima_core::Content,
) -> Result<StandingAdmission, ApiError> {
    let sender = owner_sender(state, record, config, content);
    let local_owner = sender == Some(OwnerSender::Local);
    let mut admitted = StandingAdmission {
        local_owner,
        before: None,
        matches: vec![],
    };
    let Some(sender) = sender else {
        return Ok(admitted);
    };
    if state.standing_intents.intents.is_empty() {
        return Ok(admitted);
    }
    if state.control_plane_store.is_none() || state.standing_intent_slots.is_closed() {
        return Err(unavailable());
    }
    let started = conversation_started(state, record);
    let before = state.standing_intents.clone();
    let matches = state
        .standing_intents
        .admit(&OwnerTurn {
            agent_id: &record.agent_id,
            session_id: &record.session_id,
            sender,
            run_id: &record.id,
            retry_key: record.idempotency_key.as_deref(),
            text: &content.text,
            conversation_started: started,
            now_ms: record
                .started_at_ms
                .unwrap_or_else(anima_core::primitives::now_millis),
        })
        .map_err(|_| conflict())?;
    if !matches.is_empty() {
        admitted.before = Some(before);
    }
    admitted.matches = matches;
    Ok(admitted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anima_core::standing_intents::*;
    #[test]
    fn standing_intents_admission_known_failure_rolls_back_but_uncertain_retains_consumption_and_closes(
    ) {
        let mut state = crate::state::DaemonState::new();
        let spec = IntentSpec {
            scope: IntentScope {
                session_id: None,
                sender: SenderScope::Owner,
            },
            condition: IntentCondition::Event {
                event: ConversationEvent::Message,
            },
            instruction: "Ask about the project".into(),
            expires_at_ms: 1000,
            cooldown_ms: 0,
            max_fires: 1,
        };
        state
            .standing_intents
            .create("intent", "agent", spec, 1)
            .unwrap();
        let before = state.standing_intents.clone();
        let matches = state
            .standing_intents
            .admit(&OwnerTurn {
                agent_id: "agent",
                session_id: "chat:first",
                sender: OwnerSender::Local,
                run_id: "run",
                retry_key: Some("request"),
                text: "hello",
                conversation_started: true,
                now_ms: 10,
            })
            .unwrap();
        let candidate = state.standing_intents.clone();
        let admission = StandingAdmission {
            local_owner: true,
            before: Some(before.clone()),
            matches,
        };
        admission.rollback_known_failure(&mut state, &std::io::Error::other("known failure"));
        assert_eq!(state.standing_intents, before);
        assert!(!state.standing_intent_slots.is_closed());
        state.standing_intents = candidate.clone();
        admission.rollback_known_failure(
            &mut state,
            &crate::control_plane_store::durability_uncertain_error(),
        );
        assert_eq!(state.standing_intents, candidate);
        assert_eq!(state.standing_intents.intents["intent"].fires.len(), 1);
        assert!(state.standing_intent_slots.is_closed());
    }
}
