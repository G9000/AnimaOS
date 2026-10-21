use super::{
    Memory, MemoryConsolidationError, MemoryConsolidationSource, MemoryConsolidationState,
    MemoryManager, MemoryScope, MemorySearchOptions, MemoryType, NewAgentRelationship, NewMemory,
    NewTemporalFact, NewTemporalRelationship, PreparedMemoryConsolidation,
    RelationshipEndpointKind,
};

const GROUP: &str = "cg:0123456789abcdef0123456789abcdef";
const SUMMARY_ID: &str = "maintenance-summary-123";

fn fixture() -> (MemoryManager, Vec<MemoryConsolidationSource>) {
    let mut manager = MemoryManager::new();
    let sources = ["originalalpha likes mint", "originalbeta likes tea"]
        .into_iter()
        .map(|content| {
            let memory = manager
                .add(NewMemory {
                    agent_id: "owner".into(),
                    agent_name: "Companion".into(),
                    memory_type: MemoryType::Fact,
                    content: content.into(),
                    importance: 0.6,
                    tags: None,
                    scope: Some(MemoryScope::Private),
                    room_id: None,
                    world_id: None,
                    session_id: None,
                })
                .unwrap();
            MemoryConsolidationSource {
                id: memory.id,
                expected_content: memory.content,
            }
        })
        .collect();
    (manager, sources)
}

fn prepare(
    manager: &MemoryManager,
    sources: &[MemoryConsolidationSource],
) -> PreparedMemoryConsolidation {
    manager
        .prepare_fact_consolidation(
            "owner",
            None,
            sources,
            "  summarygamma prefers mint tea  ",
            GROUP,
            SUMMARY_ID,
            |_| true,
        )
        .unwrap()
}

#[test]
fn reversible_prepare_is_read_only_and_records_exact_canonical_images() {
    let (manager, sources) = fixture();
    let before = manager.snapshot();
    let prepared = prepare(&manager, &sources);

    assert_eq!(manager.snapshot(), before);
    assert_eq!(prepared.summary().id, SUMMARY_ID);
    assert_eq!(prepared.summary().content, "summarygamma prefers mint tea");
    assert_eq!(prepared.sources_before().len(), 2);
    for (source, archived) in prepared
        .sources_before()
        .iter()
        .zip(prepared.sources_after())
    {
        assert_eq!(manager.get(&source.id).as_ref(), Some(source));
        let mut expected = source.clone();
        expected.memory_type = MemoryType::Reflection;
        expected.tags = Some(vec![GROUP.into()]);
        assert_eq!(archived, &expected);
    }
    assert_eq!(
        manager.fact_consolidation_state(&prepared),
        MemoryConsolidationState::Before
    );
    assert_eq!(
        PreparedMemoryConsolidation::from_images(
            prepared.sources_before().to_vec(),
            prepared.sources_after().to_vec(),
            prepared.summary().clone(),
        ),
        Ok(prepared)
    );
}

#[test]
fn reversible_preparation_rechecks_visibility_and_reserved_summary_id() {
    let (mut manager, sources) = fixture();
    for summary_id in ["", "  ", &"x".repeat(257), &sources[0].id] {
        let before = manager.snapshot();
        assert!(manager
            .prepare_fact_consolidation(
                "owner",
                None,
                &sources,
                "summary",
                GROUP,
                summary_id,
                |_| true
            )
            .is_err());
        assert_eq!(manager.snapshot(), before);
    }
    let mut collision = manager.get(&sources[0].id).unwrap();
    collision.id = SUMMARY_ID.into();
    manager.memories.insert(SUMMARY_ID.into(), collision);
    let before = manager.snapshot();
    assert!(manager
        .prepare_fact_consolidation(
            "owner",
            None,
            &sources,
            "summary",
            GROUP,
            SUMMARY_ID,
            |_| true
        )
        .is_err());
    assert_eq!(manager.snapshot(), before);
    manager.memories.remove(SUMMARY_ID);
    assert_eq!(
        manager.prepare_fact_consolidation(
            "owner",
            None,
            &sources,
            "summary",
            GROUP,
            SUMMARY_ID,
            |memory| memory.id != sources[1].id
        ),
        Err(MemoryConsolidationError::SourceUnavailable)
    );
}

