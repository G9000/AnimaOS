use anima_memory::{MemoryManager, MemoryPatch, MemoryScope, MemoryType, NewMemory};

use super::test_support::{inject, inject_with, snapshot_diffs, FakeProvider};
use super::*;
use crate::memory_store::{MemoryMutation, MemoryStoreConfig};

fn new_memory(content: &str) -> NewMemory {
    NewMemory {
        agent_id: "agent-1".into(),
        agent_name: "operator".into(),
        memory_type: MemoryType::Fact,
        content: content.into(),
        importance: 0.5,
        tags: None,
        scope: Some(MemoryScope::Private),
        room_id: None,
        world_id: None,
        session_id: None,
    }
}

#[test]
fn unset_empty_or_none_selects_the_none_provider() {
    for value in [None, Some(""), Some("  "), Some("none")] {
        assert_eq!(from_value(value).unwrap().id(), "none");
    }
}

#[test]
fn an_unknown_provider_id_is_a_boot_error_that_lists_the_known_ids() {
    let error = from_value(Some("mem0")).err().unwrap();
    assert_eq!(error, "unknown memory provider 'mem0'; known: none");
}

#[test]
fn installing_none_leaves_the_default_path_without_a_provider() {
    install(from_id("none").unwrap());
    assert!(ACTIVE.get().is_none());
    assert!(active().is_none());
}

#[test]
fn write_events_are_adds_updates_and_removes_by_id_and_content() {
    let mut manager = MemoryManager::new();
    let kept = manager.add(new_memory("kept")).unwrap();
    let edited = manager.add(new_memory("before")).unwrap();
    let removed = manager.add(new_memory("gone")).unwrap();
    let previous = manager.snapshot().memories;
    manager
        .update_memory(
            &edited.id,
            MemoryPatch {
                content: Some("after".into()),
                ..MemoryPatch::default()
            },
        )
        .unwrap();
    manager
        .update_memory(
            &kept.id,
            MemoryPatch {
                importance: Some(0.9),
                ..MemoryPatch::default()
            },
        )
        .unwrap();
    manager.delete_memory(&removed.id).unwrap();
    let added = manager.add(new_memory("fresh")).unwrap();

    let events = memory_write_events(&previous, &manager.snapshot().memories);
    let summary: Vec<_> = events
        .iter()
        .map(|event| {
            (
                event.action,
                event.memory_id.as_str(),
                event.content.as_str(),
            )
        })
        .collect();
    assert_eq!(events.len(), 3, "an importance-only change is not mirrored");
    assert!(summary.contains(&(MemoryWriteAction::Add, added.id.as_str(), "fresh")));
    assert!(summary.contains(&(MemoryWriteAction::Update, edited.id.as_str(), "after")));
    assert!(summary.contains(&(MemoryWriteAction::Remove, removed.id.as_str(), "gone")));
    assert!(events.iter().all(|event| event.agent_id == "agent-1"));
    assert!(events.iter().all(|event| event.memory_type == "fact"));
}

