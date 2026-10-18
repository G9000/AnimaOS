//! Bounded in-memory history store for ephemeral mode (spec §13.1): each
//! table keeps at most `EPHEMERAL_HISTORY_MAX_ROWS` rows, dropping the oldest.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use async_trait::async_trait;

use super::{
    search_tokens, searchable_text, text_matches, validate_after_query,
    validate_filtered_message_query, validate_filtered_query, ApprovalPageQuery,
    FilteredMessageSearchQuery, FilteredSessionSearchQuery, HistoryError, HistoryMessage,
    HistoryStore, MessageAfterQuery, MessagePageQuery, UsagePageQuery, EPHEMERAL_HISTORY_MAX_ROWS,
};
use crate::approvals::ApprovalRequest;
use crate::attachments::AttachmentRecord;
use crate::runs::RunRecord;
use crate::schedules::ScheduleFireRecord;
use crate::usage::UsageRecord;

pub(crate) struct MemoryHistoryStore {
    max_rows: usize,
    tables: Mutex<Tables>,
}

#[derive(Default)]
struct Tables {
    next_seq: u64,
    messages: HashMap<String, (u64, HistoryMessage)>,
    message_seqs: BTreeMap<u64, String>,
    runs: HashMap<String, (u64, RunRecord)>,
    run_seqs: BTreeMap<u64, String>,
    approvals: HashMap<String, (u64, ApprovalRequest)>,
    approval_seqs: BTreeMap<u64, String>,
    schedule_runs: HashMap<String, (u64, ScheduleFireRecord)>,
    schedule_run_seqs: BTreeMap<u64, String>,
    usage: HashMap<String, (u64, UsageRecord)>,
    usage_seqs: BTreeMap<u64, String>,
    attachments: HashMap<String, (u64, AttachmentRecord)>,
    attachment_seqs: BTreeMap<u64, String>,
}

impl MemoryHistoryStore {
    pub(crate) fn new() -> Self {
        Self::with_max_rows(EPHEMERAL_HISTORY_MAX_ROWS)
    }

    pub(crate) fn with_max_rows(max_rows: usize) -> Self {
        Self {
            max_rows: max_rows.max(1),
            tables: Mutex::new(Tables::default()),
        }
    }

    fn tables(&self) -> MutexGuard<'_, Tables> {
        self.tables
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Inserts or replaces a row by id; a new row evicts the oldest past the cap.
fn upsert<T>(
    rows: &mut HashMap<String, (u64, T)>,
    order: &mut BTreeMap<u64, String>,
    next_seq: &mut u64,
    id: String,
    value: T,
    max_rows: usize,
) {
    if let Some(existing) = rows.get_mut(&id) {
        existing.1 = value;
        return;
    }
    *next_seq += 1;
    order.insert(*next_seq, id.clone());
    rows.insert(id, (*next_seq, value));
    while rows.len() > max_rows {
        let Some((_, oldest)) = order.pop_first() else {
            break;
        };
        rows.remove(&oldest);
    }
}

fn remove_where<T>(
    rows: &mut HashMap<String, (u64, T)>,
    order: &mut BTreeMap<u64, String>,
    mut matches: impl FnMut(&T) -> bool,
) {
    let doomed = rows
        .iter()
        .filter(|(_, (_, value))| matches(value))
        .map(|(id, (seq, _))| (id.clone(), *seq))
        .collect::<Vec<_>>();
    for (id, seq) in doomed {
        rows.remove(&id);
        order.remove(&seq);
    }
}

#[async_trait]
impl HistoryStore for MemoryHistoryStore {
    fn label(&self) -> &'static str {
        "memory"
    }

    fn is_ephemeral(&self) -> bool {
        true
    }

    async fn upsert_attachments(&self, rows: &[AttachmentRecord]) -> Result<(), HistoryError> {
        let mut guard = self.tables();
        let tables = &mut *guard;
        // Validate the full batch before insertion/eviction so a rejected rewrite
        // cannot partially commit or consume capacity.
        let mut batch = HashMap::new();
        for row in rows {
            if tables
                .attachments
                .get(&row.id)
                .is_some_and(|(_, old)| old != row)
                || batch.insert(&row.id, row).is_some_and(|old| old != row)
            {
                return Err(HistoryError::new(super::ATTACHMENT_ID_CONFLICT));
            }
        }
        for row in rows {
            upsert(
                &mut tables.attachments,
                &mut tables.attachment_seqs,
                &mut tables.next_seq,
                row.id.clone(),
                row.clone(),
                self.max_rows,
            );
        }
        Ok(())
    }
    async fn get_attachment(
        &self,
        agent_id: &str,
        attachment_id: &str,
    ) -> Result<Option<AttachmentRecord>, HistoryError> {
        Ok(self
            .tables()
            .attachments
            .get(attachment_id)
            .filter(|(_, row)| row.agent_id == agent_id)
            .map(|(_, row)| row.clone()))
    }

