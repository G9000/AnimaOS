//! Behaviour every history store shares (memory, SQLite, Postgres).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anima_core::{Content, DataValue, Message, MessageRole, ToolCall};
use async_trait::async_trait;
use tokio::sync::Semaphore;

use super::{
    ApprovalPageQuery, FilteredMessageSearchQuery, FilteredSessionSearchQuery, HistoryError,
    HistoryMessage, HistoryStore, MemoryHistoryStore, MessageAfterQuery, MessagePageQuery,
    UsagePageQuery,
};
use crate::approvals::{
    ApprovalDecisionKind, ApprovalRequest, ApprovalResolution, ApprovalStatus,
    PendingApprovalStart, ResolvedBy,
};
use crate::runs::{RunRecord, RunSource, RunStart, RunStatus};
use crate::schedules::{ScheduleFireRecord, ScheduleOutcomeStatus};
use crate::usage::{PricingSource, UsageRecord, UsageSource};

pub(crate) fn history_message(
    id: &str,
    agent_id: &str,
    session_id: &str,
    role: MessageRole,
    text: &str,
    created_at_ms: u64,
) -> HistoryMessage {
    HistoryMessage {
        agent_id: agent_id.into(),
        session_id: session_id.into(),
        hidden: false,
        message: Message {
            id: id.into(),
            agent_id: agent_id.into(),
            room_id: session_id.into(),
            content: Content {
                text: text.into(),
                ..Content::default()
            },
            role,
            created_at_ms,
        },
    }
}

pub(crate) fn terminal_run(agent_id: &str, session_id: &str) -> RunRecord {
    let mut run = RunRecord::running(
        RunStart {
            agent_id: agent_id.into(),
            session_id: session_id.into(),
            source: RunSource::Api,
            source_ref: None,
            idempotency_key: None,
            text: "Deploy the build tonight".into(),
            model: "test-model".into(),
            provider: None,
            parent_run_id: None,
        },
        100,
    );
    run.finish(RunStatus::Completed, None, 110);
    run
}

/// An approval the owner allowed once, created at `created_at_ms`.
pub(crate) fn decided_approval(
    id: &str,
    agent_id: &str,
    session_id: &str,
    created_at_ms: u64,
) -> ApprovalRequest {
    let call = ToolCall {
        id: format!("call-{id}"),
        name: "memory_add".into(),
        args: BTreeMap::from([("content".to_string(), DataValue::String("the plan".into()))]),
    };
    let mut approval = ApprovalRequest::pending(
        PendingApprovalStart {
            agent_id,
            session_id,
            run_id: "run_conformance",
            call: &call,
            timeout_ms: 1_000,
            reason: None,
        },
        created_at_ms,
    );
    approval.id = id.to_string();
    approval.resolve(
        ApprovalStatus::Allowed,
        ApprovalResolution {
            decision: Some(ApprovalDecisionKind::AllowOnce),
            note: None,
            matcher: None,
            rule_id: None,
            resolved_by: ResolvedBy::Owner,
            resolved_at_ms: created_at_ms + 5,
        },
    );
    approval
}

