//! Bind a cell to authentic native context; never dispatch a raw registry handler.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anima_core::{
    tool_rpc::{
        decode_program_request, ProgramOutcome, ProgramSummary, RpcAdmission, RpcCallOutcome,
        RpcProgram, RpcRequest, MAX_RPC_FRAME_BYTES,
    },
    AgentState, Content, DataValue, Message, TaskResult, TaskStatus, ToolCall,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{runs::RunLink, tools::ToolExecutionContext};

#[derive(Clone, Debug)]
pub(crate) struct RpcGrantFence {
    pub(crate) run: RunLink,
    pub(crate) outer_call: ToolCall,
    /// True means native handler entry, not a proven or persisted effect.
    pub(crate) dispatch_started: Option<Arc<AtomicBool>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BrokerDisposition {
    Success,
    NativeFailed,
    Refused,
    ReviewRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrokerReply {
    pub(crate) disposition: BrokerDisposition,
    pub(crate) text: String,
}

/// Private worker bootstrap material. No serde or Debug implementation can
/// accidentally turn transport secrets into public input, logs or receipts.
#[derive(Clone)]
pub(crate) struct BrokerCredentials {
    pub(crate) token: String,
    pub(crate) cell_epoch: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrokerError {
    MissingContext,
    PermissionDenied,
    ForeignScope,
    UnsupportedOrigin,
    OuterIntentMissing,
}

pub(crate) struct Broker {
    context: ToolExecutionContext,
    agent: AgentState,
    input: Message,
    credentials: BrokerCredentials,
    call_prefix: String,
    fence: RpcGrantFence,
    program: RpcProgram,
    replay_dispositions: BTreeMap<u32, BrokerDisposition>,
    descriptors: Vec<anima_core::ToolDescriptor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    token: String,
    request: RpcRequest,
}

const RPC_TOOLS: &[&str] = &[
    "web_search",
    "web_fetch",
    "read_file",
    "write_file",
    "glob",
    "grep",
    "edit_file",
    "multi_edit",
    "bash",
];

impl Broker {
    pub(crate) async fn capture(
        context: ToolExecutionContext,
        agent: AgentState,
        input: Message,
        outer_call: ToolCall,
    ) -> Result<Self, BrokerError> {
        let (Some(run), Some(journal), Some(gate), Some(cancel)) = (
            &context.run_link,
            context.rpc_journal(),
            &context.approvals,
            &context.cancel,
        ) else {
            return Err(BrokerError::MissingContext);
        };
        if run != journal.bound_run()
            || run != gate.bound_run()
            || run.agent_id != agent.id
            || input.agent_id != agent.id
            || input.room_id != run.session_id
        {
            return Err(BrokerError::ForeignScope);
        }
        if context.has_upstream_tool_origin() || crate::agent_runs::is_helper_config(&agent.config)
        {
            return Err(BrokerError::UnsupportedOrigin);
        }
        if outer_call.name != "execute_code" || outer_call.id.trim().is_empty() {
            return Err(BrokerError::OuterIntentMissing);
        }
        let arguments =
            crate::routes::data_value_to_json(&DataValue::Object(outer_call.args.clone()));
        decode_program_request(
            &serde_json::to_vec(&arguments).map_err(|_| BrokerError::PermissionDenied)?,
        )
        .map_err(|_| BrokerError::PermissionDenied)?;
        if cancel.is_cancelled()
            || !agent.config.allows_tool("execute_code")
            || !agent.config.allows_tool("bash")
            || gate.rpc_outer_denied(&agent).await
        {
            return Err(BrokerError::PermissionDenied);
        }
        let (remaining, current) = journal
            .rpc_authority(run, &outer_call, None)
            .await
            .map_err(|_| BrokerError::PermissionDenied)?;
        let captured: BTreeSet<_> = RPC_TOOLS
            .iter()
            .filter(|name| agent.config.allows_tool(name) && current.contains(**name))
            .map(|name| (*name).to_owned())
            .collect();
        let fence = RpcGrantFence {
            run: run.clone(),
            outer_call,
            dispatch_started: None,
        };
        let descriptors = captured
            .iter()
            .filter_map(|name| context.rpc_descriptor(name))
            .collect();
        Ok(Self {
            context: context.with_rpc_fence(fence.clone()),
            agent,
            input,
            fence,
            credentials: BrokerCredentials {
                token: uuid::Uuid::new_v4().to_string(),
                cell_epoch: uuid::Uuid::new_v4().to_string(),
            },
            call_prefix: uuid::Uuid::new_v4().to_string(),
            program: RpcProgram::new(captured, remaining),
            replay_dispositions: BTreeMap::new(),
            descriptors,
        })
    }

    pub(crate) fn credentials(&self) -> BrokerCredentials {
        self.credentials.clone()
    }

    pub(super) fn captured_descriptors(&self) -> Vec<anima_core::ToolDescriptor> {
        self.descriptors.clone()
    }

    pub(crate) fn effective_call_limit(&self) -> u32 {
        self.program.summary(None).effective_call_limit
    }

    pub(crate) fn summary(&self, exit: Option<i32>) -> ProgramSummary {
        self.program.summary(exit)
    }

    fn refuse_frame(&mut self) -> BrokerReply {
        // Authentication failures, malformed envelopes and stale cells consume
        // attempts too, without creating replay entries or native receipts.
        let _ = self.program.admit(b"");
        BrokerReply {
            disposition: BrokerDisposition::Refused,
            text: "Tool RPC frame refused".into(),
        }
    }

    pub(crate) async fn request(&mut self, frame: &[u8]) -> TaskResult<Content> {
        project(self.request_reply(frame).await)
    }

    fn requires_review(&self) -> bool {
        self.context
            .rpc_journal()
            .is_none_or(|journal| journal.failure().is_some())
            || self.program.summary(None).outcome == ProgramOutcome::ReviewRequired
    }

    fn refuse_authenticated(&mut self) -> BrokerReply {
        let mut reply = self.refuse_frame();
        if self.requires_review() {
            reply.disposition = BrokerDisposition::ReviewRequired;
            reply.text =
                "Native outcome is pending or uncertain; no further tool call was dispatched"
                    .into();
        }
        reply
    }

    /// Dispositions derive from native handler entry and acknowledged outcome,
    /// never from error text. Authentication remains ahead of cell-state reads.
    pub(crate) async fn request_reply(&mut self, frame: &[u8]) -> BrokerReply {
        if frame.len() > MAX_RPC_FRAME_BYTES {
            return self.refuse_frame();
        }
        let Ok(envelope) = serde_json::from_slice::<Envelope>(frame) else {
            return self.refuse_frame();
        };
        let authentic_token = envelope.token.len() == self.credentials.token.len()
            && envelope
                .token
                .bytes()
                .zip(self.credentials.token.bytes())
                .fold(0u8, |different, (left, right)| different | (left ^ right))
                == 0;
        if !authentic_token || envelope.request.cell_epoch != self.credentials.cell_epoch {
            return self.refuse_frame();
        }
        if self.requires_review() {
            // Preserve an acknowledged cached uncertain reply exactly. The
            // core compares the entire canonical request before any replay;
            // this branch never dispatches, even after journal cancellation.
            if self.replay_dispositions.get(&envelope.request.sequence)
                == Some(&BrokerDisposition::ReviewRequired)
            {
                if let Ok(encoded) = serde_json::to_vec(&envelope.request) {
                    if let Ok(RpcAdmission::Replay(outcome)) = self.program.admit(&encoded) {
                        return BrokerReply {
                            disposition: BrokerDisposition::ReviewRequired,
                            text: outcome.text,
                        };
                    }
                    return BrokerReply {
                        disposition: BrokerDisposition::ReviewRequired,
                        text: "Native outcome is uncertain; no further tool call was dispatched"
                            .into(),
                    };
                }
            }
            let _ = self.program.admit(b"");
            return BrokerReply {
                disposition: BrokerDisposition::ReviewRequired,
                text: "Native outcome is pending or uncertain; no further tool call was dispatched"
                    .into(),
            };
        }
        if envelope.request.tool == "bash"
            && envelope
                .request
                .args
                .keys()
                .any(|key| !matches!(key.as_str(), "command" | "timeout" | "cwd"))
        {
            return self.refuse_authenticated();
        }
        // Retiring a run or revoking its grants closes response admission too;
        // even cached results cannot keep a retired cell's authority alive.
        if self
            .context
            .cancel
            .as_ref()
            .is_none_or(|cancel| cancel.is_cancelled())
        {
            return self.refuse_authenticated();
        }
        let Some(journal) = self.context.rpc_journal() else {
            return self.refuse_authenticated();
        };
        if journal
            .rpc_authority(
                &self.fence.run,
                &self.fence.outer_call,
                Some(&envelope.request.tool),
            )
            .await
            .is_err()
        {
            return self.refuse_authenticated();
        }
        let Ok(encoded) = serde_json::to_vec(&envelope.request) else {
            return self.refuse_frame();
        };
        let admission = match self.program.admit(&encoded) {
            Ok(admission) => admission,
            Err(error) => {
                return BrokerReply {
                    disposition: if self.requires_review() {
                        BrokerDisposition::ReviewRequired
                    } else {
                        BrokerDisposition::Refused
                    },
                    text: format!("Tool RPC refused: {error:?}"),
                }
            }
        };
        let request = match admission {
            RpcAdmission::Replay(outcome) => {
                return BrokerReply {
                    disposition: self
                        .replay_dispositions
                        .get(&envelope.request.sequence)
                        .copied()
                        .unwrap_or(BrokerDisposition::ReviewRequired),
                    text: outcome.text,
                }
            }
            RpcAdmission::Dispatch(request) => request,
        };
        let call = ToolCall {
            id: format!("rpc_{}_{}", self.call_prefix, request.sequence),
            name: request.tool,
            args: request
                .args
                .into_iter()
                .map(|(key, value)| (key, data(value)))
                .collect(),
        };
        // This is the one dispatch path. It retains native policy prompts,
        // protected-file checks, cancellation and awaited Started/outcome saves.
        let dispatched = Arc::new(AtomicBool::new(false));
        let mut fence = self.fence.clone();
        fence.dispatch_started = Some(dispatched.clone());
        let result = self
            .context
            .clone()
            .with_rpc_fence(fence)
            .execute_tool(self.agent.clone(), self.input.clone(), call)
            .await;
        let outcome = RpcCallOutcome {
            succeeded: result.status == TaskStatus::Success && result.error.is_none(),
            text: result
                .error
                .unwrap_or_else(|| result.data.map(|data| data.text).unwrap_or_default()),
            effect_uncertain: self
                .context
                .rpc_journal()
                .is_none_or(|journal| journal.failure().is_some()),
        };
        let mut disposition = if outcome.effect_uncertain {
            BrokerDisposition::ReviewRequired
        } else if outcome.succeeded {
            BrokerDisposition::Success
        } else if dispatched.load(Ordering::Acquire) {
            BrokerDisposition::NativeFailed
        } else {
            BrokerDisposition::Refused
        };
        let text = outcome.text.clone();
        if let Err(error) = self.program.finish(request.sequence, outcome) {
            // Keep the pending sequence blocked if the native output exceeds
            // the cell budget. Never redispatch to obtain a smaller response.
            return BrokerReply {
                disposition: BrokerDisposition::ReviewRequired,
                text: format!("Tool RPC result unavailable: {error:?}"),
            };
        }
        if self.requires_review() {
            disposition = BrokerDisposition::ReviewRequired;
        }
        // Only a small closed tag is duplicated; bounded native text remains in
        // the core replay map and does not become a second receipt registry.
        self.replay_dispositions
            .insert(request.sequence, disposition);
        BrokerReply { disposition, text }
    }
}

fn project(reply: BrokerReply) -> TaskResult<Content> {
    if reply.disposition == BrokerDisposition::Success {
        TaskResult::success(
            Content {
                text: reply.text,
                ..Default::default()
            },
            0,
        )
    } else {
        TaskResult::error(reply.text, 0)
    }
}

fn data(value: Value) -> DataValue {
    match value {
        Value::Null => DataValue::Null,
        Value::Bool(value) => DataValue::Bool(value),
        Value::Number(value) => DataValue::Number(value.as_f64().expect("JSON numbers are finite")),
        Value::String(value) => DataValue::String(value),
        Value::Array(values) => DataValue::Array(values.into_iter().map(data).collect()),
        Value::Object(values) => DataValue::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, data(value)))
                .collect(),
        ),
    }
}