    async fn upsert_messages(&self, messages: &[HistoryMessage]) -> Result<(), HistoryError> {
        let mut guard = self.tables();
        let tables = &mut *guard;
        for row in messages {
            upsert(
                &mut tables.messages,
                &mut tables.message_seqs,
                &mut tables.next_seq,
                row.message.id.clone(),
                row.clone(),
                self.max_rows,
            );
        }
        Ok(())
    }

    async fn upsert_runs(&self, runs: &[RunRecord]) -> Result<(), HistoryError> {
        let mut guard = self.tables();
        let tables = &mut *guard;
        for run in runs {
            upsert(
                &mut tables.runs,
                &mut tables.run_seqs,
                &mut tables.next_seq,
                run.id.clone(),
                run.clone(),
                self.max_rows,
            );
        }
        Ok(())
    }

    async fn upsert_approvals(&self, approvals: &[ApprovalRequest]) -> Result<(), HistoryError> {
        let mut guard = self.tables();
        let tables = &mut *guard;
        for approval in approvals {
            upsert(
                &mut tables.approvals,
                &mut tables.approval_seqs,
                &mut tables.next_seq,
                approval.id.clone(),
                approval.clone(),
                self.max_rows,
            );
        }
        Ok(())
    }

    async fn get_approval(
        &self,
        approval_id: &str,
    ) -> Result<Option<ApprovalRequest>, HistoryError> {
        Ok(self
            .tables()
            .approvals
            .get(approval_id)
            .map(|(_, approval)| approval.clone()))
    }