/// Every store keeps decided approvals by id, pages them newest first
/// within a window, and removes them with their session or agent. Ids and
/// timestamps are unique per call, so a shared Postgres database can run it
/// repeatedly.
pub(crate) async fn assert_history_store_approval_conformance(store: &dyn HistoryStore) {
    let agent = format!("agent-{}", uuid::Uuid::new_v4());
    let other = format!("agent-{}", uuid::Uuid::new_v4());
    let base = (uuid::Uuid::new_v4().as_u128() % 1_000_000_000) as u64 * 1_000;
    let id = |n: u32| format!("apr_{base}_{n}");
    let first = decided_approval(&id(1), &agent, "chat:a", base + 100);
    let second = decided_approval(&id(2), &agent, "chat:b", base + 200);
    // Same time as `second`: the id breaks the tie.
    let third = decided_approval(&id(3), &agent, "chat:a", base + 200);
    let elsewhere = decided_approval(&id(4), &other, "chat:a", base + 300);
    store
        .upsert_approvals(&[
            first.clone(),
            second.clone(),
            third.clone(),
            elsewhere.clone(),
        ])
        .await
        .unwrap();

    let mut changed = first.clone();
    changed.resolution.as_mut().unwrap().note = Some("rewritten".into());
    store.upsert_approvals(&[changed.clone()]).await.unwrap();
    assert_eq!(store.get_approval(&first.id).await.unwrap(), Some(changed));
    assert_eq!(store.get_approval(&id(99)).await.unwrap(), None);

    let page = |agent_id: Option<&str>, since_ms: u64, before: Option<&ApprovalRequest>, limit| {
        ApprovalPageQuery {
            agent_id: agent_id.map(str::to_string),
            since_ms,
            before: before.map(|approval| (approval.created_at_ms, approval.id.clone())),
            limit,
        }
    };
    let ids = |rows: Vec<ApprovalRequest>| {
        rows.into_iter()
            .map(|approval| approval.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(store
            .page_approvals(&page(Some(&agent), base, None, 10))
            .await
            .unwrap()),
        [third.id.clone(), second.id.clone(), first.id.clone()]
    );
    let first_page = store
        .page_approvals(&page(Some(&agent), base, None, 2))
        .await
        .unwrap();
    assert_eq!(
        ids(first_page.clone()),
        [third.id.clone(), second.id.clone()]
    );
    assert_eq!(
        ids(store
            .page_approvals(&page(Some(&agent), base, first_page.last(), 2))
            .await
            .unwrap()),
        [first.id.clone()]
    );
    assert_eq!(
        ids(store
            .page_approvals(&page(Some(&agent), base + 150, None, 10))
            .await
            .unwrap()),
        [third.id.clone(), second.id.clone()],
        "only approvals created inside the window"
    );
    assert_eq!(
        ids(store
            .page_approvals(&page(Some(&agent), base, Some(&third), 1))
            .await
            .unwrap()),
        [second.id.clone()],
        "a cursor inside a tie continues with the lower id"
    );
    assert_eq!(
        ids(store
            .page_approvals(&page(Some(&agent), base + 100, None, 10))
            .await
            .unwrap()),
        [third.id.clone(), second.id.clone(), first.id.clone()],
        "the window includes its start"
    );
    // `since_ms = base` keeps other runs' older rows out of a shared database;
    // the filter keeps this run's rows if newer ones from other runs exist.
    let everyone = store
        .page_approvals(&page(None, base, None, 1_000))
        .await
        .unwrap()
        .into_iter()
        .filter(|approval| approval.agent_id == agent || approval.agent_id == other)
        .collect::<Vec<_>>();
    assert_eq!(
        ids(everyone),
        [
            elsewhere.id.clone(),
            third.id.clone(),
            second.id.clone(),
            first.id.clone()
        ]
    );

    store.delete_session(&agent, "chat:b").await.unwrap();
    assert_eq!(store.get_approval(&second.id).await.unwrap(), None);
    assert!(store.get_approval(&first.id).await.unwrap().is_some());
    store.delete_agent(&agent).await.unwrap();
    assert_eq!(store.get_approval(&first.id).await.unwrap(), None);
    assert_eq!(store.get_approval(&third.id).await.unwrap(), None);
    assert_eq!(
        store.get_approval(&elsewhere.id).await.unwrap(),
        Some(elsewhere)
    );
}

pub(crate) fn fire_record(
    id: &str,
    schedule_id: &str,
    agent_id: &str,
    fired_at_ms: u64,
) -> ScheduleFireRecord {
    ScheduleFireRecord {
        id: id.into(),
        schedule_id: schedule_id.into(),
        agent_id: agent_id.into(),
        fired_at_ms,
        finished_at_ms: fired_at_ms + 10,
        outcome: ScheduleOutcomeStatus::Spoke,
        run_id: Some(format!("run_{id}")),
        session_id: Some(format!("schedule:{schedule_id}")),
        error_code: None,
        manual: false,
    }
}

/// Automation fire history (spec Â§9.1): idempotent writes, newest first per
/// automation, kept by session deletions and removed with the agent.
pub(crate) async fn assert_history_store_schedule_run_conformance(store: &dyn HistoryStore) {
    let agent = format!("agent-{}", uuid::Uuid::new_v4());
    let other = format!("agent-{}", uuid::Uuid::new_v4());
    let schedule = format!("schedule-{}", uuid::Uuid::new_v4());
    let fire =
        |n: u32, at: u64| fire_record(&format!("schedule:{schedule}:{n}"), &schedule, &agent, at);
    let (first, second, tie) = (fire(1, 100), fire(2, 200), fire(3, 200));
    let elsewhere = fire_record(
        &format!("elsewhere-{schedule}"),
        "schedule-other",
        &agent,
        300,
    );
    let foreign = fire_record(&format!("foreign-{schedule}"), &schedule, &other, 400);
    store
        .upsert_schedule_runs(&[
            first.clone(),
            second.clone(),
            tie.clone(),
            elsewhere.clone(),
            foreign.clone(),
        ])
        .await
        .unwrap();
    let mut rewritten = first.clone();
    rewritten.outcome = ScheduleOutcomeStatus::Failed;
    store
        .upsert_schedule_runs(&[rewritten.clone()])
        .await
        .unwrap();
    store.upsert_schedule_runs(&[]).await.unwrap();

    assert_eq!(
        store
            .page_schedule_runs(&agent, &schedule, 50)
            .await
            .unwrap(),
        vec![tie.clone(), second.clone(), rewritten.clone()],
        "newest first, the id breaking a tie, the rewrite in place"
    );
    assert_eq!(
        store
            .page_schedule_runs(&agent, &schedule, 1)
            .await
            .unwrap(),
        vec![tie]
    );
    assert_eq!(
        store
            .page_schedule_runs(&other, &schedule, 50)
            .await
            .unwrap(),
        vec![foreign.clone()]
    );

    store
        .delete_session(&agent, &format!("schedule:{schedule}"))
        .await
        .unwrap();
    assert_eq!(
        store
            .page_schedule_runs(&agent, &schedule, 50)
            .await
            .unwrap()
            .len(),
        3,
        "deleting a session leaves the automation's history"
    );
    store.delete_agent(&agent).await.unwrap();
    assert!(store
        .page_schedule_runs(&agent, &schedule, 50)
        .await
        .unwrap()
        .is_empty());
    assert!(store
        .page_schedule_runs(&agent, "schedule-other", 50)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .page_schedule_runs(&other, &schedule, 50)
            .await
            .unwrap(),
        vec![foreign],
        "another agent's history stays"
    );
}

pub(crate) fn usage_fixture(
    id: &str,
    agent_id: &str,
    session_id: Option<&str>,
    created_at_ms: u64,
) -> UsageRecord {
    UsageRecord {
        id: id.into(),
        agent_id: agent_id.into(),
        session_id: session_id.map(str::to_string),
        run_id: Some(format!("run_{id}")),
        source: UsageSource::Chat,
        provider: "anthropic".into(),
        model: "claude-fable-5-1".into(),
        prompt_tokens: 10,
        completion_tokens: 5,
        cached_prompt_tokens: 2,
        reasoning_tokens: 1,
        total_tokens: 15,
        cost_micros: Some(100),
        pricing_source: PricingSource::Table,
        duration_ms: 7,
        created_at_ms,
    }
}

fn usage_ids(rows: &[UsageRecord]) -> Vec<String> {
    rows.iter().map(|row| row.id.clone()).collect()
}

fn usage_query(agent_id: &str, limit: usize) -> UsagePageQuery {
    UsagePageQuery {
        from_ms: 0,
        to_ms: u64::MAX / 4,
        agent_id: Some(agent_id.to_string()),
        session_id: None,
        before: None,
        limit,
    }
}

/// Usage records: idempotent by id with the first price kept, filtered by
/// range, agent, and session, paged newest first with a byte-order id
/// tie-break, and kept when a session or agent is deleted. Ids and agents are
/// unique per call, so a shared Postgres database can run it repeatedly.
pub(crate) async fn assert_history_store_usage_conformance(store: &dyn HistoryStore) {
    let tag = uuid::Uuid::new_v4().to_string();
    let agent = format!("agent-{tag}");
    let other = format!("agent-other-{tag}");
    let (chat, notes) = (format!("chat:{tag}"), format!("notes:{tag}"));
    let id = |name: &str| format!("{tag}-{name}");

    // Idempotent by id; a rewrite changes the fields but not the first price.
    let first = usage_fixture(&id("a"), &agent, Some(&chat), 1_000);
    store.upsert_usage(&[]).await.unwrap();
    store.upsert_usage(&[first.clone()]).await.unwrap();
    store.upsert_usage(&[first.clone()]).await.unwrap();
    assert_eq!(
        store.page_usage(&usage_query(&agent, 50)).await.unwrap(),
        vec![first.clone()],
        "writing an id twice keeps one row"
    );
    let mut rewritten = first.clone();
    rewritten.duration_ms = 99;
    rewritten.cost_micros = Some(999_999);
    rewritten.pricing_source = PricingSource::Override;
    store.upsert_usage(&[rewritten]).await.unwrap();
    let kept = store.page_usage(&usage_query(&agent, 50)).await.unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].duration_ms, 99, "other fields are replaced");
    assert_eq!(
        (kept[0].cost_micros, kept[0].pricing_source),
        (Some(100), PricingSource::Table),
        "re-upserting an id must not change its stored price"
    );

    // An unpriced row stays unpriced when the id is written again.
    let mut unpriced = usage_fixture(&id("unpriced"), &agent, Some(&notes), 1_100);
    unpriced.cost_micros = None;
    unpriced.pricing_source = PricingSource::Unknown;
    store.upsert_usage(&[unpriced.clone()]).await.unwrap();
    let mut priced_later = unpriced.clone();
    priced_later.cost_micros = Some(5);
    priced_later.pricing_source = PricingSource::Table;
    store.upsert_usage(&[priced_later]).await.unwrap();
    let rows = store.page_usage(&usage_query(&agent, 50)).await.unwrap();
    let stored = rows.iter().find(|row| row.id == unpriced.id).unwrap();
    assert_eq!(
        (stored.cost_micros, stored.pricing_source),
        (None, PricingSource::Unknown)
    );

    // Filters: range [from, to), agent, session; a row with no session never
    // matches a session filter.
    let loose = usage_fixture(&id("loose"), &agent, None, 1_200);
    let elsewhere = usage_fixture(&id("elsewhere"), &other, Some(&chat), 1_300);
    store
        .upsert_usage(&[loose.clone(), elsewhere.clone()])
        .await
        .unwrap();
    let mut by_range = usage_query(&agent, 50);
    by_range.from_ms = 1_100;
    by_range.to_ms = 1_200;
    assert_eq!(
        usage_ids(&store.page_usage(&by_range).await.unwrap()),
        vec![id("unpriced")],
        "from is inclusive and to is exclusive"
    );
    let mut by_session = usage_query(&agent, 50);
    by_session.session_id = Some(chat.clone());
    assert_eq!(
        usage_ids(&store.page_usage(&by_session).await.unwrap()),
        vec![id("a")]
    );
    let mut everyone = usage_query(&agent, 50);
    everyone.agent_id = None;
    everyone.session_id = Some(chat.clone());
    let both = store.page_usage(&everyone).await.unwrap();
    assert_eq!(
        usage_ids(&both),
        vec![id("elsewhere"), id("a")],
        "no agent filter spans agents"
    );
    assert_eq!(
        usage_ids(&store.page_usage(&usage_query(&other, 50)).await.unwrap()),
        vec![id("elsewhere")]
    );

    // Paging: newest first, ties broken by id in byte order (uppercase
    // sorts before lowercase), with a strict cursor.
    let paged_agent = format!("agent-paged-{tag}");
    let ties = ["pB", "pa", "pc"]
        .iter()
        .map(|name| usage_fixture(&id(name), &paged_agent, Some(&chat), 5_000))
        .collect::<Vec<_>>();
    let older = usage_fixture(&id("older"), &paged_agent, Some(&chat), 4_000);
    let newer = usage_fixture(&id("newer"), &paged_agent, Some(&chat), 6_000);
    store
        .upsert_usage(&[
            ties[0].clone(),
            older,
            ties[1].clone(),
            newer,
            ties[2].clone(),
        ])
        .await
        .unwrap();
    let expected = vec![id("newer"), id("pc"), id("pa"), id("pB"), id("older")];
    assert_eq!(
        usage_ids(
            &store
                .page_usage(&usage_query(&paged_agent, 50))
                .await
                .unwrap()
        ),
        expected,
        "newest first, then the id descending in byte order"
    );
    let mut walked = Vec::new();
    let mut query = usage_query(&paged_agent, 2);
    loop {
        let page = store.page_usage(&query).await.unwrap();
        assert!(page.len() <= 2);
        let Some(last) = page.last() else { break };
        query.before = Some((last.created_at_ms, last.id.clone()));
        walked.extend(usage_ids(&page));
    }
    assert_eq!(walked, expected, "a cursor walk visits every row once");

    // Deleting a session or an agent leaves usage rows.
    store.delete_session(&agent, &chat).await.unwrap();
    assert_eq!(
        usage_ids(&store.page_usage(&usage_query(&agent, 50)).await.unwrap()).len(),
        3,
        "deleting a session keeps its usage rows"
    );
    store.delete_agent(&agent).await.unwrap();
    assert_eq!(
        usage_ids(&store.page_usage(&usage_query(&agent, 50)).await.unwrap()).len(),
        3,
        "deleting an agent keeps its usage rows"
    );
}

