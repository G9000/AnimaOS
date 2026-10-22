//! Durable owner/native events. The SSE broadcast is deliberately not a queue.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{ScheduleError, ScheduleTrigger, ScheduledPromptRecord};

pub(crate) const MAX_EVENT_PAYLOAD_BYTES: usize = 8192;
pub(crate) const MAX_EVENT_OCCURRENCES: usize = 256;
pub(crate) const MAX_PENDING_EVENTS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EventOccurrence {
    pub(crate) id: String,
    pub(crate) event_id: String,
    pub(crate) agent_id: String,
    pub(crate) schedule_id: String,
    pub(crate) topic: String,
    pub(crate) payload: Value,
    pub(crate) at_ms: u64,
    pub(crate) claimed: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EventRegistry {
    occurrences: Vec<EventOccurrence>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EventAdmission {
    pub(crate) accepted: usize,
    pub(crate) duplicate: usize,
}

pub(crate) fn valid_topic(topic: &str) -> bool {
    !topic.is_empty()
        && topic.len() <= 80
        && topic.starts_with(|c: char| c.is_ascii_lowercase())
        && topic
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
}

fn valid_event_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-:".contains(&c))
}

fn validate_payload(payload: &Value) -> Result<(), ScheduleError> {
    if !payload.is_object()
        || serde_json::to_vec(payload).map_or(true, |v| v.len() > MAX_EVENT_PAYLOAD_BYTES)
    {
        return Err(ScheduleError::Invalid(
            "Event payload must be an object of at most 8192 bytes",
        ));
    }
    let mut pending = vec![(payload, 0)];
    let mut nodes = 0;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if nodes > 256 || depth > 8 {
            return Err(ScheduleError::Invalid(
                "Event payload exceeds its structure limit",
            ));
        }
        match value {
            Value::Array(items) => pending.extend(items.iter().map(|v| (v, depth + 1))),
            Value::Object(items) => pending.extend(items.values().map(|v| (v, depth + 1))),
            _ => {}
        }
    }
    Ok(())
}

fn occurrence_id(schedule_id: &str, event_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update((schedule_id.len() as u64).to_be_bytes());
    digest.update(schedule_id.as_bytes());
    digest.update(event_id.as_bytes());
    format!("event:{}", hex::encode(digest.finalize()))
}

fn stale(item: &EventOccurrence, schedules: &HashMap<String, ScheduledPromptRecord>) -> bool {
    !schedules.get(&item.schedule_id).is_some_and(|s| {
        s.agent_id == item.agent_id
            && matches!(&s.trigger, ScheduleTrigger::Event { topic } if topic == &item.topic)
    })
}

impl EventRegistry {
    pub(crate) fn snapshot(&self) -> Vec<EventOccurrence> {
        self.occurrences.clone()
    }