#[tokio::test(flavor = "current_thread")]
async fn a_persisted_add_update_and_delete_are_mirrored_in_the_background() {
    let fake = Arc::new(FakeProvider::new("fake"));
    let _active = inject(fake.clone());
    let mut manager = MemoryManager::new();

    let id = {
        let mut mutation = MemoryMutation::new(&mut manager);
        let id = mutation.add(new_memory("likes tea")).unwrap().id;
        mutation.persist(None).await.unwrap();
        id
    };
    fake.wait_until(|fake| fake.writes.lock().unwrap().len() == 1)
        .await;
    {
        let mut mutation = MemoryMutation::new(&mut manager);
        mutation
            .update_memory(
                &id,
                MemoryPatch {
                    content: Some("likes green tea".into()),
                    ..MemoryPatch::default()
                },
            )
            .unwrap();
        mutation.persist(None).await.unwrap();
    }
    fake.wait_until(|fake| fake.writes.lock().unwrap().len() == 2)
        .await;
    {
        let mut mutation = MemoryMutation::new(&mut manager);
        mutation.delete_memory(&id).unwrap();
        mutation.persist(None).await.unwrap();
    }
    fake.wait_until(|fake| fake.writes.lock().unwrap().len() == 3)
        .await;

    let writes = fake.writes.lock().unwrap();
    let actions: Vec<_> = writes.iter().map(|event| event.action).collect();
    assert_eq!(
        actions,
        [
            MemoryWriteAction::Add,
            MemoryWriteAction::Update,
            MemoryWriteAction::Remove
        ]
    );
    assert_eq!(writes[1].content, "likes green tea");
    assert_eq!(
        writes[2].content, "likes green tea",
        "a remove carries the content it had"
    );
    assert!(writes.iter().all(|event| event.memory_id == id));
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_persist_mirrors_nothing() {
    let fake = Arc::new(FakeProvider::new("fake"));
    let _active = inject(fake.clone());
    let directory = std::env::temp_dir().join(format!("provider-mirror-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    // A directory where the snapshot file belongs makes the save fail.
    let config = MemoryStoreConfig::Json(directory.clone());
    let mut manager = MemoryManager::new();
    {
        let mut mutation = MemoryMutation::new(&mut manager);
        mutation.add(new_memory("never saved")).unwrap();
        assert!(mutation.persist(Some(&config)).await.is_err());
    }
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(fake.writes.lock().unwrap().is_empty());
    assert_eq!(snapshot_diffs(), 0, "a failed persist never diffs");
    assert_eq!(manager.size(), 0, "the failed mutation rolled back");
    std::fs::remove_dir(directory).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn with_no_provider_a_persist_never_snapshots_or_diffs() {
    assert_eq!(snapshot_diffs(), 0);
    let mut manager = MemoryManager::new();
    {
        let mut mutation = MemoryMutation::new(&mut manager);
        mutation.add(new_memory("plain")).unwrap();
        mutation.persist(None).await.unwrap();
    }
    assert_eq!(snapshot_diffs(), 0);
    // The counter does move when a provider is active.
    let fake = Arc::new(FakeProvider::new("fake"));
    let _active = inject(fake.clone());
    {
        let mut mutation = MemoryMutation::new(&mut manager);
        mutation.add(new_memory("mirrored")).unwrap();
        mutation.persist(None).await.unwrap();
    }
    assert_eq!(snapshot_diffs(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn sync_turn_reaches_the_active_provider_and_is_a_no_op_for_none() {
    let turn = TurnRecord {
        agent_id: "agent-1".into(),
        session_id: "session-1".into(),
        user_text: "hello".into(),
        reply_text: "hi".into(),
        at_ms: 7,
    };
    sync_turn(|| unreachable!("none never builds the turn"));
    let fake = Arc::new(FakeProvider::new("fake"));
    let _active = inject(fake.clone());
    sync_turn(|| turn.clone());
    fake.wait_until(|fake| fake.turns.lock().unwrap().len() == 1)
        .await;
    assert_eq!(fake.turns.lock().unwrap()[0], turn);
}

#[tokio::test(flavor = "current_thread")]
async fn status_reports_none_as_ready_and_an_unavailable_reason() {
    let (id, status) = super::status().await;
    assert_eq!((id.as_str(), status), ("none", ProviderStatus::Ready));
    let mut fake = FakeProvider::new("fake");
    fake.unavailable = Some("service unreachable".into());
    let _active = inject(Arc::new(fake));
    let (id, status) = super::status().await;
    assert_eq!(id, "fake");
    assert_eq!(
        status,
        ProviderStatus::Unavailable("service unreachable".into())
    );
}

fn short(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

#[tokio::test(flavor = "current_thread")]
async fn a_provider_that_never_answers_status_is_reported_unavailable() {
    let mut fake = FakeProvider::new("slow");
    fake.status_delay = Some(Duration::from_secs(60));
    let _active = inject_with(Arc::new(fake), 8, short(50), short(50));
    let started = std::time::Instant::now();
    let (id, status) = super::status().await;
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(id, "slow");
    assert_eq!(
        status,
        ProviderStatus::Unavailable("status check timed out".into())
    );
}

#[test]
fn the_production_timeouts_and_queue_size_are_as_specified() {
    assert_eq!(STATUS_TIMEOUT, Duration::from_secs(2));
    assert_eq!(BACKGROUND_CALL_TIMEOUT, Duration::from_secs(10));
    assert_eq!(QUEUE_CAPACITY, 1_024);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_add_then_a_remove_written_back_to_back_arrive_in_order() {
    let fake = Arc::new(FakeProvider::new("fake"));
    let _active = inject(fake.clone());
    let mut manager = MemoryManager::new();
    let rounds = 150;
    for round in 0..rounds {
        let id = {
            let mut mutation = MemoryMutation::new(&mut manager);
            let id = mutation
                .add(new_memory(&format!("memory {round}")))
                .unwrap()
                .id;
            mutation.persist(None).await.unwrap();
            id
        };
        let mut mutation = MemoryMutation::new(&mut manager);
        mutation.delete_memory(&id).unwrap();
        mutation.persist(None).await.unwrap();
    }
    fake.wait_until(|fake| fake.writes.lock().unwrap().len() == rounds * 2)
        .await;
    let writes = fake.writes.lock().unwrap();
    let mut added = std::collections::HashSet::new();
    for event in writes.iter() {
        match event.action {
            MemoryWriteAction::Add => {
                added.insert(event.memory_id.clone());
            }
            MemoryWriteAction::Remove => assert!(
                added.contains(&event.memory_id),
                "a Remove arrived before its Add"
            ),
            MemoryWriteAction::Update => unreachable!(),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_full_queue_drops_the_overflow_and_never_blocks_the_writer() {
    let fake = Arc::new(FakeProvider::new("fake"));
    let active = inject_with(fake.clone(), 2, short(500), short(500));
    let mut manager = MemoryManager::new();
    let memories: Vec<_> = (0..4)
        .map(|index| manager.add(new_memory(&format!("event {index}"))).unwrap())
        .collect();
    // The worker has not run yet (this thread has not yielded): two fit, two
    // are dropped, and the call returns at once.
    mirror_persisted(&[], || memories.clone());
    assert_eq!(active.dropped(), 2);
    fake.wait_until(|fake| fake.writes.lock().unwrap().len() == 2)
        .await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    let contents: Vec<_> = fake
        .writes
        .lock()
        .unwrap()
        .iter()
        .map(|event| event.content.clone())
        .collect();
    assert_eq!(
        contents,
        ["event 0", "event 1"],
        "the first, in order, got through"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_hanging_event_times_out_on_its_own_and_later_events_still_arrive() {
    let mut fake = FakeProvider::new("fake");
    fake.hang_write_content = Some("hangs".into());
    let fake = Arc::new(fake);
    let _active = inject_with(fake.clone(), 8, short(50), short(500));
    let mut manager = MemoryManager::new();
    let memories: Vec<_> = ["hangs", "after", "later"]
        .iter()
        .map(|content| manager.add(new_memory(content)).unwrap())
        .collect();
    mirror_persisted(&[], || memories.clone());
    fake.wait_until(|fake| fake.writes.lock().unwrap().len() == 3)
        .await;
    let contents: Vec<_> = fake
        .writes
        .lock()
        .unwrap()
        .iter()
        .map(|event| event.content.clone())
        .collect();
    assert_eq!(contents, ["hangs", "after", "later"]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_panicking_status_is_reported_unavailable() {
    let mut fake = FakeProvider::new("panicky");
    fake.panics = true;
    let _active = inject(Arc::new(fake));
    let (id, status) = super::status().await;
    assert_eq!(id, "panicky");
    assert_eq!(
        status,
        ProviderStatus::Unavailable("provider panicked".into())
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_panicking_call_is_caught_and_the_worker_keeps_sending() {
    let mut fake = FakeProvider::new("panicky");
    fake.panics = true;
    fake.panic_write_content = Some("panics".into());
    let fake = Arc::new(fake);
    let _active = inject_with(fake.clone(), 8, short(500), short(500));
    sync_turn(|| TurnRecord {
        agent_id: "agent-1".into(),
        session_id: "session-1".into(),
        user_text: "hello".into(),
        reply_text: "hi".into(),
        at_ms: 7,
    });
    let mut manager = MemoryManager::new();
    let memories: Vec<_> = ["panics", "after"]
        .iter()
        .map(|content| manager.add(new_memory(content)).unwrap())
        .collect();
    mirror_persisted(&[], || memories.clone());
    fake.wait_until(|fake| fake.writes.lock().unwrap().len() == 2)
        .await;
    assert_eq!(fake.turns.lock().unwrap().len(), 1);
}