fn ids(rows: &[HistoryMessage]) -> Vec<String> {
    rows.iter().map(|row| row.message.id.clone()).collect()
}

fn page(
    agent_id: &str,
    session_id: &str,
    before: Option<&HistoryMessage>,
    limit: usize,
    include_hidden: bool,
) -> MessagePageQuery {
    MessagePageQuery {
        agent_id: agent_id.into(),
        session_id: session_id.into(),
        before: before.map(HistoryMessage::order),
        limit,
        include_hidden,
    }
}

/// Every store must pass this. Agent ids, timestamps, and message ids are
/// unique per call, so a shared Postgres database can run it repeatedly.
pub(crate) async fn assert_history_store_conformance(store: &dyn HistoryStore) {
    let agent = format!("agent-{}", uuid::Uuid::new_v4());
    let other = format!("agent-{}", uuid::Uuid::new_v4());
    let base = (uuid::Uuid::new_v4().as_u128() % 1_000_000_000) as u64 * 1_000;
    let at = |offset: u64| base + offset;
    let id = |offset: u64, ordinal: u64| format!("msg-{}-{ordinal}", base + offset);
    let mut silent = history_message(
        &id(300, 12),
        &agent,
        "chat:a",
        MessageRole::Assistant,
        "CHECKIN_OK deploy",
        at(300),
    );
    silent.hidden = true;
    // Two messages in the same millisecond keep their creation order through
    // the id counter (9 before 10, which string order would reverse).
    let rows = vec![
        history_message(
            &id(100, 9),
            &agent,
            "chat:a",
            MessageRole::User,
            "Deploy the build tonight",
            at(100),
        ),
        history_message(
            &id(100, 10),
            &agent,
            "chat:a",
            MessageRole::Assistant,
            "Build deployed.",
            at(100),
        ),
        history_message(
            &id(200, 11),
            &agent,
            "chat:a",
            MessageRole::User,
            "Thanks",
            at(200),
        ),
        silent,
        history_message(
            &id(150, 13),
            &agent,
            "chat:b",
            MessageRole::User,
            "deployment notes for later",
            at(150),
        ),
        history_message(
            &id(160, 14),
            &other,
            "chat:a",
            MessageRole::User,
            "deploy elsewhere",
            at(160),
        ),
    ];
    store.upsert_messages(&rows).await.expect("messages upsert");
    let mut edited = rows[2].clone();
    edited.message.content.text = "Thanks!".into();
    store
        .upsert_messages(&[edited.clone(), edited])
        .await
        .expect("an id written twice stays one row");

    let newest = store
        .page_messages(&page(&agent, "chat:a", None, 10, false))
        .await
        .unwrap();
    assert_eq!(ids(&newest), [id(200, 11), id(100, 10), id(100, 9)]);
    assert_eq!(newest[0].message.content.text, "Thanks!");
    assert_eq!(newest[0].agent_id, agent);
    assert_eq!(newest[0].session_id, "chat:a");
    let everything = store
        .page_messages(&page(&agent, "chat:a", None, 10, true))
        .await
        .unwrap();
    assert_eq!(
        ids(&everything),
        [id(300, 12), id(200, 11), id(100, 10), id(100, 9)]
    );
    assert!(everything[0].hidden);
    let second = store
        .page_messages(&page(&agent, "chat:a", Some(&rows[2]), 1, false))
        .await
        .unwrap();
    assert_eq!(ids(&second), [id(100, 10)]);
    let last = store
        .page_messages(&page(&agent, "chat:a", Some(&rows[1]), 5, false))
        .await
        .unwrap();
    assert_eq!(ids(&last), [id(100, 9)]);
    assert!(store
        .page_messages(&page(&agent, "chat:a", Some(&rows[0]), 5, false))
        .await
        .unwrap()
        .is_empty());

    let counts = store
        .visible_message_counts(
            &agent,
            &[
                "chat:a".to_string(),
                "chat:b".to_string(),
                "chat:none".to_string(),
            ],
        )
        .await
        .unwrap();
    assert_eq!(counts.get("chat:a"), Some(&3));
    assert_eq!(counts.get("chat:b"), Some(&1));
    assert_eq!(counts.get("chat:none").copied().unwrap_or(0), 0);
    assert!(store
        .visible_message_counts(&agent, &[])
        .await
        .unwrap()
        .is_empty());

    let agents = [agent.clone()];
    assert_eq!(
        ids(&store.search_messages(&agents, "deploy", 10).await.unwrap()),
        [id(150, 13), id(100, 10), id(100, 9)],
        "word prefixes, newest first, hidden rows and other agents excluded"
    );
    assert_eq!(
        ids(&store.search_messages(&agents, "DEPL", 10).await.unwrap()),
        [id(150, 13), id(100, 10), id(100, 9)]
    );
    assert_eq!(
        ids(&store
            .search_messages(&agents, "deploy tonight", 10)
            .await
            .unwrap()),
        [id(100, 9)],
        "every word must match"
    );
    assert_eq!(
        ids(&store.search_messages(&agents, "deploy", 1).await.unwrap()),
        [id(150, 13)]
    );
    assert!(store
        .search_messages(&agents, "!!", 10)
        .await
        .unwrap()
        .is_empty());
    assert!(store
        .search_messages(&[], "deploy", 10)
        .await
        .unwrap()
        .is_empty());
    let both = [agent.clone(), other.clone()];
    assert_eq!(
        ids(&store.search_messages(&both, "elsewhere", 10).await.unwrap()),
        [id(160, 14)]
    );

    // A fresh agent id keeps this block from perturbing any count or page
    // asserted above.
    let word_boundary_agent = format!("agent-{}", uuid::Uuid::new_v4());
    store
        .upsert_messages(&[
            history_message(
                &id(170, 15),
                &word_boundary_agent,
                "chat:a",
                MessageRole::User,
                "underdeployment fixed",
                at(170),
            ),
            history_message(
                &id(180, 16),
                &word_boundary_agent,
                "chat:a",
                MessageRole::User,
                "re-deploy tomorrow",
                at(180),
            ),
        ])
        .await
        .expect("word-boundary messages upsert");
    assert_eq!(
        ids(&store
            .search_messages(&[word_boundary_agent.clone()], "deploy", 10)
            .await
            .unwrap()),
        [id(180, 16)],
        "a mid-word occurrence must not match a word-prefix search"
    );

    let known = store
        .existing_message_ids(&[id(100, 9), format!("missing-{base}")])
        .await
        .unwrap();
    assert_eq!(known, HashSet::from([id(100, 9)]));
    assert!(store.existing_message_ids(&[]).await.unwrap().is_empty());
    assert_eq!(
        store
            .get_message(&agent, "chat:a", &id(100, 10))
            .await
            .unwrap()
            .map(|row| row.message.content.text),
        Some("Build deployed.".to_string())
    );
    assert_eq!(
        store
            .get_message(&agent, "chat:b", &id(100, 10))
            .await
            .unwrap(),
        None,
        "a message belongs to one session"
    );

    let run = terminal_run(&agent, "chat:a");
    let kept_run = terminal_run(&agent, "chat:b");
    store
        .upsert_runs(&[run.clone(), kept_run.clone()])
        .await
        .unwrap();
    store.upsert_runs(std::slice::from_ref(&run)).await.unwrap();
    assert_eq!(store.get_run(&run.id).await.unwrap(), Some(run.clone()));

    store.delete_session(&agent, "chat:a").await.unwrap();
    assert!(store
        .page_messages(&page(&agent, "chat:a", None, 10, true))
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        ids(&store.search_messages(&agents, "deploy", 10).await.unwrap()),
        [id(150, 13)]
    );
    assert_eq!(store.get_run(&run.id).await.unwrap(), None);
    assert_eq!(
        store.get_run(&kept_run.id).await.unwrap(),
        Some(kept_run.clone())
    );
    assert_eq!(
        store
            .page_messages(&page(&other, "chat:a", None, 10, false))
            .await
            .unwrap()
            .len(),
        1,
        "other agents' sessions are untouched"
    );
    store
        .delete_session(&agent, "chat:a")
        .await
        .expect("deleting twice is harmless");

    // Deleting an agent removes the rows of every one of its sessions.
    let doomed = format!("agent-{}", uuid::Uuid::new_v4());
    store
        .upsert_messages(&[
            history_message(
                &id(400, 17),
                &doomed,
                "chat:a",
                MessageRole::User,
                "farewell",
                at(400),
            ),
            history_message(
                &id(410, 18),
                &doomed,
                "chat:b",
                MessageRole::User,
                "farewell again",
                at(410),
            ),
        ])
        .await
        .expect("the doomed agent's messages upsert");
    let doomed_run = terminal_run(&doomed, "chat:b");
    store
        .upsert_runs(std::slice::from_ref(&doomed_run))
        .await
        .unwrap();
    store.delete_agent(&doomed).await.unwrap();
    assert!(store
        .existing_message_ids(&[id(400, 17), id(410, 18)])
        .await
        .unwrap()
        .is_empty());
    assert_eq!(store.get_run(&doomed_run.id).await.unwrap(), None);
    assert_eq!(
        store
            .page_messages(&page(&agent, "chat:b", None, 10, false))
            .await
            .unwrap()
            .len(),
        1,
        "another agent's rows survive"
    );
    assert_eq!(
        store.get_run(&kept_run.id).await.unwrap(),
        Some(kept_run),
        "another agent's runs survive"
    );
    store
        .delete_agent(&doomed)
        .await
        .expect("deleting an agent twice is harmless");
}