#[test]
fn reversible_image_reconstruction_rejects_arbitrary_field_changes() {
    let (manager, sources) = fixture();
    let prepared = prepare(&manager, &sources);
    let changes: &[fn(&mut Memory)] = &[
        |memory| memory.id.push('x'),
        |memory| memory.agent_id.push('x'),
        |memory| memory.agent_name.push('x'),
        |memory| memory.content.push('x'),
        |memory| memory.created_at += 1,
        |memory| memory.importance = 0.9,
        |memory| memory.tags = None,
        |memory| memory.scope = MemoryScope::Shared,
        |memory| memory.room_id = Some("room".into()),
        |memory| memory.world_id = Some("world".into()),
        |memory| memory.session_id = Some("session".into()),
        |memory| memory.memory_type = MemoryType::Observation,
    ];
    for change in changes {
        let mut after = prepared.sources_after().to_vec();
        change(&mut after[1]);
        assert!(PreparedMemoryConsolidation::from_images(
            prepared.sources_before().to_vec(),
            after,
            prepared.summary().clone(),
        )
        .is_err());
    }
    for change in changes {
        let mut summary = prepared.summary().clone();
        change(&mut summary);
        // ID, content and timestamp are intentionally allocated by the host/preparer.
        if summary.id != prepared.summary().id
            || summary.content != prepared.summary().content
            || summary.created_at != prepared.summary().created_at
        {
            continue;
        }
        assert!(PreparedMemoryConsolidation::from_images(
            prepared.sources_before().to_vec(),
            prepared.sources_after().to_vec(),
            summary,
        )
        .is_err());
    }
    let mut before = prepared.sources_before().to_vec();
    before[1] = before[0].clone();
    assert!(PreparedMemoryConsolidation::from_images(
        before,
        prepared.sources_after().to_vec(),
        prepared.summary().clone()
    )
    .is_err());
}

#[test]
fn reversible_apply_and_undo_preserve_unrelated_records_and_graphs() {
    let (mut manager, sources) = fixture();
    let prepared = prepare(&manager, &sources);
    let mut neighbor = prepared.sources_before()[0].clone();
    neighbor.id = "unrelated".into();
    neighbor.content = "neighbor mutation made after preparation".into();
    manager
        .memories
        .insert(neighbor.id.clone(), neighbor.clone());
    add_citation(&mut manager, &sources[0].id, "agent");
    let before = manager.snapshot();

    let applied = manager
        .apply_fact_consolidation(&prepared, |_| true)
        .unwrap();
    assert_eq!(applied.summary, *prepared.summary());
    assert_eq!(manager.get(SUMMARY_ID), Some(prepared.summary().clone()));
    assert_eq!(
        manager.fact_consolidation_state(&prepared),
        MemoryConsolidationState::After
    );
    assert_eq!(
        manager.snapshot().agent_relationships,
        before.agent_relationships
    );
    assert_eq!(manager.get("unrelated"), Some(neighbor));
    assert_eq!(
        manager
            .search("summarygamma", MemorySearchOptions::default())
            .len(),
        1
    );

    manager
        .undo_fact_consolidation(&prepared, |_| true)
        .unwrap();

    assert_eq!(manager.snapshot(), before);
    assert_eq!(
        manager.fact_consolidation_state(&prepared),
        MemoryConsolidationState::Before
    );
    assert!(manager
        .search("summarygamma", MemorySearchOptions::default())
        .is_empty());
    assert_eq!(
        manager
            .search("originalalpha", MemorySearchOptions::default())
            .len(),
        1
    );
}

#[test]
fn reversible_apply_refuses_any_semantic_source_drift_without_partial_changes() {
    let changes: &[fn(&mut Memory)] = &[
        |memory| memory.content.push('x'),
        |memory| memory.agent_name.push('x'),
        |memory| memory.created_at += 1,
        |memory| memory.importance = 0.9,
        |memory| memory.tags = Some(vec!["new tag".into()]),
    ];
    for change in changes {
        let (mut manager, sources) = fixture();
        let prepared = prepare(&manager, &sources);
        change(manager.memories.get_mut(&sources[1].id).unwrap());
        let before = manager.snapshot();
        assert_eq!(
            manager.apply_fact_consolidation(&prepared, |_| true),
            Err(MemoryConsolidationError::StateDiverged)
        );
        assert_eq!(manager.snapshot(), before);
        assert_eq!(
            manager.fact_consolidation_state(&prepared),
            MemoryConsolidationState::Diverged
        );
    }
}

#[test]
fn reversible_apply_refuses_hidden_source_or_summary_collision_atomically() {
    let (mut manager, sources) = fixture();
    let prepared = prepare(&manager, &sources);
    let before = manager.snapshot();
    assert_eq!(
        manager.apply_fact_consolidation(&prepared, |memory| memory.id != sources[1].id),
        Err(MemoryConsolidationError::SourceUnavailable)
    );
    assert_eq!(manager.snapshot(), before);
    manager
        .memories
        .insert(SUMMARY_ID.into(), prepared.summary().clone());
    let collision = manager.snapshot();
    assert_eq!(
        manager.fact_consolidation_state(&prepared),
        MemoryConsolidationState::Diverged
    );
    assert_eq!(
        manager.apply_fact_consolidation(&prepared, |_| true),
        Err(MemoryConsolidationError::StateDiverged)
    );
    assert_eq!(manager.snapshot(), collision);
}

#[test]
fn reversible_undo_refuses_new_summary_citations_without_stripping_evidence() {
    for kind in ["agent", "fact", "temporal"] {
        let (mut manager, sources) = fixture();
        let prepared = prepare(&manager, &sources);
        manager
            .apply_fact_consolidation(&prepared, |_| true)
            .unwrap();
        add_citation(&mut manager, SUMMARY_ID, kind);
        let before = manager.snapshot();
        assert_eq!(
            manager.undo_fact_consolidation(&prepared, |_| true),
            Err(MemoryConsolidationError::SummaryCited)
        );
        assert_eq!(manager.snapshot(), before);
    }
}

