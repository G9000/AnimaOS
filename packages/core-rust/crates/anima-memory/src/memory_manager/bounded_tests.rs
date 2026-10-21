use super::*;
#[test]
fn bounded_memories_checks_record_and_metadata_bytes_before_cloning() {
    let mut manager = MemoryManager::new();
    let memory = manager
        .add(NewMemory {
            agent_id: "owner".into(),
            agent_name: "fixture".into(),
            memory_type: MemoryType::Fact,
            content: "short canonical fact".into(),
            importance: 0.5,
            tags: Some(vec!["m".repeat(2048)]),
            scope: Some(MemoryScope::Private),
            room_id: None,
            world_id: None,
            session_id: None,
        })
        .unwrap();
    assert!(manager.bounded_memories(0, 8192).is_none());
    assert!(
        manager.bounded_memories(1, 1024).is_none(),
        "metadata must count even when content is small"
    );
    let rows = manager.bounded_memories(1, 8192).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, memory.id);
    assert_eq!(rows[0].content, memory.content);
    assert_eq!(manager.snapshot().memories.len(), 1);
}

#[test]
fn bounded_memories_charges_empty_metadata_entries_before_iterating_or_cloning() {
    let mut manager = MemoryManager::new();
    manager
        .add(NewMemory {
            agent_id: "owner".into(),
            agent_name: "fixture".into(),
            memory_type: MemoryType::Fact,
            content: "short fact".into(),
            importance: 0.5,
            tags: Some(vec![String::new(); 10000]),
            scope: Some(MemoryScope::Private),
            room_id: None,
            world_id: None,
            session_id: None,
        })
        .unwrap();
    assert!(
        manager.bounded_memories(1, 1024).is_none(),
        "empty metadata still occupies memory and iteration work"
    );
}