/// Session search (Controller ruling 2, M2 pre-flight audit): sessions are
/// ranked by their newest matching message, so a session whose only match is
/// older than another session's flood of matches is still returned. Agent id
/// and timestamps are unique per call, so a shared Postgres database can run
/// it repeatedly.
pub(crate) async fn assert_history_store_session_search_conformance(store: &dyn HistoryStore) {
    let agent = format!("agent-{}", uuid::Uuid::new_v4());
    let base = (uuid::Uuid::new_v4().as_u128() % 1_000_000_000) as u64 * 1_000;
    let at = |offset: u64| base + offset;
    let id = |offset: u64, ordinal: u64| format!("msg-{}-{ordinal}", base + offset);

    // "chat:busy" gets 4 matches, more than the `limit: 3` passed below, so a
    // row-limited search (the pre-ruling behaviour: take the newest `limit`
    // *rows*, then group) would fill its whole budget with "chat:busy" rows
    // alone. "chat:quiet" has a single, older match that must still surface
    // once results are grouped into sessions before the limit is applied.
    let mut rows = (0u64..4)
        .map(|n| {
            history_message(
                &id(100 + n * 10, 20 + n),
                &agent,
                "chat:busy",
                MessageRole::User,
                "deploy the build",
                at(100 + n * 10),
            )
        })
        .collect::<Vec<_>>();
    rows.push(history_message(
        &id(10, 30),
        &agent,
        "chat:quiet",
        MessageRole::User,
        "deploy notes",
        at(10),
    ));
    let mut hidden = history_message(
        &id(500, 40),
        &agent,
        "chat:busy",
        MessageRole::Assistant,
        "deploy hidden",
        at(500),
    );
    hidden.hidden = true;
    rows.push(hidden);
    store
        .upsert_messages(&rows)
        .await
        .expect("session-search messages upsert");

    let newest_busy = id(130, 23);
    let quiet = id(10, 30);
    let agents = [agent.clone()];
    let found = store.search_sessions(&agents, "deploy", 3).await.unwrap();
    assert_eq!(
        found
            .iter()
            .map(|row| (row.session_id.as_str(), row.message.id.as_str()))
            .collect::<Vec<_>>(),
        [
            ("chat:busy", newest_busy.as_str()),
            ("chat:quiet", quiet.as_str())
        ],
        "a session with more than the limit's worth of matches must not crowd out an older session's match"
    );
    assert!(
        found.iter().all(|row| !row.hidden),
        "hidden messages never match"
    );

    assert_eq!(
        ids(&store.search_sessions(&agents, "deploy", 1).await.unwrap()),
        [newest_busy],
        "the limit caps the number of sessions, newest session first"
    );
    assert!(store
        .search_sessions(&[], "deploy", 3)
        .await
        .unwrap()
        .is_empty());
    assert!(store
        .search_sessions(&agents, "!!", 3)
        .await
        .unwrap()
        .is_empty());
}

/// A check-in prompt's scheduler suffix (`schedules::wrap_checkin_prompt`)
/// carries ordinary words ("scheduled", "reply", "exactly"...) that must not
/// make the prompt match every search (review fix, M2 fix round 1); a word
/// from the prompt itself must still match. Fresh agent id per call.
pub(crate) async fn assert_history_store_checkin_text_conformance(store: &dyn HistoryStore) {
    let agent = format!("agent-{}", uuid::Uuid::new_v4());
    let checkin = crate::sessions::test_support::checkin_prompt(
        &agent,
        "checkin-1",
        "schedule:s1",
        "s1",
        "Check the widgets inventory",
        1,
    );
    store
        .upsert_messages(&[HistoryMessage {
            agent_id: agent.clone(),
            session_id: "schedule:s1".into(),
            hidden: false,
            message: checkin,
        }])
        .await
        .expect("checkin message upsert");

    let agents = [agent.clone()];
    assert!(
        store
            .search_messages(&agents, "scheduled", 10)
            .await
            .unwrap()
            .is_empty(),
        "a scheduler-suffix-only word must not match a check-in prompt"
    );
    assert!(
        store
            .search_sessions(&agents, "scheduled", 10)
            .await
            .unwrap()
            .is_empty(),
        "a scheduler-suffix-only word must not match a check-in prompt (search_sessions)"
    );
    assert_eq!(
        ids(&store.search_messages(&agents, "widgets", 10).await.unwrap()),
        ["checkin-1"],
        "a word of the prompt itself must still match"
    );
    assert_eq!(
        ids(&store.search_sessions(&agents, "widgets", 10).await.unwrap()),
        ["checkin-1"],
        "a word of the prompt itself must still match (search_sessions)"
    );
}

