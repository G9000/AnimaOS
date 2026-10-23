//! Secondary-call custody shares the existing control-plane transaction.

use super::*;
use anima_core::primitives::now_millis;

impl HistoryService {
    /// Validate independently of live agents/sessions: usage outlives deletion.
    pub(crate) fn validate_pending_usage(records: &[UsageRecord]) -> Result<(), String> {
        if records.len() > USAGE_QUEUE_MAX {
            return Err("pending usage exceeds its row bound".into());
        }
        let mut ids = HashSet::new();
        for record in records {
            let bounded = |value: &str| {
                !value.trim().is_empty()
                    && value.len() <= 1024
                    && !value.chars().any(char::is_control)
                    && !crate::memory_text::has_hidden_text(value)
            };
            if !bounded(&record.id)
                || !bounded(&record.agent_id)
                || !bounded(&record.provider)
                || !bounded(&record.model)
                || record.session_id.as_deref().is_some_and(|id| !bounded(id))
                || record.run_id.as_deref().is_some_and(|id| !bounded(id))
                || record.created_at_ms > i64::MAX as u64
                || !ids.insert(&record.id)
            {
                return Err("pending usage contains invalid, oversized or duplicate rows".into());
            }
        }
        Ok(())
    }

    pub(crate) fn restore_usage(&self, records: Vec<UsageRecord>) -> Result<(), String> {
        Self::validate_pending_usage(&records)?;
        if records.len() > self.max_usage {
            return Err("pending usage exceeds the configured row bound".into());
        }
        self.usage().rows = records.into();
        self.wake.notify_one();
        Ok(())
    }

    pub(crate) fn pending_usage(&self) -> Vec<UsageRecord> {
        self.usage().rows.iter().cloned().collect()
    }

    /// Prepare rows under the control-plane transaction; the caller must save
    /// before reporting durable acceptance. Returns only newly inserted IDs,
    /// so a failed save can roll back its own additions without removing retries.
    pub(crate) fn enqueue_usage(
        &self,
        records: Vec<UsageRecord>,
    ) -> Result<Vec<String>, HistoryError> {
        let result = (|| {
            if records.len() > USAGE_QUEUE_MAX {
                return Err(HistoryError::new(
                    "pending usage batch exceeds its row bound",
                ));
            }
            let mut usage = self.usage();
            let mut known = usage
                .rows
                .iter()
                .map(|row| (row.id.as_str(), row))
                .collect::<HashMap<_, _>>();
            let mut added = Vec::new();
            for record in &records {
                if let Some(existing) = known.get(record.id.as_str()) {
                    if **existing != *record {
                        return Err(HistoryError::new(
                            "pending usage ID is already bound to different data",
                        ));
                    }
                } else {
                    known.insert(record.id.as_str(), record);
                    added.push(record.clone());
                }
            }
            Self::validate_pending_usage(&added).map_err(HistoryError::new)?;
            if usage.rows.len().saturating_add(added.len()) > self.max_usage {
                return Err(HistoryError::new(
                    "pending usage capacity reached; new rows were not accepted",
                ));
            }
            let ids = added.iter().map(|row| row.id.clone()).collect();
            usage.rows.extend(added);
            Ok(ids)
        })();
        if let Err(error) = &result {
            self.note_usage_failure(error);
        }
        result
    }

    pub(crate) fn rollback_usage(&self, ids: &[String]) {
        let ids = ids.iter().map(String::as_str).collect::<HashSet<_>>();
        self.usage()
            .rows
            .retain(|row| !ids.contains(row.id.as_str()));
    }

    fn note_usage_failure(&self, error: &HistoryError) {
        self.record_result(&Err(HistoryError::new(error)), now_millis());
    }

    /// Once started, detached ownership finishes the save even if an HTTP
    /// request or a title worker is cancelled while awaiting acknowledgement.
    pub(crate) async fn enqueue_usage_durable(
        self: &Arc<Self>,
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        records: Vec<UsageRecord>,
    ) -> Result<UsageAcceptance, HistoryError> {
        let (history, state) = (Arc::clone(self), Arc::clone(state));
        tokio::spawn(async move {
            let _transaction = transactions.lock().await;
            let (inserted, persist, acceptance) = {
                let mut daemon = state.write().await;
                if !Arc::ptr_eq(&history, &daemon.history) {
                    return Err(HistoryError::new(
                        "pending usage service changed before admission",
                    ));
                }
                let inserted = history.enqueue_usage(records)?;
                let acceptance = if daemon.control_plane_store.is_some() && !history.is_ephemeral()
                {
                    UsageAcceptance::Durable
                } else {
                    UsageAcceptance::Ephemeral
                };
                (inserted, daemon.control_plane_persist_request(), acceptance)
            };
            if let Err(error) = persist.save().await {
                // An uncertain save may already contain these IDs. Retain them
                // rather than let a later save erase possibly durable custody.
                if !crate::control_plane_store::is_durability_uncertain(&error) {
                    history.rollback_usage(&inserted);
                }
                let error = HistoryError::new(format!("pending usage could not be saved: {error}"));
                history.note_usage_failure(&error);
                return Err(error);
            }
            history.wake.notify_one();
            Ok(acceptance)
        })
        .await
        .map_err(|error| HistoryError::new(format!("pending usage save task exited: {error}")))?
    }

    pub(super) async fn write_usage(
        &self,
        state: &SharedDaemonState,
        transactions: &Mutex<()>,
        report: &mut FlushReport,
    ) -> Result<(), HistoryError> {
        loop {
            let records = {
                let _transaction = transactions.lock().await;
                self.usage()
                    .rows
                    .iter()
                    .take(HISTORY_USAGE_BATCH)
                    .cloned()
                    .collect::<Vec<_>>()
            };
            if records.is_empty() {
                return Ok(());
            }
            // No control-plane lock spans network storage. Later enqueues can
            // proceed; only these immutable IDs are acknowledged below.
            self.store.upsert_usage(&records).await?;
            let _transaction = transactions.lock().await;
            let persist = {
                let mut daemon = state.write().await;
                let ids = records.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
                self.rollback_usage(&ids);
                daemon.control_plane_persist_request()
            };
            if let Err(error) = persist.save().await {
                let mut usage = self.usage();
                let existing = usage
                    .rows
                    .iter()
                    .map(|row| row.id.clone())
                    .collect::<HashSet<_>>();
                for record in records.into_iter().rev() {
                    if !existing.contains(&record.id) {
                        usage.rows.push_front(record);
                    }
                }
                return Err(HistoryError::new(format!(
                    "pending usage acknowledgement could not be saved: {error}"
                )));
            }
            report.usage += records.len();
        }
    }
}