#[test]
fn reversible_undo_refuses_changed_or_hidden_records_without_partial_changes() {
    for changed in [
        "source",
        "summary",
        "missing",
        "hidden-source",
        "hidden-summary",
    ] {
        let (mut manager, sources) = fixture();
        let prepared = prepare(&manager, &sources);
        manager
            .apply_fact_consolidation(&prepared, |_| true)
            .unwrap();
        match changed {
            "source" => manager.memories.get_mut(&sources[0].id).unwrap().importance = 0.8,
            "summary" => manager
                .memories
                .get_mut(SUMMARY_ID)
                .unwrap()
                .content
                .push('x'),
            "missing" => {
                manager.memories.remove(&sources[0].id);
            }
            _ => {}
        }
        let before = manager.snapshot();
        assert!(manager
            .undo_fact_consolidation(&prepared, |memory| {
                !(changed == "hidden-source" && memory.id == sources[0].id
                    || changed == "hidden-summary" && memory.id == SUMMARY_ID)
            })
            .is_err());
        assert_eq!(manager.snapshot(), before);
    }
}

#[test]
fn reversible_state_classifier_accepts_only_complete_before_or_after_images() {
    let (mut manager, sources) = fixture();
    let prepared = prepare(&manager, &sources);
    manager
        .memories
        .insert(sources[0].id.clone(), prepared.sources_after()[0].clone());
    assert_eq!(
        manager.fact_consolidation_state(&prepared),
        MemoryConsolidationState::Diverged
    );
    manager
        .memories
        .insert(sources[1].id.clone(), prepared.sources_after()[1].clone());
    assert_eq!(
        manager.fact_consolidation_state(&prepared),
        MemoryConsolidationState::Diverged
    );
    manager
        .memories
        .insert(SUMMARY_ID.into(), prepared.summary().clone());
    assert_eq!(
        manager.fact_consolidation_state(&prepared),
        MemoryConsolidationState::After
    );
    let mut restored = MemoryManager::new();
    restored.replace_snapshot(manager.snapshot());
    let journal = PreparedMemoryConsolidation::from_images(
        prepared.sources_before().to_vec(),
        prepared.sources_after().to_vec(),
        prepared.summary().clone(),
    )
    .unwrap();
    assert_eq!(
        restored.fact_consolidation_state(&journal),
        MemoryConsolidationState::After
    );
    restored
        .undo_fact_consolidation(&journal, |_| true)
        .unwrap();
    assert_eq!(
        restored.fact_consolidation_state(&journal),
        MemoryConsolidationState::Before
    );
}

fn add_citation(manager: &mut MemoryManager, id: &str, kind: &str) {
    let agent = RelationshipEndpointKind::Agent;
    match kind {
        "agent" => {
            manager
                .upsert_agent_relationship(NewAgentRelationship {
                    source_kind: Some(agent),
                    source_agent_id: "owner".into(),
                    source_agent_name: "Companion".into(),
                    target_kind: Some(agent),
                    target_agent_id: "friend".into(),
                    target_agent_name: "Friend".into(),
                    relationship_type: "knows".into(),
                    summary: None,
                    strength: 0.5,
                    confidence: 0.5,
                    evidence_memory_ids: vec![id.into()],
                    tags: None,
                    room_id: None,
                    world_id: None,
                    session_id: None,
                })
                .unwrap();
        }
        "fact" => {
            manager
                .add_temporal_fact(NewTemporalFact {
                    subject_kind: agent,
                    subject_id: "owner".into(),
                    subject_name: "Companion".into(),
                    predicate: "likes".into(),
                    object_kind: None,
                    object_id: None,
                    object_name: None,
                    value: Some("tea".into()),
                    valid_from: None,
                    valid_to: None,
                    observed_at: None,
                    confidence: 0.5,
                    evidence_memory_ids: vec![id.into()],
                    supersedes_fact_ids: Vec::new(),
                    status: None,
                    tags: None,
                    room_id: None,
                    world_id: None,
                    session_id: None,
                })
                .unwrap();
        }
        "temporal" => {
            manager
                .add_temporal_relationship(NewTemporalRelationship {
                    source_kind: agent,
                    source_id: "owner".into(),
                    source_name: "Companion".into(),
                    target_kind: agent,
                    target_id: "friend".into(),
                    target_name: "Friend".into(),
                    relationship_type: "knows".into(),
                    summary: None,
                    strength: 0.5,
                    confidence: 0.5,
                    valid_from: None,
                    valid_to: None,
                    observed_at: None,
                    evidence_memory_ids: vec![id.into()],
                    supersedes_relationship_ids: Vec::new(),
                    status: None,
                    tags: None,
                    room_id: None,
                    world_id: None,
                    session_id: None,
                })
                .unwrap();
        }
        _ => unreachable!(),
    }
}