/// A single huge message (final fix wave item B — e.g. a large `web_fetch`
/// or `read_file` result) must not fail Postgres's generated `search` column,
/// which errors past roughly 1 MB of distinct words: the stores cap the
/// indexed/matched text to `MAX_INDEXED_TEXT_BYTES`, but `record` (read back
/// through `get_message`/`page_messages`) always keeps the full message.
/// Fresh agent id per call.
pub(crate) async fn assert_history_store_indexed_text_cap_conformance(store: &dyn HistoryStore) {
    let agent = format!("agent-{}", uuid::Uuid::new_v4());
    let base = (uuid::Uuid::new_v4().as_u128() % 1_000_000_000) as u64 * 1_000;
    let msg_id = format!("msg-{base}-1");
    // ASCII filler so every byte offset is also a char boundary: "alphaneedle"
    // sits well inside the cap, "omeganeedle" starts only after it.
    let filler = "x".repeat(super::MAX_INDEXED_TEXT_BYTES);
    let text = format!("alphaneedle {filler} omeganeedle");
    assert!(
        text.len() > super::MAX_INDEXED_TEXT_BYTES,
        "the fixture must exceed the cap"
    );
    store
        .upsert_messages(&[history_message(
            &msg_id,
            &agent,
            "chat:a",
            MessageRole::User,
            &text,
            base,
        )])
        .await
        .expect("oversized message upsert");

    let fetched = store
        .get_message(&agent, "chat:a", &msg_id)
        .await
        .unwrap()
        .expect("the message is found");
    assert_eq!(
        fetched.message.content.text, text,
        "get_message returns the full message unchanged, however long"
    );
    let paged = store
        .page_messages(&page(&agent, "chat:a", None, 10, false))
        .await
        .unwrap();
    assert_eq!(
        paged.first().map(|row| row.message.content.text.clone()),
        Some(text.clone()),
        "page_messages returns the full message unchanged, however long"
    );

    let agents = [agent.clone()];
    assert_eq!(
        ids(&store
            .search_messages(&agents, "alphaneedle", 10)
            .await
            .unwrap()),
        [msg_id.clone()],
        "a word inside the first 64 KiB is searchable"
    );
    assert!(
        store
            .search_messages(&agents, "omeganeedle", 10)
            .await
            .unwrap()
            .is_empty(),
        "a word only past the first 64 KiB must not be indexed"
    );
}

/// SQLite FTS5's default `unicode61` tokenizer strips diacritics (`café`
/// would index as `cafe`), while the memory store, the hot-tail matcher
/// (`sessions::views`), and Postgres's `simple` text-search config don't
/// (final fix wave item D4); every store must agree. Fresh agent id per
/// call.
pub(crate) async fn assert_history_store_diacritics_conformance(store: &dyn HistoryStore) {
    let agent = format!("agent-{}", uuid::Uuid::new_v4());
    let base = (uuid::Uuid::new_v4().as_u128() % 1_000_000_000) as u64 * 1_000;
    let msg_id = format!("msg-{base}-1");
    store
        .upsert_messages(&[history_message(
            &msg_id,
            &agent,
            "chat:a",
            MessageRole::User,
            "Let's meet at the café tomorrow",
            base,
        )])
        .await
        .expect("diacritics message upsert");

    let agents = [agent.clone()];
    assert!(
        store
            .search_messages(&agents, "cafe", 10)
            .await
            .unwrap()
            .is_empty(),
        "a search without the diacritic must not match café"
    );
    assert!(
        store
            .search_sessions(&agents, "cafe", 10)
            .await
            .unwrap()
            .is_empty(),
        "a search without the diacritic must not match café (search_sessions)"
    );
    assert_eq!(
        ids(&store.search_messages(&agents, "café", 10).await.unwrap()),
        [msg_id.clone()],
        "a search with the diacritic must match café"
    );
    assert_eq!(
        ids(&store.search_sessions(&agents, "café", 10).await.unwrap()),
        [msg_id.clone()],
        "a search with the diacritic must match café (search_sessions)"
    );
}

