use super::*;
use crate::schedules::{ScheduleTarget, ScheduleTrigger, ScheduledPromptRecord};
pub(super) fn record() -> ScheduledPromptRecord {
    serde_json::from_value(serde_json::json!({"id":"test-job","agentId":"agent-a","prompt":"secret owner text","trigger":{"interval":{"intervalMs":60000}},"enabled":true,"target":"workspace","nextDueAtMs":60000,"createdAtMs":1,"updatedAtMs":2,"counters":{"runs":3,"failures":3,"consecutiveFailures":3}})).unwrap()
}
#[test]
fn doctor_pause_is_exact_and_undo_refuses_an_intervening_edit() {
    let before = record();
    let (after, journal) = prepare_pause(&before, &fingerprint(&before), 10).unwrap();
    assert!(!after.enabled);
    assert_eq!(after.prompt, before.prompt);
    assert_eq!(after.counters, before.counters);
    assert_eq!(journal.before, before);
    let restored = undo_pause(&after, &journal, 100000).unwrap();
    assert!(restored.enabled);
    assert_eq!(restored.next_due_at_ms, 160000);
    assert_eq!(restored.counters, before.counters);
    let mut changed = after.clone();
    changed.prompt.push('!');
    assert!(undo_pause(&changed, &journal, 100000).is_err());
    assert!(prepare_pause(&before, &fingerprint(&changed), 10).is_err());
}
#[test]
fn doctor_repairs_are_narrow_and_have_durable_bounded_images() {
    for mutate in [0, 1, 2, 3] {
        let mut r = record();
        match mutate {
            0 => r.counters.consecutive_failures = 2,
            1 => r.enabled = false,
            2 => r.trigger = ScheduleTrigger::Once { at_ms: 99 },
            _ => {
                r.target = ScheduleTarget::Connector {
                    connector_id: "c".into(),
                }
            }
        }
        assert!(prepare_pause(&r, &fingerprint(&r), 10).is_err());
    }
    let r = record();
    let (_, journal) = prepare_pause(&r, &fingerprint(&r), 10).unwrap();
    let registry = DoctorRegistry {
        repairs: std::collections::BTreeMap::from([(journal.id.clone(), journal)]),
    };
    let round: DoctorRegistry =
        serde_json::from_slice(&serde_json::to_vec(&registry).unwrap()).unwrap();
    round.validate().unwrap();
    let mut tampered = round;
    tampered
        .repairs
        .values_mut()
        .next()
        .unwrap()
        .after_fingerprint = "bad".into();
    assert!(tampered.validate().is_err());
}
#[tokio::test]
async fn doctor_reports_observations_without_echoing_private_text() {
    let shared = std::sync::Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
    shared
        .write()
        .await
        .schedules
        .insert("test-job".into(), record());
    let snapshot = report(&shared).await;
    let text = serde_json::to_string(&snapshot).unwrap();
    assert!(!text.contains("secret owner text"));
    assert!(!text.contains("agent-a"));
    assert!(snapshot
        .checks
        .iter()
        .any(|c| c.id == "automations" && c.status == CheckStatus::Degraded));
    assert!(snapshot
        .checks
        .iter()
        .any(|c| c.id == "providers" && c.status == CheckStatus::Unprobed));
    assert!(snapshot.checks.iter().all(|c| c.elapsed_ms <= 1000));
    let lock = shared.write().await;
    let busy = report(&shared).await;
    assert!(busy.checks.iter().all(|c| c.status == CheckStatus::Unknown));
    drop(lock);
}

