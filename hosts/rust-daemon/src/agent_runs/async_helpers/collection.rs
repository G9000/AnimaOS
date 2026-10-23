//! Read-only joins over existing immutable helper assignments. Live events are
//! wakeups only; readiness always comes from committed terminal evidence.
use std::collections::HashSet;
use std::time::Duration;

use anima_core::{DataValue, ToolCall};
use futures::future::try_join_all;
use serde_json::{json, Value};
use tokio::time::Instant;

use super::super::AgentRunCoordinator;
use super::{active_caller, bounded_text};
use crate::runs::{RunLink, RunRecord, RunStatus};

const MAX_COLLECTION_BYTES: usize = 32 * 1024;
const SNAPSHOT_READ_TIMEOUT: Duration = Duration::from_secs(2);
const FALLBACK_POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy)]
enum JoinMode {
    Any,
    All,
}

impl JoinMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::All => "all",
        }
    }
}

fn arguments(call: &ToolCall) -> Result<(Vec<String>, JoinMode, Duration), String> {
    if call
        .args
        .keys()
        .any(|key| !matches!(key.as_str(), "run_ids" | "mode" | "timeout_ms"))
    {
        return Err("Unsupported helper collection argument".into());
    }
    let Some(DataValue::Array(values)) = call.args.get("run_ids") else {
        return Err("run_ids must contain between 1 and 8 unique helper run handles".into());
    };
    if !(1..=8).contains(&values.len()) {
        return Err("run_ids must contain between 1 and 8 unique helper run handles".into());
    }
    let mut seen = HashSet::new();
    let mut ids = Vec::with_capacity(values.len());
    for value in values {
        let DataValue::String(id) = value else {
            return Err("run_ids must contain between 1 and 8 unique helper run handles".into());
        };
        if id.trim().is_empty()
            || id.len() > 200
            || id.chars().any(char::is_control)
            || !seen.insert(id.clone())
        {
            return Err("run_ids must contain between 1 and 8 unique helper run handles".into());
        }
        ids.push(id.clone());
    }
    let mode = match call.args.get("mode") {
        None => JoinMode::All,
        Some(DataValue::String(value)) if value == "any" => JoinMode::Any,
        Some(DataValue::String(value)) if value == "all" => JoinMode::All,
        _ => return Err("mode must be any or all".into()),
    };
    let timeout_ms = match call.args.get("timeout_ms") {
        None => 10_000,
        Some(DataValue::Number(value))
            if value.is_finite() && value.fract() == 0.0 && (0.0..=30_000.0).contains(value) =>
        {
            *value as u64
        }
        _ => return Err("timeout_ms must be an integer between 0 and 30000".into()),
    };
    Ok((ids, mode, Duration::from_millis(timeout_ms)))
}

