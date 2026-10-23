use std::collections::BTreeMap;
use std::sync::Arc;

use anima_core::execution_limits::{ExecutionBindingHint, ExecutionContext};
use anima_core::{Content, DataValue, ToolCall, ToolDescriptor};
use async_trait::async_trait;

/// Immutable host approval input for one exact tool invocation.
#[derive(Clone, Debug)]
pub struct ToolInvocation {
    pub(crate) agent_id: String,
    pub(crate) execution: ExecutionBindingHint,
    pub(crate) tool_call: ToolCall,
    pub(crate) descriptor: ToolDescriptor,
}

impl ToolInvocation {
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub fn execution(&self) -> &ExecutionBindingHint {
        &self.execution
    }
    pub fn tool_call(&self) -> &ToolCall {
        &self.tool_call
    }
    pub fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolApproval {
    Approved,
    Denied(String),
}

/// Host approval for one immutable invocation. Descriptor exposure never grants
/// approval. A denial, callback error, Stop or lost execution authority prevents
/// the effect. Callbacks must be cancel-safe and perform no external effects:
/// their waiting futures can be dropped on Stop or deadline.
///
/// `revalidate` runs synchronously after the last budget wait and immediately
/// before tool execution. It must return quickly without I/O. Override it when
/// host grants can be revoked.
/// The default is suitable for hosts whose approval cannot be revoked during
/// this invocation. Keep durable approval/Undo custody in the host.
#[async_trait]
pub trait ToolPolicy: Send + Sync {
    async fn approve(&self, invocation: &ToolInvocation) -> Result<ToolApproval, String>;
    fn revalidate(&self, _invocation: &ToolInvocation) -> Result<(), String> {
        Ok(())
    }
}

/// A tool the harness exposes to the model.
///
/// Implement `descriptor` to advertise the tool to the model (name, description,
/// JSON schema for the arguments) and `execute` to run it with the
/// model-supplied arguments.
#[async_trait]
pub trait HarnessTool: Send + Sync {
    /// Descriptor advertised to the model via `AgentConfig::tools`.
    fn descriptor(&self) -> ToolDescriptor;

    /// Execute the tool with the model-supplied arguments.
    async fn execute(&self, args: BTreeMap<String, DataValue>) -> Result<Content, String>;
}

/// Registry of tools available to a [`Harness`](crate::Harness). Registration is
/// trusted embedder execution authority unless the harness has a [`ToolPolicy`].
/// Model-visible descriptors alone are never approval.
#[derive(Clone, Default)]
pub struct ToolSet {
    tools: Vec<Arc<dyn HarnessTool>>,
}

impl ToolSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style registration.
    pub fn with_tool(mut self, tool: impl HarnessTool + 'static) -> Self {
        self.register(tool);
        self
    }

    pub fn register(&mut self, tool: impl HarnessTool + 'static) {
        self.tools.push(Arc::new(tool));
    }

    pub fn register_shared(&mut self, tool: Arc<dyn HarnessTool>) {
        self.tools.push(tool);
    }

    /// Append all tools from another set.
    pub fn extend(&mut self, other: ToolSet) {
        self.tools.extend(other.tools);
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Descriptors for `AgentConfig::tools`, in registration order.
    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools.iter().map(|tool| tool.descriptor()).collect()
    }

    /// Execute directly with trusted embedder authority. This low-level method
    /// does not run a harness policy or budget; prefer Harness::run/chat for
    /// controlled model-requested execution.
    pub async fn execute(
        &self,
        name: &str,
        args: BTreeMap<String, DataValue>,
    ) -> Result<Content, String> {
        let (tool, _) = self.resolve(name)?;
        tool.execute(args).await
    }

    fn resolve(&self, name: &str) -> Result<(Arc<dyn HarnessTool>, ToolDescriptor), String> {
        self.tools
            .iter()
            .find_map(|tool| {
                let descriptor = tool.descriptor();
                (descriptor.name == name).then(|| (tool.clone(), descriptor))
            })
            .ok_or_else(|| format!("Unknown tool: {name}"))
    }

    pub(crate) async fn execute_admitted(
        &self,
        agent_id: String,
        context: ExecutionContext,
        policy: Option<Arc<dyn ToolPolicy>>,
        tool_call: ToolCall,
    ) -> Result<Content, String> {
        context.check_live()?;
        // Retain the exact implementation and arguments before any approval
        // wait. Mutable descriptor implementations cannot redirect approval to
        // a different registered tool after the host has reviewed it.
        let (tool, descriptor) = self.resolve(&tool_call.name)?;
        let invocation = ToolInvocation {
            agent_id,
            execution: context.binding_hint(),
            tool_call,
            descriptor,
        };
        if let Some(policy) = &policy {
            match context.bounded(policy.approve(&invocation)).await? {
                ToolApproval::Approved => {}
                ToolApproval::Denied(reason) => {
                    return Err(format!("Tool approval denied: {reason}"))
                }
            }
        }
        // Final host approval revalidation comes after every await. There is no
        // intervening budget wait in which a revoked grant could go unchecked.
        // Core retains the exact effect's future until physical retirement.
        context.check_authority().await?;
        context.check_live()?;
        if let Some(policy) = &policy {
            policy.revalidate(&invocation)?;
        }
        context.check_live()?;
        tool.execute(invocation.tool_call.args).await
    }
}
