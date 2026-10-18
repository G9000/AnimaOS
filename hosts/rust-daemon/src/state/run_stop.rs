//! Stop requests (spec §4.6): what a stop changes, applied under the
//! control-plane transaction and saved before any run is signalled.

use std::collections::HashSet;

use super::{ApprovalUndo, DaemonState, Settlement};
use crate::approvals::ApprovalRequest;
use crate::connectors::{
    InboundProcessingState, OutboundDeliveryState, TelegramInboundRecord, TelegramOutboundRecord,
};
use crate::jobs::{AgentJobRecord, AgentJobStatus};
use crate::runs::{
    RunError, RunRecord, RunSource, RunStatus, RunStopRequest, RUN_STOPPED, STOPPED_BY_OWNER,
};

/// Everything a stop changed, as it was, so a failed save can put it back.
#[derive(Debug, Default)]
pub(crate) struct RunStopUndo {
    pub(crate) runs: Vec<RunRecord>,
    pub(crate) inbound: Vec<TelegramInboundRecord>,
    pub(crate) outbound: Vec<TelegramOutboundRecord>,
    pub(crate) jobs: Vec<AgentJobRecord>,
    pub(crate) discord_inbound: Vec<crate::connectors::discord::DiscordInboundRecord>,
    pub(crate) discord_outbound: Vec<crate::connectors::discord::DiscordOutboundRecord>,
    /// Approvals the stop settled, as they were (spec §4.6).
    pub(crate) approvals: Vec<ApprovalUndo>,
    /// Terminal helpers can still own a pending continuation. Restore this
    /// metadata separately because ordinary stop undo only restores stop flags.
    pub(crate) helper_metadata: Vec<(
        String,
        Option<RunStopRequest>,
        crate::runs::AsyncHelper,
        bool,
    )>,
}

impl RunStopUndo {
    /// Nothing changed, so nothing needs saving (a repeated stop).
    pub(crate) fn is_empty(&self) -> bool {
        self.runs.is_empty()
            && self.inbound.is_empty()
            && self.outbound.is_empty()
            && self.discord_inbound.is_empty()
            && self.discord_outbound.is_empty()
            && self.jobs.is_empty()
            && self.approvals.is_empty()
            && self.helper_metadata.is_empty()
    }
}

/// A planned stop: saved first, then every control in `signal` is cancelled.
#[derive(Debug)]
pub(crate) struct RunStopPlan {
    /// The stopped run as it is now.
    pub(crate) run: RunRecord,
    /// The run and the queued and in-flight runs it started, transitively.
    pub(crate) signal: Vec<String>,
    /// Queued runs cancelled outright, as they are now.
    pub(crate) cancelled: Vec<RunRecord>,
    /// Approvals this stop settled as `stopped`, as they now are. A run
    /// they moved back to `running` is not announced as started: its
    /// terminal event follows (controller ruling m6).
    pub(crate) approvals: Vec<ApprovalRequest>,
    pub(crate) undo: RunStopUndo,
}

impl DaemonState {
    pub(crate) fn request_helper_stop(
        &mut self,
        owner: &str,
        run_id: &str,
        now_ms: u64,
    ) -> Option<RunStopPlan> {
        let original = self.runs.get(run_id)?.clone();
        let helper = original.async_helper.as_ref()?.clone();
        if helper.owner_agent_id != owner {
            return None;
        }
        let continuation_id = helper.completion.run_id().map(str::to_string);
        if let Some(completion) = continuation_id.as_deref().and_then(|id| self.runs.get(id)) {
            if completion.agent_id != owner
                || !completion.helper_completion.as_ref().is_some_and(|origin| {
                    origin.helper_run_id == run_id
                        && origin.launch_parent_run_id == helper.parent_run_id
                })
            {
                return None;
            }
        }
        let mut plan = self.request_run_stop(&original.agent_id, run_id, now_ms)?;
        let target_completion = if matches!(
            helper.completion,
            crate::runs::HelperCompletionState::Manual
        ) {
            crate::runs::HelperCompletionState::Manual
        } else {
            crate::runs::HelperCompletionState::Suppressed {
                reason: "Stopped through stop_helper".into(),
                run_id: continuation_id.clone(),
            }
        };
        if original.stop.is_none() || helper.completion != target_completion {
            plan.undo.helper_metadata.push((
                original.id.clone(),
                original.stop.clone(),
                helper,
                original.mirrored,
            ));
            let current = self.runs.get_mut(run_id).expect("owned helper exists");
            current.stop.get_or_insert(RunStopRequest {
                requested_at_ms: now_ms,
            });
            current
                .async_helper
                .as_mut()
                .expect("typed helper exists")
                .completion = target_completion;
            current.mirrored = false;
        }
        if let Some(id) = continuation_id {
            if let Some(other) = self.request_run_stop(owner, &id, now_ms) {
                plan.signal.extend(other.signal);
                plan.cancelled.extend(other.cancelled);
                plan.approvals.extend(other.approvals);
                plan.undo.runs.extend(other.undo.runs);
                plan.undo.inbound.extend(other.undo.inbound);
                plan.undo.outbound.extend(other.undo.outbound);
                plan.undo.jobs.extend(other.undo.jobs);
                plan.undo.discord_inbound.extend(other.undo.discord_inbound);
                plan.undo
                    .discord_outbound
                    .extend(other.undo.discord_outbound);
                plan.undo.approvals.extend(other.undo.approvals);
                plan.undo.helper_metadata.extend(other.undo.helper_metadata);
            }
        }
        plan.run =
            self.with_live_tools(self.runs.get(run_id).expect("owned helper exists").clone());
        Some(plan)
    }