    async fn page_approvals(
        &self,
        query: &ApprovalPageQuery,
    ) -> Result<Vec<ApprovalRequest>, HistoryError> {
        let tables = self.tables();
        let mut rows = tables
            .approvals
            .values()
            .map(|(_, approval)| approval)
            .filter(|approval| {
                query
                    .agent_id
                    .as_deref()
                    .is_none_or(|agent_id| approval.agent_id == agent_id)
                    && approval.created_at_ms >= query.since_ms
                    && query.before.as_ref().is_none_or(|(at_ms, id)| {
                        (approval.created_at_ms, approval.id.as_str()) < (*at_ms, id.as_str())
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            (right.created_at_ms, &right.id).cmp(&(left.created_at_ms, &left.id))
        });
        rows.truncate(query.limit);
        Ok(rows)
    }

    async fn upsert_usage(&self, records: &[UsageRecord]) -> Result<(), HistoryError> {
        let mut guard = self.tables();
        let tables = &mut *guard;
        for record in records {
            let mut record = record.clone();
            if let Some((_, existing)) = tables.usage.get(&record.id) {
                record.cost_micros = existing.cost_micros;
                record.pricing_source = existing.pricing_source;
            }
            upsert(
                &mut tables.usage,
                &mut tables.usage_seqs,
                &mut tables.next_seq,
                record.id.clone(),
                record,
                self.max_rows,
            );
        }
        Ok(())
    }

    async fn page_usage(&self, query: &UsagePageQuery) -> Result<Vec<UsageRecord>, HistoryError> {
        let tables = self.tables();
        let mut rows = tables
            .usage
            .values()
            .map(|(_, record)| record)
            .filter(|record| {
                record.created_at_ms >= query.from_ms
                    && record.created_at_ms < query.to_ms
                    && query
                        .agent_id
                        .as_deref()
                        .is_none_or(|agent_id| record.agent_id == agent_id)
                    && query
                        .session_id
                        .as_deref()
                        .is_none_or(|session_id| record.session_id.as_deref() == Some(session_id))
                    && query.before.as_ref().is_none_or(|(at_ms, id)| {
                        (record.created_at_ms, record.id.as_str()) < (*at_ms, id.as_str())
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            (right.created_at_ms, &right.id).cmp(&(left.created_at_ms, &left.id))
        });
        rows.truncate(query.limit);
        Ok(rows)
    }

    async fn existing_message_ids(&self, ids: &[String]) -> Result<HashSet<String>, HistoryError> {
        let tables = self.tables();
        Ok(ids
            .iter()
            .filter(|id| tables.messages.contains_key(*id))
            .cloned()
            .collect())
    }

    async fn get_message(
        &self,
        agent_id: &str,
        session_id: &str,
        message_id: &str,
    ) -> Result<Option<HistoryMessage>, HistoryError> {
        Ok(self
            .tables()
            .messages
            .get(message_id)
            .map(|(_, row)| row)
            .filter(|row| row.agent_id == agent_id && row.session_id == session_id)
            .cloned())
    }

    async fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>, HistoryError> {
        Ok(self.tables().runs.get(run_id).map(|(_, run)| run.clone()))
    }

    async fn page_messages(
        &self,
        query: &MessagePageQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        let tables = self.tables();
        let mut rows = tables
            .messages
            .values()
            .map(|(_, row)| row)
            .filter(|row| {
                row.agent_id == query.agent_id
                    && row.session_id == query.session_id
                    && (query.include_hidden || !row.hidden)
                    && query
                        .before
                        .as_ref()
                        .is_none_or(|before| row.order() < *before)
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| right.order().cmp(&left.order()));
        rows.truncate(query.limit);
        Ok(rows)
    }

    async fn page_messages_after(
        &self,
        query: &MessageAfterQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        validate_after_query(query)?;
        if query.limit == 0 {
            return Ok(Vec::new());
        }
        let tables = self.tables();
        let mut selected: Vec<&HistoryMessage> = Vec::with_capacity(query.limit);
        for (_, row) in tables.messages.values() {
            if row.agent_id != query.agent_id
                || row.session_id != query.session_id
                || (!query.include_hidden && row.hidden)
                || row.order() <= query.after
            {
                continue;
            }
            let order = row.order();
            let position = selected
                .binary_search_by(|existing| existing.order().cmp(&order))
                .unwrap_or_else(|position| position);
            if position >= query.limit {
                continue;
            }
            selected.insert(position, row);
            selected.truncate(query.limit);
        }
        Ok(selected.into_iter().cloned().collect())
    }

    async fn visible_message_counts(
        &self,
        agent_id: &str,
        session_ids: &[String],
    ) -> Result<HashMap<String, usize>, HistoryError> {
        let wanted = session_ids.iter().collect::<HashSet<_>>();
        let mut counts = HashMap::new();
        for (_, row) in self.tables().messages.values() {
            if row.agent_id == agent_id && !row.hidden && wanted.contains(&row.session_id) {
                *counts.entry(row.session_id.clone()).or_insert(0) += 1;
            }
        }
        Ok(counts)
    }

    async fn search_messages(
        &self,
        agent_ids: &[String],
        query: &str,
        limit: usize,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        let tokens = search_tokens(query);
        if tokens.is_empty() || agent_ids.is_empty() {
            return Ok(Vec::new());
        }
        let tables = self.tables();
        let mut rows = tables
            .messages
            .values()
            .map(|(_, row)| row)
            .filter(|row| {
                !row.hidden
                    && agent_ids.contains(&row.agent_id)
                    && text_matches(searchable_text(&row.message), &tokens)
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| right.order().cmp(&left.order()));
        rows.truncate(limit);
        Ok(rows)
    }

    async fn search_sessions(
        &self,
        agent_ids: &[String],
        query: &str,
        limit: usize,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        let tokens = search_tokens(query);
        if tokens.is_empty() || agent_ids.is_empty() {
            return Ok(Vec::new());
        }
        let tables = self.tables();
        let mut newest: HashMap<(String, String), HistoryMessage> = HashMap::new();
        for (_, row) in tables.messages.values() {
            if row.hidden
                || !agent_ids.contains(&row.agent_id)
                || !text_matches(searchable_text(&row.message), &tokens)
            {
                continue;
            }
            let key = (row.agent_id.clone(), row.session_id.clone());
            match newest.get(&key) {
                Some(existing) if existing.order() >= row.order() => {}
                _ => {
                    newest.insert(key, row.clone());
                }
            }
        }
        let mut rows = newest.into_values().collect::<Vec<_>>();
        rows.sort_by(|left, right| right.order().cmp(&left.order()));
        rows.truncate(limit);
        Ok(rows)
    }

    async fn upsert_schedule_runs(&self, fires: &[ScheduleFireRecord]) -> Result<(), HistoryError> {
        let mut guard = self.tables();
        let tables = &mut *guard;
        for fire in fires {
            upsert(
                &mut tables.schedule_runs,
                &mut tables.schedule_run_seqs,
                &mut tables.next_seq,
                fire.id.clone(),
                fire.clone(),
                self.max_rows,
            );
        }
        Ok(())
    }

    async fn page_schedule_runs(
        &self,
        agent_id: &str,
        schedule_id: &str,
        limit: usize,
    ) -> Result<Vec<ScheduleFireRecord>, HistoryError> {
        let tables = self.tables();
        let mut rows = tables
            .schedule_runs
            .values()
            .map(|(_, fire)| fire)
            .filter(|fire| fire.agent_id == agent_id && fire.schedule_id == schedule_id)
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            right
                .fired_at_ms
                .cmp(&left.fired_at_ms)
                .then_with(|| right.id.cmp(&left.id))
        });
        rows.truncate(limit);
        Ok(rows)
    }

    async fn search_messages_filtered(
        &self,
        query: &FilteredMessageSearchQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        validate_filtered_message_query(query)?;
        let tokens = search_tokens(&query.query);
        if tokens.is_empty() || query.session_ids.is_empty() || query.limit == 0 {
            return Ok(Vec::new());
        }
        let allowed: HashSet<_> = query.session_ids.iter().map(String::as_str).collect();
        let excluded: HashSet<_> = query
            .excluded_message_ids
            .iter()
            .map(String::as_str)
            .collect();
        let tables = self.tables();
        let mut rows: Vec<_> = tables
            .messages
            .values()
            .map(|(_, row)| row)
            .filter(|row| {
                row.agent_id == query.agent_id
                    && allowed.contains(row.session_id.as_str())
                    && !excluded.contains(row.message.id.as_str())
                    && !row.hidden
                    && row.message.role != anima_core::MessageRole::System
                    && query
                        .after_ms
                        .is_none_or(|after| row.message.created_at_ms >= after)
                    && query
                        .before_ms
                        .is_none_or(|before| row.message.created_at_ms < before)
                    && text_matches(searchable_text(&row.message), &tokens)
            })
            .collect();
        rows.sort_by(|a, b| b.order().cmp(&a.order()));
        rows.truncate(query.limit);
        Ok(rows.into_iter().cloned().collect())
    }

    async fn search_sessions_filtered(
        &self,
        query: &FilteredSessionSearchQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        validate_filtered_query(query)?;
        let tokens = search_tokens(&query.query);
        if tokens.is_empty() || query.session_ids.is_empty() || query.limit == 0 {
            return Ok(Vec::new());
        }
        let allowed = query
            .session_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let excluded = query
            .excluded_message_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let tables = self.tables();
        let mut newest: HashMap<&str, &HistoryMessage> = HashMap::new();
        for (_, row) in tables.messages.values() {
            if row.agent_id != query.agent_id
                || !allowed.contains(row.session_id.as_str())
                || excluded.contains(row.message.id.as_str())
                || row.hidden
                || row.message.role == anima_core::MessageRole::System
                || query
                    .after_ms
                    .is_some_and(|after| row.message.created_at_ms < after)
                || query
                    .before_ms
                    .is_some_and(|before| row.message.created_at_ms >= before)
                || !text_matches(searchable_text(&row.message), &tokens)
            {
                continue;
            }
            match newest.get(row.session_id.as_str()) {
                Some(existing) if existing.order() >= row.order() => {}
                _ => {
                    newest.insert(&row.session_id, row);
                }
            }
        }
        let mut rows = newest.into_values().collect::<Vec<_>>();
        rows.sort_by(|left, right| right.order().cmp(&left.order()));
        rows.truncate(query.limit);
        Ok(rows.into_iter().cloned().collect())
    }

    async fn delete_session(&self, agent_id: &str, session_id: &str) -> Result<(), HistoryError> {
        let mut guard = self.tables();
        let tables = &mut *guard;
        remove_where(
            &mut tables.attachments,
            &mut tables.attachment_seqs,
            |row| row.agent_id == agent_id && row.session_id == session_id,
        );
        remove_where(&mut tables.messages, &mut tables.message_seqs, |row| {
            row.agent_id == agent_id && row.session_id == session_id
        });
        remove_where(&mut tables.runs, &mut tables.run_seqs, |run| {
            run.agent_id == agent_id && run.session_id == session_id
        });
        remove_where(
            &mut tables.approvals,
            &mut tables.approval_seqs,
            |approval| approval.agent_id == agent_id && approval.session_id == session_id,
        );
        Ok(())
    }

    async fn delete_agent(&self, agent_id: &str) -> Result<(), HistoryError> {
        let mut guard = self.tables();
        let tables = &mut *guard;
        remove_where(
            &mut tables.attachments,
            &mut tables.attachment_seqs,
            |row| row.agent_id == agent_id,
        );
        remove_where(&mut tables.messages, &mut tables.message_seqs, |row| {
            row.agent_id == agent_id
        });
        remove_where(&mut tables.runs, &mut tables.run_seqs, |run| {
            run.agent_id == agent_id
        });
        remove_where(
            &mut tables.approvals,
            &mut tables.approval_seqs,
            |approval| approval.agent_id == agent_id,
        );
        remove_where(
            &mut tables.schedule_runs,
            &mut tables.schedule_run_seqs,
            |fire| fire.agent_id == agent_id,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::conformance::{
        assert_history_store_approval_conformance, assert_history_store_checkin_text_conformance,
        assert_history_store_conformance, assert_history_store_diacritics_conformance,
        assert_history_store_indexed_text_cap_conformance,
        assert_history_store_schedule_run_conformance,
        assert_history_store_session_search_conformance, assert_history_store_usage_conformance,
        history_message,
    };
    use anima_core::MessageRole;

    #[tokio::test]
    async fn memory_store_meets_the_conformance_suite() {
        let store = MemoryHistoryStore::new();
        assert_history_store_conformance(&store).await;
        crate::history::conformance::assert_history_store_attachment_conformance(&store).await;
        assert_history_store_session_search_conformance(&store).await;
        crate::history::conformance::assert_history_store_m41_conformance(&store).await;
        assert_history_store_checkin_text_conformance(&store).await;
        assert_history_store_indexed_text_cap_conformance(&store).await;
        assert_history_store_diacritics_conformance(&store).await;
        assert_history_store_approval_conformance(&store).await;
        assert_history_store_schedule_run_conformance(&store).await;
        assert_history_store_usage_conformance(&store).await;
    }

    #[tokio::test]
    async fn ephemeral_tables_drop_the_oldest_rows_beyond_the_cap() {
        let store = MemoryHistoryStore::with_max_rows(2);
        let rows = (0..3u64)
            .map(|n| {
                history_message(
                    &format!("msg-{n}-{n}"),
                    "agent-1",
                    "chat:a",
                    MessageRole::User,
                    "hello",
                    n,
                )
            })
            .collect::<Vec<_>>();
        let all_ids = rows
            .iter()
            .map(|row| row.message.id.clone())
            .collect::<Vec<_>>();
        store.upsert_messages(&rows).await.unwrap();
        assert_eq!(
            store.existing_message_ids(&all_ids).await.unwrap(),
            HashSet::from(["msg-1-1".to_string(), "msg-2-2".to_string()])
        );

        // Rewriting a kept row neither makes it newer nor evicts another row.
        store.upsert_messages(&rows[1..2]).await.unwrap();
        assert_eq!(store.existing_message_ids(&all_ids).await.unwrap().len(), 2);
        assert!(store.is_ephemeral());
        assert_eq!(store.label(), "memory");
    }
    #[tokio::test]
    async fn attachment_capacity_and_failed_batch_are_bounded() {
        use crate::history::conformance::attachment_record;
        let store = MemoryHistoryStore::with_max_rows(2);
        let a = attachment_record("a", "agent", "chat:a");
        let b = attachment_record("b", "agent", "chat:a");
        let c = attachment_record("c", "agent", "chat:a");
        store
            .upsert_attachments(&[a.clone(), b.clone(), c.clone()])
            .await
            .unwrap();
        assert!(store.get_attachment("agent", "a").await.unwrap().is_none());
        store
            .upsert_attachments(std::slice::from_ref(&b))
            .await
            .unwrap();
        let d = attachment_record("d", "agent", "chat:a");
        let mut changed = c.clone();
        changed.sha256 = "different".into();
        assert!(store.upsert_attachments(&[d, changed]).await.is_err());
        assert_eq!(store.get_attachment("agent", "b").await.unwrap(), Some(b));
        assert_eq!(store.get_attachment("agent", "c").await.unwrap(), Some(c));
        assert!(store.get_attachment("agent", "d").await.unwrap().is_none());
    }
}
