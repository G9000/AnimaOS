//! Durable publication authority is separate from session/run authority.
use super::{
    DaemonState, McpConnectionState, McpDispatchStamp, McpManager, McpReadyBinding,
    McpServiceError, McpToolAccess,
};
use crate::memory_provider::{
    binding,
    outbox::{Publication, PublicationState},
};
use anima_core::{CancelSignal, Content, ToolCall};
use serde_json::{json, Value};
use std::sync::{atomic::Ordering, Arc};

#[derive(Clone)]
pub(super) struct PublicationAuthority {
    item: Publication,
    remote: String,
    args: Value,
    owner: crate::profile_owner::ProfileOwner,
}
#[derive(Clone)]
pub(super) struct PublicationStamp {
    authority: Arc<PublicationAuthority>,
    identity: InvocationIdentity,
    connection_version: u64,
    grant_epoch: u64,
}
#[derive(Clone)]
pub(super) struct InvocationIdentity {
    pub(super) agent_id: String,
    pub(super) connection_id: String,
    pub(super) alias: String,
    pub(super) descriptor_digest: String,
    pub(super) wire_generation: u64,
}
pub(super) enum InvocationStamp {
    Run(McpDispatchStamp),
    Publication(PublicationStamp),
}
impl InvocationStamp {
    pub(super) fn identity(&self) -> InvocationIdentity {
        match self {
            Self::Publication(s) => s.identity.clone(),
            Self::Run(s) => InvocationIdentity {
                agent_id: s.agent_id.clone(),
                connection_id: s.connection_id.clone(),
                alias: s.alias.clone(),
                descriptor_digest: s.descriptor_digest.clone(),
                wire_generation: s.wire_generation,
            },
        }
    }
}
fn operation_allowed(
    state: PublicationState,
    resume: Option<PublicationState>,
    remote: &str,
    write: bool,
) -> bool {
    let phase = if state == PublicationState::Sending {
        resume.unwrap_or(PublicationState::Unknown)
    } else {
        state
    };
    if !matches!(
        phase,
        PublicationState::Queued
            | PublicationState::Unknown
            | PublicationState::SubmittedForReview
            | PublicationState::Blocked
            | PublicationState::Conflict
    ) {
        return false;
    }
    if remote == "mother_get_context" {
        return true;
    }
    match phase {
        PublicationState::Queued => state == PublicationState::Sending && write,
        PublicationState::Unknown => remote == "mother_get_receipt",
        PublicationState::SubmittedForReview => remote == "mother_get_proposal",
        _ => false,
    }
}
impl McpManager {
    pub(super) fn publication_scope(
        &self,
        state: &DaemonState,
        authority: &PublicationAuthority,
    ) -> Result<(), &'static str> {
        let item = &authority.item;
        let admission = &item.admission;
        authority
            .owner
            .ensure_owned()
            .map_err(|_| "profile_ownership_lost")?;
        Self::owner(state, &admission.agent_id).map_err(|_| "publication_authority_changed")?;
        self.same_publication(state)
            .map_err(|_| "publication_authority_changed")?;
        if self
            .inner
            .live
            .lock()
            .map_err(|_| "connection_unavailable")?
            .connections
            .get(&admission.binding.connection_id)
            .is_some_and(|connection| {
                connection.state == McpConnectionState::Disconnected && connection.error.is_none()
            })
        {
            return Err("connection_manually_disconnected");
        }
        let runtime = state
            .agents
            .get(&admission.agent_id)
            .ok_or("publication_authority_changed")?;
        if crate::agent_runs::is_helper_config(runtime.config())
            || binding::binding(runtime.config()).ok().flatten().as_ref()
                != Some(&admission.binding)
        {
            return Err("publication_authority_changed");
        }
        let row = state
            .mcp_registry
            .connections
            .get(&admission.binding.connection_id)
            .filter(|r| {
                r.agent_id == admission.agent_id
                    && r.enabled
                    && r.version == admission.connection_version
            })
            .ok_or("publication_authority_changed")?;
        if state
            .paired_devices
            .records
            .iter()
            .any(|d| d.mcp_connection_id == row.id)
        {
            return Err("publication_authority_changed");
        }
        let write = row
            .reviewed_tools
            .iter()
            .find(|t| {
                t.descriptor.remote_name == admission.remote_tool
                    && t.descriptor.descriptor_digest == admission.descriptor_digest
            })
            .ok_or("publication_authority_changed")?;
        let tool = row
            .reviewed_tools
            .iter()
            .find(|t| t.descriptor.remote_name == authority.remote)
            .ok_or("publication_authority_changed")?;
        let allowed_args = match authority.remote.as_str() {
            "mother_get_context" => json!({"limit":1}),
            "mother_get_receipt" => json!({"operationId":admission.operation_id}),
            "mother_get_proposal" => json!({"id":item.proposal_id}),
            name if name == admission.remote_tool => admission.request.clone(),
            _ => return Err("publication_authority_changed"),
        };
        if allowed_args != authority.args
            || !operation_allowed(
                item.state,
                item.resume_state,
                &authority.remote,
                tool == write,
            )
        {
            return Err("publication_authority_changed");
        }
        for reviewed in [write, tool] {
            match McpToolAccess::for_agent(state, &admission.agent_id) {
                McpToolAccess::Automatic => {}
                McpToolAccess::Review
                    if runtime
                        .config()
                        .tools
                        .iter()
                        .flatten()
                        .any(|t| t == &reviewed.tool_descriptor()) => {}
                _ => return Err("publication_authority_changed"),
            }
        }
        let write_call = ToolCall {
            id: admission.operation_id.clone(),
            name: write.alias.clone(),
            args: admission
                .request
                .as_object()
                .ok_or("publication_authority_changed")?
                .iter()
                .map(|(k, v)| (k.clone(), portable(v)))
                .collect(),
        };
        match crate::approvals::evaluate(
            &state.approvals.policy(&admission.agent_id),
            &state.approvals.rules_for(&admission.agent_id),
            &[],
            &write_call,
        ) {
            crate::approvals::Verdict::Allow => {}
            crate::approvals::Verdict::Ask => return Err("approval_required"),
            crate::approvals::Verdict::Deny => return Err("policy_denied"),
        }
        let args = authority
            .args
            .as_object()
            .ok_or("publication_authority_changed")?
            .iter()
            .map(|(k, v)| (k.clone(), portable(v)))
            .collect();
        let call = ToolCall {
            id: admission.operation_id.clone(),
            name: tool.alias.clone(),
            args,
        };
        match crate::approvals::evaluate(
            &state.approvals.policy(&admission.agent_id),
            &state.approvals.rules_for(&admission.agent_id),
            &[],
            &call,
        ) {
            crate::approvals::Verdict::Allow => Ok(()),
            crate::approvals::Verdict::Ask => Err("approval_required"),
            crate::approvals::Verdict::Deny => Err("policy_denied"),
        }
    }
    pub(super) async fn publication_current(
        &self,
        authority: &PublicationAuthority,
    ) -> Result<(), &'static str> {
        authority
            .owner
            .verify_owned()
            .await
            .map_err(|_| "profile_ownership_lost")?;
        let state = self.state().map_err(|_| "publication_authority_changed")?;
        let service = {
            let guard = state.read().await;
            self.publication_scope(&guard, authority)?;
            guard
                .memory_publications
                .clone()
                .ok_or("publication_authority_changed")?
        };
        if !service.accepting() {
            return Err("publication_authority_changed");
        }
        let item = &authority.item;
        let actual = service
            .get(&item.admission.agent_id, &item.admission.publication_id)
            .await
            .map_err(|_| "publication_authority_changed")?
            .ok_or("publication_authority_changed")?;
        if actual != *item || !service.accepting() {
            return Err("publication_authority_changed");
        }
        self.publication_scope(&*state.read().await, authority)?;
        Ok(())
    }
    pub(crate) async fn publication_preflight(
        &self,
        item: &Publication,
        remote: &str,
        args: &Value,
    ) -> Result<(), &'static str> {
        let state = self.state().map_err(|_| "publication_authority_changed")?;
        let owner = state
            .read()
            .await
            .profile_owner()
            .ok_or("profile_ownership_lost")?;
        let authority = PublicationAuthority {
            item: item.clone(),
            remote: remote.into(),
            args: args.clone(),
            owner,
        };
        self.publication_current(&authority).await
    }
    async fn publication_binding(
        &self,
        stamp: &PublicationStamp,
    ) -> Result<McpReadyBinding, McpServiceError> {
        self.running()?;
        self.publication_current(&stamp.authority)
            .await
            .map_err(|_| McpServiceError::Revoked)?;
        let binding = self
            .inner
            .publication
            .read()
            .map_err(|_| McpServiceError::Unavailable)?
            .live
            .get(&(
                stamp.identity.agent_id.clone(),
                stamp.identity.alias.clone(),
            ))
            .cloned()
            .ok_or(McpServiceError::Revoked)?;
        let state = self.state()?;
        let guard = state.read().await;
        self.publication_scope(&guard, &stamp.authority)
            .map_err(|_| McpServiceError::Revoked)?;
        if guard
            .memory_publications
            .as_ref()
            .is_none_or(|service| !service.accepting())
        {
            return Err(McpServiceError::Revoked);
        }
        if !guard
            .mcp_registry
            .connections
            .get(&binding.connection_id)
            .is_some_and(|r| r.reviewed_tools.contains(&binding.tool))
        {
            return Err(McpServiceError::Revoked);
        }
        let epoch = *self
            .inner
            .live
            .lock()
            .map_err(|_| McpServiceError::Unavailable)?
            .grant_epochs
            .get(&stamp.identity.agent_id)
            .unwrap_or(&0);
        if binding.connection_id != stamp.identity.connection_id
            || binding.version != stamp.connection_version
            || binding.generation != stamp.identity.wire_generation
            || binding.tool.descriptor.descriptor_digest != stamp.identity.descriptor_digest
            || epoch != stamp.grant_epoch
        {
            return Err(McpServiceError::Revoked);
        }
        Ok(binding)
    }
    pub(super) async fn invocation_binding(
        &self,
        stamp: &InvocationStamp,
    ) -> Result<McpReadyBinding, McpServiceError> {
        match stamp {
            InvocationStamp::Run(s) => self.dispatch_binding(s).await,
            InvocationStamp::Publication(s) => self.publication_binding(s).await,
        }
    }
    pub(crate) async fn invoke_publication(
        &self,
        item: &Publication,
        remote: &str,
        args: Value,
        cancel: CancelSignal,
    ) -> Result<Content, &'static str> {
        let state = self.state().map_err(|_| "publication_authority_changed")?;
        let owner = state
            .read()
            .await
            .profile_owner()
            .ok_or("profile_ownership_lost")?;
        let authority = Arc::new(PublicationAuthority {
            item: item.clone(),
            remote: remote.into(),
            args: args.clone(),
            owner,
        });
        self.publication_current(&authority).await?;
        if self
            .workspace_context_tool(
                &item.admission.agent_id,
                &item.admission.binding.connection_id,
                remote,
            )
            .await
            .is_none()
        {
            self.reconnect_publication(authority.clone(), &cancel)
                .await?;
        }
        self.publication_current(&authority).await?;
        let binding = self
            .inner
            .publication
            .read()
            .map_err(|_| "publication_authority_changed")?
            .live
            .values()
            .find(|b| {
                b.connection_id == item.admission.binding.connection_id
                    && b.tool.descriptor.remote_name == remote
            })
            .cloned()
            .ok_or("publication_authority_changed")?;
        let epoch = *self
            .inner
            .live
            .lock()
            .map_err(|_| "publication_authority_changed")?
            .grant_epochs
            .get(&item.admission.agent_id)
            .unwrap_or(&0);
        let stamp = PublicationStamp {
            authority: authority.clone(),
            identity: InvocationIdentity {
                agent_id: item.admission.agent_id.clone(),
                connection_id: binding.connection_id,
                alias: binding.tool.alias,
                descriptor_digest: binding.tool.descriptor.descriptor_digest,
                wire_generation: binding.generation,
            },
            connection_version: binding.version,
            grant_epoch: epoch,
        };
        let manager = Self {
            publication_scope: Some(authority.clone()),
            ..self.clone()
        };
        let lost = authority.owner.lost_signal();
        let call_cancel = cancel.clone();
        // Owned work retains the profile fence through transport retirement and cleanup.
        self.owned(async move {
            let combined = CancelSignal::new();
            let signal = combined.clone();
            let relay = tokio::spawn(async move {
                tokio::select! {_=lost.cancelled()=>{},_=call_cancel.cancelled()=>{}}
                signal.cancel();
            });
            let result = manager
                .invoke_owned(InvocationStamp::Publication(stamp), args, combined)
                .await;
            relay.abort();
            let _ = relay.await;
            result
        })
        .await
        .map_err(|e| e.code())
    }
    async fn reconnect_publication(
        &self,
        authority: Arc<PublicationAuthority>,
        cancel: &CancelSignal,
    ) -> Result<(), &'static str> {
        self.publication_current(&authority).await?;
        let item = &authority.item;
        {
            let live = self
                .inner
                .live
                .lock()
                .map_err(|_| "connection_unavailable")?;
            if live
                .connections
                .get(&item.admission.binding.connection_id)
                .is_some_and(|c| {
                    !matches!(
                        c.state,
                        McpConnectionState::Disconnected | McpConnectionState::Degraded
                    ) || c.session.is_some()
                        || c.closing.load(Ordering::SeqCst) != 0
                        || (c.state == McpConnectionState::Disconnected && c.error.is_none())
                })
            {
                return Err("connection_unavailable");
            }
        }
        let manager = Self {
            inner: self.inner.clone(),
            management_run: None,
            context_run: None,
            publication_scope: Some(authority.clone()),
        };
        let lost = authority.owner.lost_signal();
        tokio::select! {result=manager.connect(&item.admission.agent_id,&item.admission.binding.connection_id,item.admission.connection_version)=>result.map(|_|()).map_err(|e|e.code()),_=cancel.cancelled()=>{let _=manager.retire(&item.admission.binding.connection_id,Some(McpServiceError::Cancelled));Err("cancelled")},_=lost.cancelled()=>{let _=self.retire(&item.admission.binding.connection_id,Some(McpServiceError::Revoked));Err("profile_ownership_lost")}}
    }
}
fn portable(v: &Value) -> anima_core::DataValue {
    use anima_core::DataValue;
    match v {
        Value::Null => DataValue::Null,
        Value::Bool(v) => DataValue::Bool(*v),
        Value::String(v) => DataValue::String(v.clone()),
        Value::Number(v) => DataValue::Number(v.as_f64().unwrap_or_default()),
        Value::Array(v) => DataValue::Array(v.iter().map(portable).collect()),
        Value::Object(v) => {
            DataValue::Object(v.iter().map(|(k, v)| (k.clone(), portable(v))).collect())
        }
    }
}
#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