/// Additive M41 ports keep exact scope/order and filter before grouping/topN.
/// UUID source identities permit the same fixture on a shared Postgres store.
pub(crate) async fn assert_history_store_m41_conformance(store: &dyn HistoryStore) {
    let agent = format!("agent-{}", uuid::Uuid::new_v4());
    let other = format!("agent-{}", uuid::Uuid::new_v4());
    let base = (uuid::Uuid::new_v4().as_u128() % 1_000_000_000) as u64 * 1000;
    let id = |offset, ordinal| format!("msg-{}-{ordinal}", base + offset);
    let row = |offset, ordinal, session: &str, role, text: &str| {
        history_message(
            &id(offset, ordinal),
            &agent,
            session,
            role,
            text,
            base + offset,
        )
    };
    let nine = row(0, 9, "window", MessageRole::User, "nine");
    let ten = row(0, 10, "window", MessageRole::Assistant, "ten");
    let hundred = row(0, 100, "window", MessageRole::Tool, "one hundred");
    let mut hidden = row(0, 50, "window", MessageRole::User, "hidden");
    hidden.hidden = true;
    let mut rows = vec![
        nine.clone(),
        ten.clone(),
        hundred.clone(),
        hidden.clone(),
        row(0, 101, "elsewhere", MessageRole::User, "wrong session"),
        history_message(
            &id(0, 102),
            &other,
            "window",
            MessageRole::User,
            "wrong owner",
            base,
        ),
    ];
    let letter = |name| {
        history_message(
            &format!("{name}-{base}"),
            &agent,
            "window",
            MessageRole::User,
            name,
            base + 2,
        )
    };
    let alpha = letter("A");
    let zulu = letter("Z");
    let lower = letter("a");
    let unicode = letter("é");
    rows.extend([alpha.clone(), zulu.clone(), lower.clone(), unicode.clone()]);
    rows.extend((3..33).map(|offset| row(offset, 1, "window", MessageRole::User, "neighbor")));
    let a = row(
        200,
        1,
        "filtered:a",
        MessageRole::User,
        "deployment building plan",
    );
    let b = row(
        360,
        1,
        "filtered:b",
        MessageRole::Tool,
        "deploy build result",
    );
    let shadowed = row(
        370,
        1,
        "filtered:a",
        MessageRole::User,
        "deploy build stale cold copy",
    );
    let counter = row(
        360,
        100,
        "filtered:counter",
        MessageRole::Tool,
        "deploy build newest counter",
    );
    let mut hidden_match = row(
        380,
        1,
        "filtered:a",
        MessageRole::Assistant,
        "deploy build hidden",
    );
    hidden_match.hidden = true;
    rows.extend([
        a.clone(),
        b.clone(),
        hidden_match,
        counter.clone(),
        shadowed.clone(),
        row(
            360,
            9,
            "filtered:counter",
            MessageRole::User,
            "deploy build nine",
        ),
        row(
            360,
            10,
            "filtered:counter",
            MessageRole::Assistant,
            "deploy build ten",
        ),
        row(
            199,
            1,
            "filtered:a",
            MessageRole::User,
            "deploy build too early",
        ),
        row(
            390,
            1,
            "filtered:a",
            MessageRole::System,
            "deploy build system instruction",
        ),
        row(
            400,
            1,
            "filtered:a",
            MessageRole::Assistant,
            "deploy build exclusive boundary",
        ),
        row(
            500,
            1,
            "filtered:a",
            MessageRole::User,
            "deploy build too late",
        ),
        history_message(
            &id(398, 1),
            &other,
            "filtered:a",
            MessageRole::User,
            "deploy build foreign",
            base + 398,
        ),
    ]);
    rows.extend((0..25).map(|index| {
        row(
            600 + index,
            1,
            &format!("wrong-kind-{index}"),
            MessageRole::User,
            "deploy build distractor",
        )
    }));
    store.upsert_messages(&rows).await.unwrap();
    let ids = |rows: Vec<HistoryMessage>| {
        rows.into_iter()
            .map(|row| row.message.id)
            .collect::<Vec<_>>()
    };
    let after = |anchor: &HistoryMessage, limit, include_hidden| MessageAfterQuery {
        agent_id: agent.clone(),
        session_id: "window".into(),
        after: anchor.order(),
        limit,
        include_hidden,
    };
    assert!(store
        .page_messages_after(&after(&ten, 0, false))
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .page_messages_after(&after(&ten, 1, false))
            .await
            .unwrap(),
        [hundred.clone()]
    );
    assert_eq!(
        ids(store
            .page_messages_after(&after(&ten, 2, true))
            .await
            .unwrap()),
        [hidden.message.id.clone(), hundred.message.id.clone()]
    );
    let page = store
        .page_messages_after(&after(&ten, 20, false))
        .await
        .unwrap();
    assert_eq!(page.len(), 20);
    assert!(page.iter().all(|row| row.agent_id == agent
        && row.session_id == "window"
        && !row.hidden
        && row.order() > ten.order()));
    assert!(page
        .windows(2)
        .all(|pair| pair[0].order() < pair[1].order()));
    assert_eq!(
        ids(store
            .page_messages_after(&after(&alpha, 3, false))
            .await
            .unwrap()),
        [zulu.message.id, lower.message.id, unicode.message.id]
    );
    assert_eq!(
        ids(store
            .page_messages(&MessagePageQuery {
                agent_id: agent.clone(),
                session_id: "window".into(),
                before: Some(ten.order()),
                limit: 20,
                include_hidden: false
            })
            .await
            .unwrap()),
        [nine.message.id]
    );
    let message_query = FilteredMessageSearchQuery {
        agent_id: agent.clone(),
        session_ids: vec![
            "filtered:a".into(),
            "filtered:b".into(),
            "filtered:counter".into(),
        ],
        excluded_message_ids: vec![shadowed.message.id.clone()],
        query: "deplo buil".into(),
        after_ms: Some(base + 200),
        before_ms: Some(base + 400),
        limit: 5,
    };
    assert_eq!(
        ids(store
            .search_messages_filtered(&message_query)
            .await
            .unwrap()),
        [
            counter.message.id.clone(),
            id(360, 10),
            id(360, 9),
            b.message.id.clone(),
            a.message.id.clone()
        ],
        "individual matches retain several messages in one session and filter before topN"
    );
    let mut small = message_query.clone();
    small.limit = 1;
    assert_eq!(
        store.search_messages_filtered(&small).await.unwrap(),
        [counter.clone()]
    );
    small.session_ids = vec!["filtered:a".into()];
    assert_eq!(
        store.search_messages_filtered(&small).await.unwrap(),
        [a.clone()]
    );
    small.excluded_message_ids.clear();
    assert_eq!(
        store.search_messages_filtered(&small).await.unwrap(),
        [shadowed.clone()]
    );
    small.query = "!!!".into();
    assert!(store
        .search_messages_filtered(&small)
        .await
        .unwrap()
        .is_empty());
    for case in 0..6 {
        let mut invalid = message_query.clone();
        match case {
            0 => invalid.limit = 22,
            1 => invalid.session_ids = vec!["s".into(); 501],
            2 => invalid.excluded_message_ids = vec!["m".into(); 5001],
            3 => invalid.query = "n".repeat(201),
            4 => invalid.before_ms = invalid.after_ms,
            _ => invalid.agent_id = "bad\0id".into(),
        }
        assert!(
            store.search_messages_filtered(&invalid).await.is_err(),
            "message search bounds case {case}"
        );
    }
    let mut query = FilteredSessionSearchQuery {
        agent_id: agent.clone(),
        session_ids: vec!["filtered:a".into(), "filtered:b".into()],
        excluded_message_ids: vec![shadowed.message.id.clone()],
        query: "deplo buil".into(),
        after_ms: Some(base + 200),
        before_ms: Some(base + 400),
        limit: 2,
    };
    assert_eq!(
        store.search_sessions_filtered(&query).await.unwrap(),
        [b.clone(), a.clone()]
    );
    let mut cold_only = query.clone();
    cold_only.excluded_message_ids.clear();
    assert_eq!(
        store.search_sessions_filtered(&cold_only).await.unwrap(),
        [shadowed.clone(), b.clone()]
    );
    let mut tied = query.clone();
    tied.session_ids = vec!["filtered:b".into(), "filtered:counter".into()];
    tied.limit = 1;
    assert_eq!(
        store.search_sessions_filtered(&tied).await.unwrap(),
        [counter],
        "grouping and topN preserve numeric ordinal ties"
    );
    query.limit = 1;
    assert_eq!(
        store.search_sessions_filtered(&query).await.unwrap(),
        [b.clone()]
    );
    query.session_ids = vec!["filtered:a".into()];
    assert_eq!(
        store.search_sessions_filtered(&query).await.unwrap(),
        [a.clone()],
        "scope, hidden, role and time filters must precede newest-per-session/topN"
    );
    query.after_ms = Some(base + 201);
    assert!(store
        .search_sessions_filtered(&query)
        .await
        .unwrap()
        .is_empty());
    query.after_ms = Some(base + 200);
    query.session_ids.push("filtered:b".into());
    query.before_ms = Some(base + 360);
    assert_eq!(
        store.search_sessions_filtered(&query).await.unwrap(),
        [a.clone()],
        "before is exclusive"
    );
    query.limit = 0;
    assert!(store
        .search_sessions_filtered(&query)
        .await
        .unwrap()
        .is_empty());
    query.limit = 2;
    let valid = query.clone();
    query.session_ids.clear();
    assert!(store
        .search_sessions_filtered(&query)
        .await
        .unwrap()
        .is_empty());
    query = valid.clone();
    query.query = "!!!".into();
    assert!(store
        .search_sessions_filtered(&query)
        .await
        .unwrap()
        .is_empty());
    query = valid.clone();
    query.session_ids = (0..499)
        .map(|index| format!("absent-{index}"))
        .chain(std::iter::once("filtered:a".into()))
        .collect();
    query.excluded_message_ids = (0..4999)
        .map(|index| format!("absent-message-{index}"))
        .chain(std::iter::once(shadowed.message.id))
        .collect();
    assert_eq!(
        store.search_sessions_filtered(&query).await.unwrap(),
        [a],
        "the full500-session/5000-exclusion scope must work"
    );
    for case in 0..10 {
        let mut invalid = valid.clone();
        match case {
            0 => invalid.session_ids = vec!["filtered:a".into(); 501],
            1 => invalid.limit = usize::MAX,
            2 => invalid.query = "茶".repeat(201),
            3 => invalid.after_ms = Some(9_007_199_254_740_992),
            4 => invalid.before_ms = invalid.after_ms,
            5 => invalid.session_ids = vec!["s".repeat(257)],
            6 => invalid.query = "deploy\u{202E}".into(),
            7 => invalid.agent_id = "a".repeat(257),
            8 => invalid.excluded_message_ids = vec!["m".into(); 5001],
            _ => invalid.excluded_message_ids = vec!["m".repeat(257)],
        }
        assert!(
            store.search_sessions_filtered(&invalid).await.is_err(),
            "filtered argument case {case}"
        );
    }
    for case in 0..4 {
        let mut invalid = after(&ten, 1, false);
        match case {
            0 => invalid.limit = 21,
            1 => invalid.after.created_at_ms = u64::MAX,
            2 => invalid.after.ordinal = u64::MAX,
            _ => invalid.after.id = "m".repeat(257),
        }
        assert!(
            store.page_messages_after(&invalid).await.is_err(),
            "forward argument case {case}"
        );
    }
    assert_eq!(
        store
            .get_message(&agent, "window", &ten.message.id)
            .await
            .unwrap(),
        Some(ten)
    );
    store.delete_agent(&agent).await.unwrap();
    store.delete_agent(&other).await.unwrap();
}