impl AgentRunCoordinator {
    pub(crate) async fn collect_async_helpers(
        &self,
        owner: String,
        parent: RunLink,
        call: ToolCall,
        caller_cancel: Option<anima_core::CancelSignal>,
    ) -> Result<String, String> {
        let (ids, mode, timeout) = arguments(&call)?;
        // Subscribe before reading the ledger so a terminal publication cannot
        // fall between our snapshot and the first wait. No coordinator/control
        // plane transaction is acquired by this read-only operation.
        let (cancel, mut events) = {
            let guard = tokio::select! {
                biased;
                error = caller_cancelled(caller_cancel.as_ref()) => return Err(error),
                guard = tokio::time::timeout(SNAPSHOT_READ_TIMEOUT, self.state.read()) => {
                    guard.map_err(|_| "Helper collection state is unavailable".to_string())?
                }
            };
            let control = guard
                .live
                .runs()
                .control(&parent.run_id)
                .ok_or("Helper collection requires the original active run")?;
            if control.cancel.is_cancelled() {
                return Err(control.cancel.run_error().into());
            }
            active_caller(&guard, &owner, &parent, "wait_helper_runs")?;
            (control.cancel, guard.live.subscribe(&owner).ok())
        };
        let deadline = Instant::now() + timeout;
        let mut first_snapshot = true;
        let mut previous: Option<Vec<Value>> = None;
        loop {
            if cancel.is_cancelled() {
                return Err(cancel.run_error().into());
            }
            // The first snapshot has its own finite read allowance, including
            // timeout=0. Later reads share the remaining completion wait.
            let read_deadline = if first_snapshot {
                Instant::now() + SNAPSHOT_READ_TIMEOUT
            } else {
                deadline.min(Instant::now() + SNAPSHOT_READ_TIMEOUT)
            };
            {
                let guard = tokio::select! {
                    biased;
                    error = caller_cancelled(caller_cancel.as_ref()) => return Err(error),
                    _ = cancel.cancelled() => return Err(cancel.run_error().into()),
                    guard = tokio::time::timeout_at(read_deadline, self.state.read()) => {
                        guard.map_err(|_| "Helper collection state is unavailable".to_string())?
                    }
                };
                active_caller(&guard, &owner, &parent, "wait_helper_runs")?;
            }
            // timeout=0 still reads one bounded snapshot; it never waits for a
            // helper to finish. The local/history read itself has a finite cap.
            let snapshot = tokio::select! {
                biased;
                error = caller_cancelled(caller_cancel.as_ref()) => return Err(error),
                _ = cancel.cancelled() => return Err(cancel.run_error().into()),
                snapshot = tokio::time::timeout_at(read_deadline, self.helper_collection_snapshot(&owner, &ids)) => snapshot,
            };
            let runs = match snapshot {
                Ok(Ok(runs)) => runs,
                Ok(Err(error)) => return Err(error),
                Err(_) if previous.is_some() => {
                    return collection_answer(previous.take().unwrap(), mode, true);
                }
                Err(_) => return Err("Helper collection history is unavailable".into()),
            };
            first_snapshot = false;
            let ready = runs.iter().filter(|run| run["ready"] == true).count();
            let condition_met = match mode {
                JoinMode::Any => ready > 0,
                JoinMode::All => ready == ids.len(),
            };
            if condition_met || Instant::now() >= deadline {
                return collection_answer(runs, mode, !condition_met);
            }
            previous = Some(runs);
            if let Some(subscription) = events.as_mut() {
                // Lag means re-read immediately. Closing the event stream or
                // exhausting its subscriber cap falls back to bounded polling.
                let delivery = tokio::select! {
                    biased;
                    error = caller_cancelled(caller_cancel.as_ref()) => return Err(error),
                    _ = cancel.cancelled() => return Err(cancel.run_error().into()),
                    // Re-enter the loop to revalidate the caller before
                    // returning the last committed snapshot at the deadline.
                    _ = tokio::time::sleep_until(deadline) => continue,
                    delivery = subscription.next() => delivery,
                };
                if delivery.is_none() {
                    events = None;
                }
            } else {
                tokio::select! {
                    biased;
                    error = caller_cancelled(caller_cancel.as_ref()) => return Err(error),
                    _ = cancel.cancelled() => return Err(cancel.run_error().into()),
                    _ = tokio::time::sleep_until(deadline.min(Instant::now() + FALLBACK_POLL)) => {}
                }
            }
        }
    }

    async fn helper_collection_snapshot(
        &self,
        owner: &str,
        ids: &[String],
    ) -> Result<Vec<Value>, String> {
        // Validate ownership of every requested identity before assembling any
        // response. Unknown and foreign assignments share the same refusal.
        let records =
            try_join_all(ids.iter().map(|id| self.owned_helper_record(owner, id))).await?;
        try_join_all(records.into_iter().map(|record| async move {
            let terminal = record.status.is_terminal();
            let committed = if terminal && !record.terminal_published {
                // In-process publication is deliberately not restored. An
                // exact durable history witness permits read-only collection,
                // without reviving any automatic callback rights.
                let history = self.state.read().await.history.store();
                history
                    .get_run(&record.id)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|saved| same_terminal_evidence(&record, &saved))
            } else {
                terminal
            };
            let result = if committed && record.status == RunStatus::Completed {
                self.committed_helper_text(&record).await
            } else {
                None
            };
            let ready = committed && (record.status != RunStatus::Completed || result.is_some());
            let result_bytes = result.as_ref().map_or(0, String::len);
            let preview = result.as_deref().map(|text| bounded_text(text, 2048));
            let validation = result.as_deref().and_then(|text| super::super::helper_options::result_validation(record.async_helper.as_ref().and_then(|helper| helper.options.as_ref()), text));
            Ok(json!({
                "outputValidation": validation,
                "route": record.async_helper.as_ref().and_then(|helper| helper.options.as_ref()).and_then(|options| options.route.as_ref()),
                "runId": record.id,
                "agentId": record.agent_id,
                "sessionId": record.session_id,
                "status": record.status.as_str(),
                "terminal": terminal,
                "ready": ready,
                "replyMessageId": record.reply_message_id,
                "resultText": preview,
                "resultBytes": result_bytes,
                "resultTruncated": result_bytes > 2048,
                "resultUnavailable": record.status == RunStatus::Completed && result.is_none(),
                "error": record.error.as_ref().map(|error| json!({
                    "code": bounded_text(&error.code, 128),
                    "message": bounded_text(&error.message, 1024),
                    "truncated": error.code.len() > 128 || error.message.len() > 1024,
                })),
            }))
        }))
        .await
    }
}