    /// Plans stopping `run_id` of `agent_id` and the queued and in-flight runs
    /// it started, transitively (spec §4.6: helper runs are stopped too). A
    /// queued run is cancelled outright; a running one records the stop once.
    /// `None` when the ledger has no such run of this agent.
    pub(crate) fn request_run_stop(
        &mut self,
        agent_id: &str,
        run_id: &str,
        now_ms: u64,
    ) -> Option<RunStopPlan> {
        let record = self
            .runs
            .get(run_id)
            .filter(|record| record.agent_id == agent_id)?
            .clone();
        if record.status.is_terminal() {
            let mut undo = RunStopUndo::default();
            // A Telegram reply committed but not delivered yet is never sent.
            if record.source == RunSource::Telegram {
                self.suppress_undelivered_reply(&record, &mut undo);
            }
            if record.source == RunSource::Discord {
                self.suppress_discord_reply(&record, &mut undo);
            }
            let mut plan = RunStopPlan {
                run: record,
                signal: Vec::new(),
                cancelled: Vec::new(),
                approvals: Vec::new(),
                undo,
            };
            self.suppress_helper_callbacks(&mut plan, &HashSet::from([run_id.to_string()]));
            // A terminal launching run can still have a queued/running
            // continuation. Suppression and its stop share this save.
            let callbacks: Vec<_> = self
                .runs
                .active_records()
                .into_iter()
                .filter(|record| {
                    record
                        .helper_completion
                        .as_ref()
                        .is_some_and(|origin| origin.launch_parent_run_id == run_id)
                })
                .map(|record| record.id.clone())
                .collect();
            for id in callbacks {
                if let Some(other) = self.request_run_stop(agent_id, &id, now_ms) {
                    plan.signal.extend(other.signal);
                    plan.cancelled.extend(other.cancelled);
                    plan.approvals.extend(other.approvals);
                    plan.undo.runs.extend(other.undo.runs);
                    plan.undo.approvals.extend(other.undo.approvals);
                    plan.undo.helper_metadata.extend(other.undo.helper_metadata);
                }
            }
            return Some(plan);
        }
        let mut targets = vec![record.id.clone()];
        let mut seen: HashSet<String> = targets.iter().cloned().collect();
        let mut index = 0;
        while index < targets.len() {
            let parent = targets[index].clone();
            for child in self.runs.active_records() {
                if child.parent_run_id.as_deref() == Some(parent.as_str())
                    && seen.insert(child.id.clone())
                {
                    targets.push(child.id.clone());
                }
            }
            index += 1;
        }
        let mut plan = RunStopPlan {
            run: record,
            signal: targets.clone(),
            cancelled: Vec::new(),
            approvals: Vec::new(),
            undo: RunStopUndo::default(),
        };
        let mut sources: Vec<(RunSource, String)> = Vec::new();
        let mut stopping: Vec<String> = Vec::new();
        for id in &targets {
            let Some(target) = self.runs.get_mut(id) else {
                continue;
            };
            match target.status {
                RunStatus::Queued => {
                    plan.undo.runs.push(target.clone());
                    target.stop = Some(RunStopRequest {
                        requested_at_ms: now_ms,
                    });
                    target.finish(
                        RunStatus::Cancelled,
                        Some(RunError::new(RUN_STOPPED, STOPPED_BY_OWNER)),
                        now_ms,
                    );
                    plan.cancelled.push(target.clone());
                }
                RunStatus::Running | RunStatus::AwaitingApproval if target.stop.is_none() => {
                    plan.undo.runs.push(target.clone());
                    target.stop = Some(RunStopRequest {
                        requested_at_ms: now_ms,
                    });
                    if let Some(source_ref) = target.source_ref.clone() {
                        sources.push((target.source, source_ref));
                    }
                    stopping.push(id.clone());
                }
                _ => {}
            }
        }
        self.suppress_helper_callbacks(&mut plan, &seen);
        for id in &targets {
            let source = self
                .runs
                .get(id)
                .and_then(|record| record.helper_completion.as_ref())
                .and_then(|origin| self.runs.get(&origin.helper_run_id))
                .cloned();
            if let Some(source) = source {
                if let Some(helper) = source.async_helper.as_ref().filter(|helper| matches!(&helper.completion, crate::runs::HelperCompletionState::Queued { run_id } if run_id == id)) {
                    plan.undo.helper_metadata.push((source.id.clone(), source.stop.clone(), helper.clone(), source.mirrored));
                    self.runs.settle_helper_origin(id);
                }
            }
        }
        // Source records saved with the stop, before any signal (spec §4.6).
        for (source, source_ref) in sources {
            match source {
                RunSource::Telegram => self.stop_inbound(&source_ref, &mut plan.undo),
                RunSource::Discord => self.stop_discord_inbound(&source_ref, &mut plan.undo),
                RunSource::Job => self.stop_job(&source_ref, now_ms, &mut plan.undo),
                _ => {}
            }
        }
        // Spec §4.6: a stopped run's pending approvals resolve as `stopped`
        // in this same save, so a decision that comes later finds them
        // settled and the waiting calls hear it from the stop.
        for run_id in &stopping {
            for approval_id in self.approvals.pending_ids_for_run(run_id) {
                if let Ok(settled) = self.settle_approval(&approval_id, Settlement::Stopped, now_ms)
                {
                    plan.approvals.push(settled.approval);
                    plan.undo.approvals.push(settled.undo);
                }
            }
        }
        if let Some(current) = self.runs.get(run_id).cloned() {
            plan.run = self.with_live_tools(current);
        }
        Some(plan)
    }