/// A memory store whose every call fails while `failing` is set. Unlike the
/// memory store it is not ephemeral, so pruning tests can use it.
pub(crate) struct FlakyHistoryStore {
    inner: MemoryHistoryStore,
    failing: AtomicBool,
    /// Fails both message-page directions, independent of `failing`: lets a test put
    /// `get_message` (or the hot tail) through a `before` lookup and then
    /// fail just the page read that follows it.
    page_messages_failing: AtomicBool,
    /// Fails only `upsert_usage`, independent of `failing`.
    usage_failing: AtomicBool,
    /// The size of every `upsert_usage` batch that reached the store.
    usage_batches: Mutex<Vec<usize>>,
    panic_on_write: AtomicBool,
    existence_gate: Mutex<Option<StoreGate>>,
    message_read_gate: Mutex<Option<(usize, StoreGate)>>,
    message_search_gate: Mutex<Option<StoreGate>>,
    /// The text of the injected failure; a default when unset.
    failure_text: Mutex<Option<String>>,
}

/// Holds one store call: the call adds a permit to `entered`, then waits for
/// one on `release`.
#[derive(Clone)]
pub(crate) struct StoreGate {
    pub(crate) entered: Arc<Semaphore>,
    pub(crate) release: Arc<Semaphore>,
}

impl FlakyHistoryStore {
    pub(crate) fn new() -> Self {
        Self {
            inner: MemoryHistoryStore::new(),
            failing: AtomicBool::new(false),
            page_messages_failing: AtomicBool::new(false),
            usage_failing: AtomicBool::new(false),
            usage_batches: Mutex::new(Vec::new()),
            panic_on_write: AtomicBool::new(false),
            existence_gate: Mutex::new(None),
            message_read_gate: Mutex::new(None),
            message_search_gate: Mutex::new(None),
            failure_text: Mutex::new(None),
        }
    }

    /// Sets the text of the failure `set_failing(true)` injects.
    pub(crate) fn set_failure_text(&self, text: &str) {
        *self
            .failure_text
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(text.to_string());
    }