#[tokio::test]
async fn doctor_pause_restart_undo_and_failed_save_preserve_exact_owned_state() {
    use crate::agent_runs::test_support::{coordinator_state, coordinator_with, ScriptedModel};
    use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
    let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
    let state = coordinator_state(&coordinator);
    let svc = coordinator.doctor();
    let file = std::env::temp_dir().join(format!("anima-doctor-{}.json", uuid::Uuid::new_v4()));
    let mut before = record();
    before.agent_id = agent.clone();
    {
        let mut g = state.write().await;
        g.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(file.clone())));
        g.schedules.insert(before.id.clone(), before.clone());
    }
    assert!(svc
        .repair(agent.clone(), before.id.clone(), "0".repeat(64), None)
        .await
        .is_err());
    let repaired = svc
        .repair(agent.clone(), before.id.clone(), fingerprint(&before), None)
        .await
        .unwrap();
    assert!(!state.read().await.schedules[&before.id].enabled);
    let snapshot = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(file.clone()))
        .await
        .unwrap()
        .unwrap();
    let mut restored = crate::state::DaemonState::new();
    restored.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(file.clone())));
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    let fresh = Arc::new(tokio::sync::RwLock::new(restored));
    let service = DoctorService::new(fresh.clone(), Arc::new(Mutex::new(())));
    let undone = service
        .undo(
            agent.clone(),
            repaired.id.clone(),
            repaired.fingerprint.clone(),
            None,
        )
        .await
        .unwrap();
    assert!(undone.undone);
    let current = fresh.read().await.schedules[&before.id].clone();
    assert!(current.enabled);
    assert!(current.next_due_at_ms > anima_core::primitives::now_millis());
    assert_eq!(current.counters, before.counters);
    assert_eq!(current.prompt, before.prompt);
    assert!(service
        .undo(agent.clone(), repaired.id, repaired.fingerprint, None)
        .await
        .is_err());
    let invalid = std::env::temp_dir().join(format!(
        "anima-doctor-save-failure-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&invalid).unwrap();
    {
        fresh
            .write()
            .await
            .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(invalid.clone())));
    }
    assert!(service
        .repair(agent, current.id.clone(), fingerprint(&current), None)
        .await
        .is_err());
    assert_eq!(fresh.read().await.schedules[&current.id], current);
    let _ = std::fs::remove_file(file);
    let _ = std::fs::remove_dir(invalid);
}
#[tokio::test]
async fn doctor_unknown_source_run_cannot_repair_and_derived_index_keeps_canonical_data() {
    use crate::agent_runs::test_support::{coordinator_state, coordinator_with, ScriptedModel};
    let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
    let state = coordinator_state(&coordinator);
    let svc = coordinator.doctor();
    let before = state
        .read()
        .await
        .memory_handle()
        .read()
        .await
        .snapshot()
        .memories;
    let fp = svc.index_candidate().await.unwrap();
    assert!(svc.rebuild_index("0".repeat(64)).await.is_err());
    let result = svc.rebuild_index(fp).await.unwrap();
    assert_eq!(result["canonicalMemoryChanged"], false);
    assert_eq!(result["undoSupported"], false);
    assert_eq!(
        serde_json::to_value(images(&before)).unwrap(),
        serde_json::to_value(images(
            &state
                .read()
                .await
                .memory_handle()
                .read()
                .await
                .snapshot()
                .memories
        ))
        .unwrap()
    );
    let file =
        std::env::temp_dir().join(format!("anima-doctor-auth-{}.json", uuid::Uuid::new_v4()));
    let mut r = record();
    r.agent_id = agent.clone();
    {
        let mut g = state.write().await;
        g.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(file.clone()),
        ));
        g.schedules.insert(r.id.clone(), r.clone());
    }
    let authority = DoctorAuthority {
        link: crate::runs::RunLink {
            agent_id: agent.clone(),
            session_id: "wrong".into(),
            run_id: "missing".into(),
        },
        call: anima_core::ToolCall {
            id: "call".into(),
            name: "doctor_repair".into(),
            args: Default::default(),
        },
        approved: true,
        cancel: None,
    };
    assert!(svc
        .repair(agent, r.id.clone(), fingerprint(&r), Some(authority))
        .await
        .is_err());
    assert_eq!(state.read().await.schedules[&r.id], r);
    assert!(state.read().await.doctor.repairs.is_empty());
    let _ = std::fs::remove_file(file);
}