    fn suppress_helper_callbacks(&mut self, plan: &mut RunStopPlan, parents: &HashSet<String>) {
        let mut seen = HashSet::new();
        let candidates: Vec<_> = parents
            .iter()
            .flat_map(|id| self.runs.helper_event_records(id))
            .collect();
        for current in candidates {
            if !seen.insert(current.id.clone()) {
                continue;
            }
            let original = plan
                .undo
                .runs
                .iter()
                .find(|record| record.id == current.id)
                .unwrap_or(&current);
            let Some(helper) = original.async_helper.as_ref() else {
                continue;
            };
            if !parents.contains(&helper.parent_run_id) || !helper.completion.is_unresolved() {
                continue;
            }
            plan.undo.helper_metadata.push((
                original.id.clone(),
                original.stop.clone(),
                helper.clone(),
                original.mirrored,
            ));
            let record = self
                .runs
                .get_mut(&original.id)
                .expect("source belongs to current ledger");
            record.async_helper.as_mut().unwrap().completion =
                crate::runs::HelperCompletionState::Suppressed {
                    reason: "The launching run was stopped; no helper completion will be scheduled"
                        .into(),
                    run_id: helper.completion.run_id().map(str::to_string),
                };
            record.mirrored = false;
            if let Some(cancelled) = plan
                .cancelled
                .iter_mut()
                .find(|cancelled| cancelled.id == record.id)
            {
                *cancelled = record.clone();
            }
        }
    }

    /// Marks a running Telegram turn (`sourceRef` `<connector>:<update>`)
    /// stopped, so it finishes without a reply and never runs again.
    fn stop_inbound(&mut self, source_ref: &str, undo: &mut RunStopUndo) {
        let Some((connector_id, update)) = source_ref.rsplit_once(':') else {
            return;
        };
        let Ok(update_id) = update.parse::<i64>() else {
            return;
        };
        if let Some(record) = self
            .inbound
            .get_mut(&(connector_id.to_string(), update_id))
            .filter(|record| record.processing_state == InboundProcessingState::Processing)
        {
            undo.inbound.push(record.clone());
            record.processing_state = InboundProcessingState::Stopped;
        }
    }