    pub(crate) fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }

    /// Fails message-page calls in either direction, leaving other methods healthy.
    pub(crate) fn set_page_messages_failing(&self, failing: bool) {
        self.page_messages_failing.store(failing, Ordering::SeqCst);
    }

    /// Fails only `upsert_usage` calls, leaving every other method healthy.
    pub(crate) fn set_usage_failing(&self, failing: bool) {
        self.usage_failing.store(failing, Ordering::SeqCst);
    }

    /// The size of each `upsert_usage` batch written so far, oldest first.
    pub(crate) fn usage_batches(&self) -> Vec<usize> {
        self.usage_batches
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Makes the next `upsert_messages` call panic.
    pub(crate) fn panic_on_next_write(&self) {
        self.panic_on_write.store(true, Ordering::SeqCst);
    }

    /// Holds the next `existing_message_ids` call (a reconcile's store round
    /// trip) at a gate.
    pub(crate) fn hold_next_existence_check(&self) -> StoreGate {
        let gate = StoreGate {
            entered: Arc::new(Semaphore::new(0)),
            release: Arc::new(Semaphore::new(0)),
        };
        *self
            .existence_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(gate.clone());
        gate
    }

    /// Holds one `get_message` read after the given number of reads complete.
    pub(crate) fn hold_message_read_after(&self, skip: usize) -> StoreGate {
        let gate = StoreGate {
            entered: Arc::new(Semaphore::new(0)),
            release: Arc::new(Semaphore::new(0)),
        };
        *self
            .message_read_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((skip, gate.clone()));
        gate
    }

    pub(crate) fn hold_next_message_search(&self) -> StoreGate {
        let gate = StoreGate {
            entered: Arc::new(Semaphore::new(0)),
            release: Arc::new(Semaphore::new(0)),
        };
        *self
            .message_search_gate
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(gate.clone());
        gate
    }

    fn check(&self) -> Result<(), HistoryError> {
        if self.failing.load(Ordering::SeqCst) {
            let text = self
                .failure_text
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .unwrap_or_else(|| "injected history store failure".to_string());
            Err(HistoryError::new(text))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn m41_flaky_store_delegates_both_ports_and_preserves_read_failure_switches() {
    let store = FlakyHistoryStore::new();
    let first = history_message(
        "msg-10-9",
        "port-agent",
        "port-session",
        MessageRole::User,
        "first",
        10,
    );
    let next = history_message(
        "msg-10-10",
        "port-agent",
        "port-session",
        MessageRole::Tool,
        "deploy build",
        10,
    );
    store
        .upsert_messages(&[first.clone(), next.clone()])
        .await
        .unwrap();
    let page = MessageAfterQuery {
        agent_id: "port-agent".into(),
        session_id: "port-session".into(),
        after: first.order(),
        limit: 1,
        include_hidden: false,
    };
    let search = FilteredSessionSearchQuery {
        agent_id: "port-agent".into(),
        session_ids: vec!["port-session".into()],
        excluded_message_ids: Vec::new(),
        query: "deploy".into(),
        after_ms: None,
        before_ms: None,
        limit: 1,
    };
    assert_eq!(
        store.page_messages_after(&page).await.unwrap(),
        [next.clone()]
    );
    assert_eq!(
        store.search_sessions_filtered(&search).await.unwrap(),
        [next.clone()]
    );
    store.set_page_messages_failing(true);
    assert!(store.page_messages_after(&page).await.is_err());
    assert_eq!(
        store.search_sessions_filtered(&search).await.unwrap(),
        [next]
    );
    store.set_page_messages_failing(false);
    store.set_failing(true);
    assert!(store.page_messages_after(&page).await.is_err());
    assert!(store.search_sessions_filtered(&search).await.is_err());
}

#[async_trait]
impl HistoryStore for FlakyHistoryStore {
    fn label(&self) -> &'static str {
        "flaky"
    }

    async fn upsert_attachments(
        &self,
        rows: &[crate::attachments::AttachmentRecord],
    ) -> Result<(), HistoryError> {
        self.check()?;
        self.inner.upsert_attachments(rows).await
    }
    async fn get_attachment(
        &self,
        agent_id: &str,
        attachment_id: &str,
    ) -> Result<Option<crate::attachments::AttachmentRecord>, HistoryError> {
        self.check()?;
        self.inner.get_attachment(agent_id, attachment_id).await
    }

    async fn upsert_messages(&self, messages: &[HistoryMessage]) -> Result<(), HistoryError> {
        self.check()?;
        if self.panic_on_write.swap(false, Ordering::SeqCst) {
            panic!("injected history store panic");
        }
        self.inner.upsert_messages(messages).await
    }

    async fn upsert_runs(&self, runs: &[RunRecord]) -> Result<(), HistoryError> {
        self.check()?;
        self.inner.upsert_runs(runs).await
    }

    async fn upsert_approvals(&self, approvals: &[ApprovalRequest]) -> Result<(), HistoryError> {
        self.check()?;
        self.inner.upsert_approvals(approvals).await
    }

    async fn get_approval(
        &self,
        approval_id: &str,
    ) -> Result<Option<ApprovalRequest>, HistoryError> {
        self.check()?;
        self.inner.get_approval(approval_id).await
    }

    async fn page_approvals(
        &self,
        query: &ApprovalPageQuery,
    ) -> Result<Vec<ApprovalRequest>, HistoryError> {
        self.check()?;
        self.inner.page_approvals(query).await
    }

    async fn upsert_schedule_runs(&self, fires: &[ScheduleFireRecord]) -> Result<(), HistoryError> {
        self.check()?;
        self.inner.upsert_schedule_runs(fires).await
    }

    async fn page_schedule_runs(
        &self,
        agent_id: &str,
        schedule_id: &str,
        limit: usize,
    ) -> Result<Vec<ScheduleFireRecord>, HistoryError> {
        self.check()?;
        self.inner
            .page_schedule_runs(agent_id, schedule_id, limit)
            .await
    }

    async fn upsert_usage(&self, records: &[UsageRecord]) -> Result<(), HistoryError> {
        self.check()?;
        if self.usage_failing.load(Ordering::SeqCst) {
            return Err(HistoryError::new("injected usage write failure"));
        }
        self.inner.upsert_usage(records).await?;
        self.usage_batches
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(records.len());
        Ok(())
    }

    async fn page_usage(&self, query: &UsagePageQuery) -> Result<Vec<UsageRecord>, HistoryError> {
        self.check()?;
        self.inner.page_usage(query).await
    }

    async fn existing_message_ids(&self, ids: &[String]) -> Result<HashSet<String>, HistoryError> {
        self.check()?;
        let gate = self
            .existence_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(gate) = gate {
            gate.entered.add_permits(1);
            gate.release
                .acquire()
                .await
                .expect("the store gate stays open")
                .forget();
        }
        self.inner.existing_message_ids(ids).await
    }

    async fn get_message(
        &self,
        agent_id: &str,
        session_id: &str,
        message_id: &str,
    ) -> Result<Option<HistoryMessage>, HistoryError> {
        self.check()?;
        let gate = {
            let mut pending = self
                .message_read_gate
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match pending.as_mut() {
                Some((skip, _)) if *skip > 0 => {
                    *skip -= 1;
                    None
                }
                Some(_) => pending.take().map(|(_, gate)| gate),
                None => None,
            }
        };
        if let Some(gate) = gate {
            gate.entered.add_permits(1);
            gate.release
                .acquire()
                .await
                .expect("store gate stays open")
                .forget();
        }
        self.inner
            .get_message(agent_id, session_id, message_id)
            .await
    }

    async fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>, HistoryError> {
        self.check()?;
        self.inner.get_run(run_id).await
    }

    async fn page_messages(
        &self,
        query: &MessagePageQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        self.check()?;
        if self.page_messages_failing.load(Ordering::SeqCst) {
            return Err(HistoryError::new("injected page_messages failure"));
        }
        self.inner.page_messages(query).await
    }

    async fn page_messages_after(
        &self,
        query: &MessageAfterQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        self.check()?;
        if self.page_messages_failing.load(Ordering::SeqCst) {
            return Err(HistoryError::new("injected page_messages failure"));
        }
        self.inner.page_messages_after(query).await
    }

    async fn visible_message_counts(
        &self,
        agent_id: &str,
        session_ids: &[String],
    ) -> Result<HashMap<String, usize>, HistoryError> {
        self.check()?;
        self.inner
            .visible_message_counts(agent_id, session_ids)
            .await
    }

    async fn search_messages(
        &self,
        agent_ids: &[String],
        query: &str,
        limit: usize,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        self.check()?;
        self.inner.search_messages(agent_ids, query, limit).await
    }

    async fn search_sessions(
        &self,
        agent_ids: &[String],
        query: &str,
        limit: usize,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        self.check()?;
        self.inner.search_sessions(agent_ids, query, limit).await
    }

    async fn search_messages_filtered(
        &self,
        query: &FilteredMessageSearchQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        self.check()?;
        let gate = self
            .message_search_gate
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(gate) = gate {
            gate.entered.add_permits(1);
            gate.release
                .acquire()
                .await
                .map_err(|_| HistoryError::new("test release closed"))?
                .forget();
        }
        self.inner.search_messages_filtered(query).await
    }

    async fn search_sessions_filtered(
        &self,
        query: &FilteredSessionSearchQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        self.check()?;
        self.inner.search_sessions_filtered(query).await
    }

    async fn delete_session(&self, agent_id: &str, session_id: &str) -> Result<(), HistoryError> {
        self.check()?;
        self.inner.delete_session(agent_id, session_id).await
    }

    async fn delete_agent(&self, agent_id: &str) -> Result<(), HistoryError> {
        self.check()?;
        self.inner.delete_agent(agent_id).await
    }
}

pub(crate) fn attachment_record(
    id: &str,
    agent: &str,
    session: &str,
) -> crate::attachments::AttachmentRecord {
    use crate::attachments::{AttachmentKind, AttachmentRecord};
    AttachmentRecord {
        id: id.into(),
        agent_id: agent.into(),
        session_id: session.into(),
        name: "notes.txt".into(),
        mime: "text/plain".into(),
        size: 5,
        kind: AttachmentKind::Text,
        path: "uploads/2026-09-23/notes.txt".into(),
        created_at_ms: 100,
        workspace_root: "private-root".into(),
        sha256: "digest".into(),
    }
}

pub(crate) async fn assert_history_store_attachment_conformance(store: &dyn HistoryStore) {
    let agent = format!("agent-{}", uuid::Uuid::new_v4());
    let other = format!("agent-{}", uuid::Uuid::new_v4());
    let a = attachment_record(&uuid::Uuid::new_v4().to_string(), &agent, "chat:a");
    let b = attachment_record(&uuid::Uuid::new_v4().to_string(), &agent, "chat:b");
    let c = attachment_record(&uuid::Uuid::new_v4().to_string(), &other, "chat:a");
    store
        .upsert_attachments(&[a.clone(), b.clone(), c.clone()])
        .await
        .unwrap();
    store
        .upsert_attachments(std::slice::from_ref(&a))
        .await
        .unwrap();
    assert_eq!(
        store.get_attachment(&agent, &a.id).await.unwrap(),
        Some(a.clone())
    );
    assert_eq!(store.get_attachment(&other, &a.id).await.unwrap(), None);
    for changed in [
        attachment_record(&a.id, &other, "chat:a"),
        {
            let mut row = a.clone();
            row.sha256 = "changed".into();
            row
        },
        {
            let mut row = a.clone();
            row.workspace_root = "other-root".into();
            row
        },
        {
            let mut row = a.clone();
            row.path = "uploads/other.txt".into();
            row
        },
        attachment_record(&a.id, &agent, "chat:b"),
    ] {
        assert!(store.upsert_attachments(&[changed]).await.is_err());
        assert_eq!(
            store.get_attachment(&agent, &a.id).await.unwrap(),
            Some(a.clone())
        );
    }
    let pending = attachment_record(&uuid::Uuid::new_v4().to_string(), &agent, "chat:a");
    let mut rewrite = a.clone();
    rewrite.sha256 = "rewritten".into();
    assert!(store
        .upsert_attachments(&[pending.clone(), rewrite])
        .await
        .is_err());
    assert_eq!(
        store.get_attachment(&agent, &pending.id).await.unwrap(),
        None,
        "a conflicting batch must roll back"
    );
    store.delete_session(&agent, "chat:a").await.unwrap();
    assert_eq!(store.get_attachment(&agent, &a.id).await.unwrap(), None);
    assert_eq!(
        store.get_attachment(&agent, &b.id).await.unwrap(),
        Some(b.clone())
    );
    assert_eq!(
        store.get_attachment(&other, &c.id).await.unwrap(),
        Some(c.clone())
    );
    store.delete_agent(&agent).await.unwrap();
    assert_eq!(store.get_attachment(&agent, &b.id).await.unwrap(), None);
    assert_eq!(store.get_attachment(&other, &c.id).await.unwrap(), Some(c));
}

#[tokio::test]
async fn attachment_store_errors_propagate_without_metadata() {
    let store = FlakyHistoryStore::new();
    let row = attachment_record("id", "agent", "session");
    store.set_failing(true);
    assert!(store
        .upsert_attachments(std::slice::from_ref(&row))
        .await
        .is_err());
    assert!(store.get_attachment("agent", "id").await.is_err());
    store.set_failing(false);
    assert!(store.get_attachment("agent", "id").await.unwrap().is_none());
}