    pub(crate) fn restore(
        mut occurrences: Vec<EventOccurrence>,
        schedules: &HashMap<String, ScheduledPromptRecord>,
    ) -> Result<Self, String> {
        if occurrences.len() > MAX_EVENT_OCCURRENCES {
            return Err("Too many stored automation events".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for item in &mut occurrences {
            if !valid_topic(&item.topic)
                || !valid_event_id(&item.event_id)
                || item.id != occurrence_id(&item.schedule_id, &item.event_id)
                || item.at_ms == 0
                || item.agent_id.trim().is_empty()
                || item.schedule_id.trim().is_empty()
                || !ids.insert(item.id.clone())
                || validate_payload(&item.payload).is_err()
                || schedules
                    .get(&item.schedule_id)
                    .is_some_and(|s| s.agent_id != item.agent_id)
            {
                return Err("Invalid stored automation event".into());
            }
            if stale(item, schedules) {
                item.claimed = true;
            }
        }
        occurrences.retain(|item| schedules.contains_key(&item.schedule_id));
        if occurrences.iter().filter(|v| !v.claimed).count() > MAX_PENDING_EVENTS {
            return Err("Too many pending automation events".into());
        }
        Ok(Self { occurrences })
    }

    /// The caller holds the control-plane transaction and saves/undoes this with it.
    pub(crate) fn enqueue(
        &mut self,
        schedules: &HashMap<String, ScheduledPromptRecord>,
        agent_id: &str,
        event_id: &str,
        topic: &str,
        payload: Value,
        at_ms: u64,
    ) -> Result<EventAdmission, ScheduleError> {
        let mut next = self.clone();
        let admission = next.enqueue_inner(schedules, agent_id, event_id, topic, payload, at_ms)?;
        *self = next;
        Ok(admission)
    }
    fn enqueue_inner(
        &mut self,
        schedules: &HashMap<String, ScheduledPromptRecord>,
        agent_id: &str,
        event_id: &str,
        topic: &str,
        payload: Value,
        at_ms: u64,
    ) -> Result<EventAdmission, ScheduleError> {
        if !valid_topic(topic) || !valid_event_id(event_id) || at_ms == 0 {
            return Err(ScheduleError::Invalid("Event topic or eventId is invalid"));
        }
        validate_payload(&payload)?;
        // Reusing a known key for different source bytes is a conflict, not a second effect.
        if self.occurrences.iter().any(|v| {
            v.agent_id == agent_id
                && v.event_id == event_id
                && (v.topic != topic || v.payload != payload)
        }) {
            return Err(ScheduleError::Conflict(
                "This eventId already names a different event",
            ));
        }
        self.occurrences
            .retain(|item| schedules.contains_key(&item.schedule_id));
        for item in &mut self.occurrences {
            if stale(item, schedules) {
                item.claimed = true;
            }
        }
        let mut admission = EventAdmission::default();
        let mut added = Vec::new();
        for schedule in schedules.values().filter(|s| s.agent_id == agent_id && matches!(&s.trigger, ScheduleTrigger::Event { topic: expected } if expected == topic)) {
            let id = occurrence_id(&schedule.id, event_id);
            if self.occurrences.iter().any(|v| v.id == id) { admission.duplicate += 1; continue; }
            if !schedule.enabled { continue; }
            added.push(EventOccurrence { id, event_id: event_id.into(), agent_id: agent_id.into(), schedule_id: schedule.id.clone(), topic: topic.into(), payload: payload.clone(), at_ms, claimed: false });
        }
        if self.occurrences.iter().filter(|v| !v.claimed).count() + added.len() > MAX_PENDING_EVENTS
        {
            return Err(ScheduleError::Busy("Automation event queue is full"));
        }
        // A live schedule's accepted identities never disappear: refusing new
        // capacity cannot turn a delayed webhook retry into another effect.
        if self.occurrences.len().saturating_add(added.len()) > MAX_EVENT_OCCURRENCES {
            return Err(ScheduleError::Busy("Automation event history is full; delete an event automation to release its retained identities"));
        }
        admission.accepted = added.len();
        self.occurrences.extend(added);
        Ok(admission)
    }

    #[cfg(test)]
    pub(crate) fn pending(&self, schedule_id: &str) -> Option<&EventOccurrence> {
        self.occurrences
            .iter()
            .find(|v| v.schedule_id == schedule_id && !v.claimed)
    }
    pub(crate) fn due(
        &self,
        schedule: &ScheduledPromptRecord,
        now: u64,
    ) -> Option<&EventOccurrence> {
        let ScheduleTrigger::Event { topic } = &schedule.trigger else {
            return None;
        };
        if !schedule.enabled || super::unresolved_occurrence(schedule) {
            return None;
        }
        if matches!(schedule.created_by, super::AutomationCreator::Agent { .. })
            && schedule
                .last_fired
                .as_ref()
                .is_some_and(|f| now < f.fired_at_ms.saturating_add(300_000))
        {
            return None;
        }
        self.occurrences.iter().find(|v| {
            v.schedule_id == schedule.id && !v.claimed && v.topic == *topic && v.at_ms <= now
        })
    }
    pub(crate) fn discard_pending(&mut self, schedule_id: &str) {
        for item in self
            .occurrences
            .iter_mut()
            .filter(|v| v.schedule_id == schedule_id)
        {
            item.claimed = true;
        }
    }
    pub(crate) fn remove_schedule(&mut self, schedule_id: &str) {
        self.occurrences.retain(|v| v.schedule_id != schedule_id);
    }
    pub(crate) fn claim(&mut self, id: &str) -> Option<EventOccurrence> {
        let item = self
            .occurrences
            .iter_mut()
            .find(|v| v.id == id && !v.claimed)?;
        item.claimed = true;
        Some(item.clone())
    }
    pub(crate) fn get(&self, id: &str) -> Option<&EventOccurrence> {
        self.occurrences.iter().find(|v| v.id == id)
    }
}

/// Invoked inside the source run's durable commit. Schedule runs never emit recursively.
pub(crate) fn enqueue_native(
    state: &mut crate::state::DaemonState,
    run: &crate::runs::RunRecord,
) -> Result<EventAdmission, ScheduleError> {
    use crate::runs::{RunSource, RunStatus};
    if run.parent_run_id.is_some()
        || (run.system_followup == Some(true)
            && !(run.recovery_of.is_some()
                && matches!(run.source, RunSource::Web | RunSource::Api)))
        || matches!(run.source, RunSource::Schedule | RunSource::Delegation)
    {
        return Ok(EventAdmission::default());
    }
    let topic = match run.status {
        RunStatus::Completed => "task.completed",
        RunStatus::Failed => "task.failed",
        _ => return Ok(EventAdmission::default()),
    };
    let payload = serde_json::json!({ "runId": run.id, "sessionId": run.session_id, "status": run.status.as_str() });
    state.automation_events.enqueue(
        &state.schedules,
        &run.agent_id,
        &format!("native:{}", run.id),
        topic,
        payload,
        run.finished_at_ms.unwrap_or(run.created_at_ms),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn schedules() -> HashMap<String, ScheduledPromptRecord> {
        let mut record = super::super::test_automation("a", "s");
        record.trigger = ScheduleTrigger::Event {
            topic: "files.changed".into(),
        };
        HashMap::from([("s".into(), record)])
    }
    #[test]
    fn event_retries_survive_restore_and_changed_bytes_conflict() {
        let schedules = schedules();
        let mut events = EventRegistry::default();
        let payload = serde_json::json!({"path":"notes.txt"});
        assert_eq!(
            events
                .enqueue(
                    &schedules,
                    "a",
                    "owner:e1",
                    "files.changed",
                    payload.clone(),
                    1
                )
                .unwrap()
                .accepted,
            1
        );
        let mut restored = EventRegistry::restore(events.snapshot(), &schedules).unwrap();
        assert_eq!(
            restored
                .enqueue(&schedules, "a", "owner:e1", "files.changed", payload, 2)
                .unwrap()
                .duplicate,
            1
        );
        assert!(restored
            .enqueue(
                &schedules,
                "a",
                "owner:e1",
                "files.changed",
                serde_json::json!({"path":"other"}),
                2
            )
            .is_err());
        let id = restored.pending("s").unwrap().id.clone();
        assert!(restored.claim(&id).is_some());
        assert!(restored.claim(&id).is_none());
        assert!(EventRegistry::restore(restored.snapshot(), &schedules)
            .unwrap()
            .pending("s")
            .is_none());
    }
    #[test]
    fn event_payload_scope_and_queue_are_bounded_without_partial_admission() {
        let schedules = schedules();
        let mut events = EventRegistry::default();
        assert_eq!(
            events
                .enqueue(
                    &schedules,
                    "other",
                    "e",
                    "files.changed",
                    serde_json::json!({}),
                    1
                )
                .unwrap()
                .accepted,
            0
        );
        assert!(events
            .enqueue(
                &schedules,
                "a",
                "e",
                "files.changed",
                serde_json::json!({"v":"x".repeat(MAX_EVENT_PAYLOAD_BYTES)}),
                1
            )
            .is_err());
        for n in 0..MAX_PENDING_EVENTS {
            events
                .enqueue(
                    &schedules,
                    "a",
                    &format!("e{n}"),
                    "files.changed",
                    serde_json::json!({}),
                    1,
                )
                .unwrap();
        }
        let before = events.snapshot();
        assert!(events
            .enqueue(
                &schedules,
                "a",
                "overflow",
                "files.changed",
                serde_json::json!({}),
                1
            )
            .is_err());
        assert_eq!(events.snapshot(), before);
    }

    #[test]
    fn full_history_never_forgets_a_live_schedules_earliest_webhook_identity() {
        let schedules = schedules();
        let mut events = EventRegistry::default();
        for n in 0..MAX_EVENT_OCCURRENCES {
            events
                .enqueue(
                    &schedules,
                    "a",
                    &format!("owner:e{n}"),
                    "files.changed",
                    serde_json::json!({}),
                    1,
                )
                .unwrap();
            let id = events.pending("s").unwrap().id.clone();
            events.claim(&id).unwrap();
        }
        assert!(events
            .enqueue(
                &schedules,
                "a",
                "owner:overflow",
                "files.changed",
                serde_json::json!({}),
                2
            )
            .is_err());
        let mut events = EventRegistry::restore(events.snapshot(), &schedules).unwrap();
        assert_eq!(
            events
                .enqueue(
                    &schedules,
                    "a",
                    "owner:e0",
                    "files.changed",
                    serde_json::json!({}),
                    3
                )
                .unwrap()
                .duplicate,
            1
        );
        assert!(events.pending("s").is_none());
    }

    #[test]
    fn native_outcomes_exclude_system_followups_and_capacity_preserves_the_source() {
        use crate::runs::{RunRecord, RunSource, RunStart, RunStatus};
        let mut state = crate::state::DaemonState::new();
        let mut schedule = super::super::test_automation("a", "s");
        schedule.trigger = ScheduleTrigger::Event {
            topic: "task.completed".into(),
        };
        state.schedules.insert("s".into(), schedule);
        let mut run = RunRecord::running(
            RunStart {
                agent_id: "a".into(),
                session_id: "chat:source".into(),
                source: RunSource::Web,
                source_ref: None,
                idempotency_key: None,
                text: "done".into(),
                model: "synthetic".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        run.status = RunStatus::Completed;
        run.finished_at_ms = Some(2);
        run.system_followup = Some(true);
        assert_eq!(enqueue_native(&mut state, &run).unwrap().accepted, 0);
        run.recovery_of = Some("reviewed-source".into());
        run.source = RunSource::Schedule;
        assert_eq!(enqueue_native(&mut state, &run).unwrap().accepted, 0);
        run.source = RunSource::Web;
        assert_eq!(enqueue_native(&mut state, &run).unwrap().accepted, 1);
        run.recovery_of = None;
        run.system_followup = Some(false);
        assert_eq!(enqueue_native(&mut state, &run).unwrap().duplicate, 1);
        let first_id = state.automation_events.pending("s").unwrap().id.clone();
        state.automation_events.claim(&first_id).unwrap();
        for n in 1..MAX_EVENT_OCCURRENCES {
            state
                .automation_events
                .enqueue(
                    &state.schedules,
                    "a",
                    &format!("owner:{n}"),
                    "task.completed",
                    serde_json::json!({}),
                    2,
                )
                .unwrap();
            let id = state.automation_events.pending("s").unwrap().id.clone();
            state.automation_events.claim(&id).unwrap();
        }
        let before = state.automation_events.snapshot();
        let source = serde_json::to_value(&run).unwrap();
        run.id = "overflow-source".into();
        assert!(enqueue_native(&mut state, &run).is_err());
        assert_eq!(state.automation_events.snapshot(), before);
        assert_eq!(run.status, RunStatus::Completed);
        run.id = source["id"].as_str().unwrap().into();
        assert_eq!(serde_json::to_value(run).unwrap(), source);
    }

    #[tokio::test]
    async fn event_scheduler_executes_one_durable_occurrence_and_never_emits_recursive_outcomes() {
        use crate::agent_runs::test_support::{ScriptedModel, Step};
        use crate::control_plane_store::ControlPlaneStoreConfig;
        use crate::schedules::{AutomationInput, ScheduleTarget};
        use crate::state::DaemonState;
        let model = ScriptedModel::new(vec![Step::Text(vec!["Event handled."])]);
        let (service, state, agent, _) = super::super::tests::service_with_daemon(
            DaemonState::with_model_adapter(model.clone()),
        );
        let directory = tempfile::tempdir().unwrap();
        state
            .write()
            .await
            .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
                directory.path().join("state.json"),
            )));
        let schedule = service
            .automations()
            .create(
                AutomationInput::owner(
                    agent.clone(),
                    "Summarize the file event".into(),
                    ScheduleTrigger::Event {
                        topic: "files.changed".into(),
                    },
                    ScheduleTarget::Workspace,
                ),
                10_000,
            )
            .await
            .unwrap()
            .0;
        let automations = service.automations();
        let emit = || {
            automations.emit_owner_event(
                agent.clone(),
                "file-1".into(),
                "files.changed".into(),
                serde_json::json!({"path":"report.txt","note":"source content only"}),
                10_001,
            )
        };
        assert_eq!(emit().await.unwrap().accepted, 1);
        assert_eq!(emit().await.unwrap().duplicate, 1);
        assert_eq!(service.tick_at(10_002).await.unwrap(), 1);
        service.drain().await;
        assert_eq!(service.tick_at(10_003).await.unwrap(), 0);
        let mut guard = state.write().await;
        assert_eq!(guard.schedules[&schedule.id].counters.runs, 1);
        let run = guard
            .runs
            .for_agent(&agent)
            .into_iter()
            .find(|run| run.source == crate::runs::RunSource::Schedule)
            .cloned()
            .unwrap();
        assert!(run.input.text.contains("untrusted source data"));
        assert!(run.input.text.contains("report.txt"));
        assert_eq!(enqueue_native(&mut guard, &run).unwrap().accepted, 0);
        let restored =
            EventRegistry::restore(guard.automation_events.snapshot(), &guard.schedules).unwrap();
        assert!(restored.pending(&schedule.id).is_none());
        assert_eq!(model.requests().len(), 1);
    }

    #[tokio::test]
    async fn owner_event_save_failure_restores_the_queue() {
        use crate::control_plane_store::ControlPlaneStoreConfig;
        let (service, state, agent, _) = super::super::tests::service();
        let mut schedule = super::super::test_automation(&agent, "s");
        schedule.trigger = ScheduleTrigger::Event {
            topic: "files.changed".into(),
        };
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("not-a-directory");
        std::fs::write(&blocker, b"blocker").unwrap();
        {
            let mut guard = state.write().await;
            guard.schedules.insert(schedule.id.clone(), schedule);
            guard.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
                blocker.join("state.json"),
            )));
        }
        assert!(matches!(
            service
                .automations()
                .emit_owner_event(
                    agent,
                    "e1".into(),
                    "files.changed".into(),
                    serde_json::json!({}),
                    10_001
                )
                .await,
            Err(ScheduleError::Persistence)
        ));
        assert!(state.read().await.automation_events.snapshot().is_empty());
    }
}