    fn stop_discord_inbound(&mut self, source_ref: &str, undo: &mut RunStopUndo) {
        use crate::connectors::discord::DiscordProcessingState;
        let Some((connector, message)) = source_ref.rsplit_once(':') else {
            return;
        };
        if let Some(record) = self
            .discord_inbound
            .get_mut(&(connector.into(), message.into()))
            .filter(|r| r.processing_state == DiscordProcessingState::Processing)
        {
            undo.discord_inbound.push(record.clone());
            record.processing_state = DiscordProcessingState::Stopped;
        }
    }
    fn suppress_discord_reply(&mut self, run: &RunRecord, undo: &mut RunStopUndo) {
        let Some(reply) = &run.reply_message_id else {
            return;
        };
        for record in self
            .discord_outbound
            .values_mut()
            .filter(|r| r.agent_id == run.agent_id && &r.assistant_message_id == reply)
        {
            let previous = record.clone();
            if crate::connectors::discord::request_outbound_stop(record) {
                undo.discord_outbound.push(previous);
            }
        }
    }

    /// Saves the stop marker of the running job attempt `<job>:<attempt>`.
    fn stop_job(&mut self, source_ref: &str, now_ms: u64, undo: &mut RunStopUndo) {
        let Some((job_id, attempt)) = source_ref.rsplit_once(':') else {
            return;
        };
        let Ok(attempt) = attempt.parse::<u32>() else {
            return;
        };
        if let Some(job) = self.jobs.get_mut(job_id).filter(|job| {
            job.status == AgentJobStatus::Running
                && job.attempt == attempt
                && job.stop_requested_at_ms.is_none()
        }) {
            undo.jobs.push(job.clone());
            job.stop_requested_at_ms = Some(now_ms);
        }
    }

    /// Suppresses the undelivered outbound record of `record`'s reply.
    fn suppress_undelivered_reply(&mut self, record: &RunRecord, undo: &mut RunStopUndo) {
        let Some(reply_id) = record.reply_message_id.as_deref() else {
            return;
        };
        for outbound in self.outbound.values_mut().filter(|outbound| {
            outbound.agent_id == record.agent_id
                && outbound.assistant_message_id == reply_id
                && outbound.delivery_state.awaits_delivery()
        }) {
            undo.outbound.push(outbound.clone());
            outbound.delivery_state = OutboundDeliveryState::Suppressed;
        }
    }

    /// Puts back what a stop changed after its save failed. A queued run is
    /// restored whole; a running one only loses the stop request, keeping
    /// anything its run recorded meanwhile; source records go back only if
    /// nothing else changed them since.
    pub(crate) fn revert_run_stop(&mut self, undo: RunStopUndo) {
        // Newest first, so each run goes back to awaiting its approvals.
        for approval in undo.approvals.into_iter().rev() {
            self.revert_settled_approval(approval);
        }
        for previous in undo.runs {
            match self.runs.get_mut(&previous.id) {
                Some(current) if previous.status == RunStatus::Queued => *current = previous,
                Some(current) => current.stop = previous.stop,
                None => {}
            }
        }
        for (id, stop, helper, mirrored) in undo.helper_metadata {
            if let Some(current) = self.runs.get_mut(&id) {
                current.stop = stop;
                current.async_helper = Some(helper);
                current.mirrored = mirrored;
            }
        }
        for previous in undo.inbound {
            let key = (previous.connector_id.clone(), previous.update_id);
            if self
                .inbound
                .get(&key)
                .is_some_and(|current| current.processing_state == InboundProcessingState::Stopped)
            {
                self.inbound.insert(key, previous);
            }
        }
        for previous in undo.outbound {
            if self
                .outbound
                .get(&previous.id)
                .is_some_and(|current| current.delivery_state == OutboundDeliveryState::Suppressed)
            {
                self.outbound.insert(previous.id.clone(), previous);
            }
        }
        for previous in undo.discord_inbound {
            let key = (previous.connector_id.clone(), previous.message_id.clone());
            if self.discord_inbound.get(&key).is_some_and(|r| {
                r.processing_state == crate::connectors::discord::DiscordProcessingState::Stopped
                    && r.run_id == previous.run_id
            }) {
                self.discord_inbound.insert(key, previous);
            }
        }
        for previous in undo.discord_outbound {
            let mut changed = previous.clone();
            crate::connectors::discord::request_outbound_stop(&mut changed);
            if self.discord_outbound.get(&previous.id) == Some(&changed) {
                self.discord_outbound.insert(previous.id.clone(), previous);
            }
        }
        for previous in undo.jobs {
            if let Some(current) = self.jobs.get_mut(&previous.id) {
                current.stop_requested_at_ms = previous.stop_requested_at_ms;
            }
        }
    }
}