async fn caller_cancelled(signal: Option<&anima_core::CancelSignal>) -> String {
    match signal {
        Some(signal) => {
            signal.cancelled().await;
            signal.run_error().to_string()
        }
        None => std::future::pending().await,
    }
}

fn same_terminal_evidence(current: &RunRecord, saved: &RunRecord) -> bool {
    saved.status.is_terminal()
        && saved.id == current.id
        && saved.agent_id == current.agent_id
        && saved.session_id == current.session_id
        && saved.status == current.status
        && saved.finished_at_ms == current.finished_at_ms
        && saved.parent_run_id == current.parent_run_id
        && saved.reply_message_id == current.reply_message_id
        && saved.reply_sha256 == current.reply_sha256
        && saved
            .error
            .as_ref()
            .map(|error| (&error.code, &error.message))
            == current
                .error
                .as_ref()
                .map(|error| (&error.code, &error.message))
        && saved.async_helper.as_ref().map(|helper| {
            (
                &helper.owner_agent_id,
                &helper.parent_run_id,
                &helper.launch_fingerprint,
            )
        }) == current.async_helper.as_ref().map(|helper| {
            (
                &helper.owner_agent_id,
                &helper.parent_run_id,
                &helper.launch_fingerprint,
            )
        })
}

fn collection_answer(runs: Vec<Value>, mode: JoinMode, timed_out: bool) -> Result<String, String> {
    let ready: Vec<_> = runs
        .iter()
        .filter(|run| run["ready"] == true)
        .map(|run| run["runId"].clone())
        .collect();
    let pending: Vec<_> = runs
        .iter()
        .filter(|run| run["ready"] != true)
        .map(|run| run["runId"].clone())
        .collect();
    let condition_met = match mode {
        JoinMode::Any => !ready.is_empty(),
        JoinMode::All => pending.is_empty(),
    };
    let truncated = runs
        .iter()
        .any(|run| run["resultTruncated"] == true || run["error"]["truncated"] == true);
    let mut answer = json!({
        "mode": mode.as_str(),
        "conditionMet": condition_met,
        "timedOut": timed_out && !condition_met,
        "readyRunIds": ready,
        "pendingRunIds": pending,
        "runs": runs,
        "truncated": truncated,
    });
    loop {
        let text = answer.to_string();
        if text.len() <= MAX_COLLECTION_BYTES {
            return Ok(text);
        }
        let mut shortened = false;
        for run in answer["runs"].as_array_mut().expect("collection runs") {
            if let Some(text) = run["resultText"].as_str().filter(|text| !text.is_empty()) {
                let preview = bounded_text(text, text.len() / 2);
                run["resultText"] = json!(preview);
                run["resultTruncated"] = json!(true);
                shortened = true;
            }
            if let Some(message) = run["error"]["message"]
                .as_str()
                .filter(|text| !text.is_empty())
            {
                let preview = bounded_text(message, message.len() / 2);
                run["error"]["message"] = json!(preview);
                run["error"]["truncated"] = json!(true);
                shortened = true;
            }
        }
        answer["truncated"] = json!(true);
        if !shortened {
            return Err("Helper collection identities exceed the response bound".into());
        }
    }
}
