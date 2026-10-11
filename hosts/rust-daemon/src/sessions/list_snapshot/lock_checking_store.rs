//! History fixture that asserts the daemon state is unlocked on every call.
use crate::approvals::ApprovalRequest;
use crate::history::*;
use crate::runs::RunRecord;
use crate::schedules::ScheduleFireRecord;
use crate::state::DaemonState;
use crate::usage::UsageRecord;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicUsize;
use std::sync::Weak;
use tokio::sync::RwLock;

pub(super) struct LockCheckingStore {
    pub(super) state: Weak<RwLock<DaemonState>>,
    pub(super) inner: MemoryHistoryStore,
    pub(super) calls: AtomicUsize,
}
#[async_trait]
impl HistoryStore for LockCheckingStore {
    fn label(&self) -> &'static str {
        "lock-checking"
    }
    async fn upsert_attachments(
        &self,
        rows: &[crate::attachments::AttachmentRecord],
    ) -> Result<(), HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.upsert_attachments(rows).await
    }
    async fn get_attachment(
        &self,
        agent_id: &str,
        attachment_id: &str,
    ) -> Result<Option<crate::attachments::AttachmentRecord>, HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.get_attachment(agent_id, attachment_id).await
    }
    async fn upsert_messages(&self, messages: &[HistoryMessage]) -> Result<(), HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.upsert_messages(messages).await
    }
    async fn upsert_runs(&self, runs: &[RunRecord]) -> Result<(), HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.upsert_runs(runs).await
    }
    async fn upsert_approvals(&self, approvals: &[ApprovalRequest]) -> Result<(), HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.upsert_approvals(approvals).await
    }
    async fn get_approval(
        &self,
        approval_id: &str,
    ) -> Result<Option<ApprovalRequest>, HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.get_approval(approval_id).await
    }
    async fn page_approvals(
        &self,
        query: &ApprovalPageQuery,
    ) -> Result<Vec<ApprovalRequest>, HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.page_approvals(query).await
    }
    async fn upsert_schedule_runs(&self, fires: &[ScheduleFireRecord]) -> Result<(), HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.upsert_schedule_runs(fires).await
    }
    async fn page_schedule_runs(
        &self,
        agent_id: &str,
        schedule_id: &str,
        limit: usize,
    ) -> Result<Vec<ScheduleFireRecord>, HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner
            .page_schedule_runs(agent_id, schedule_id, limit)
            .await
    }
    async fn upsert_usage(&self, records: &[UsageRecord]) -> Result<(), HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.upsert_usage(records).await
    }
    async fn page_usage(&self, query: &UsagePageQuery) -> Result<Vec<UsageRecord>, HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.page_usage(query).await
    }
    async fn existing_message_ids(&self, ids: &[String]) -> Result<HashSet<String>, HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.existing_message_ids(ids).await
    }
    async fn get_message(
        &self,
        agent_id: &str,
        session_id: &str,
        message_id: &str,
    ) -> Result<Option<HistoryMessage>, HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner
            .get_message(agent_id, session_id, message_id)
            .await
    }
    async fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>, HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.get_run(run_id).await
    }
    async fn page_messages(
        &self,
        query: &MessagePageQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.page_messages(query).await
    }
    async fn visible_message_counts(
        &self,
        agent_id: &str,
        session_ids: &[String],
    ) -> Result<HashMap<String, usize>, HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.search_messages(agent_ids, query, limit).await
    }
    async fn search_sessions(
        &self,
        agent_ids: &[String],
        query: &str,
        limit: usize,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.search_sessions(agent_ids, query, limit).await
    }
    async fn delete_session(&self, agent_id: &str, session_id: &str) -> Result<(), HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.delete_session(agent_id, session_id).await
    }
    async fn delete_agent(&self, agent_id: &str) -> Result<(), HistoryError> {
        assert!(
            self.state.upgrade().unwrap().try_write().is_ok(),
            "history must be called after state guard release"
        );
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.delete_agent(agent_id).await
    }
}
