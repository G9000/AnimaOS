use std::sync::Arc;

use anima_core::execution_limits::{ExecutionContext, ExecutionLimits};
use anima_core::turn_recovery::{TurnBinding, TurnCheckpointSink};
use anima_core::{
    AgentConfig, AgentRuntime, AgentRuntimeSnapshot, AgentState, Content, EngineEvent, Message,
    ModelAdapter, RunControl, RunObserver, TaskResult,
};

use crate::config::{HarnessBuilder, HarnessControls};
use crate::tool::{ToolPolicy, ToolSet};

enum ExecutionMode {
    /// Only fresh builders may automatically admit independent owner inputs.
    Independent(ExecutionLimits),
    PendingBound {
        run_id: String,
        limits: ExecutionLimits,
    },
    Attached,
    /// Even legacy snapshots require an explicit trusted context attachment.
    Restored,
}

/// An embeddable agent harness: system prompt + tools + agentic loop +
/// provider credentials, composed on top of `anima-core`'s [`AgentRuntime`].
///
/// Build one with [`Harness::builder`].
pub struct Harness {
    runtime: AgentRuntime,
    adapter: Arc<dyn ModelAdapter>,
    tools: ToolSet,
    tool_policy: Option<Arc<dyn ToolPolicy>>,
    execution_mode: ExecutionMode,
    checkpoint_binding: Option<TurnBinding>,
}

impl Harness {
    pub fn builder() -> HarnessBuilder {
        HarnessBuilder::new()
    }

    pub(crate) fn new(
        config: AgentConfig,
        adapter: Arc<dyn ModelAdapter>,
        tools: ToolSet,
        event_listener: Option<Arc<dyn Fn(EngineEvent) + Send + Sync>>,
        controls: HarnessControls,
    ) -> Result<Self, String> {
        controls.execution_limits.validate()?;
        if let Some(id) = &controls.run_id {
            validate_run_id(id)?;
        }
        if let Some((binding, _)) = &controls.checkpoint {
            binding.validate()?;
            if controls
                .run_id
                .as_ref()
                .is_some_and(|id| id != &binding.attempt_run_id)
                || controls
                    .execution_context
                    .as_ref()
                    .is_some_and(|context| context.run_id != binding.attempt_run_id)
            {
                return Err("Checkpoint does not match the execution run".into());
            }
        }
        let mut runtime = AgentRuntime::new(config, adapter.clone());
        if let Some(listener) = event_listener {
            runtime.set_event_listener(listener);
        }
        if let Some(control) = controls.run_control {
            runtime.set_run_control(control);
        }
        if let Some(observer) = controls.observer {
            runtime.set_run_observer(observer);
        }
        let checkpoint_binding = controls
            .checkpoint
            .as_ref()
            .map(|(binding, _)| binding.clone());
        let bound_id = controls.run_id.or_else(|| {
            checkpoint_binding
                .as_ref()
                .map(|binding| binding.attempt_run_id.clone())
        });
        if let Some(id) = &bound_id {
            runtime.set_run_id(id.clone());
        }
        if let Some((binding, sink)) = controls.checkpoint {
            runtime.set_turn_checkpoint_sink(binding, sink);
        }
        let execution_mode = match controls.execution_context {
            Some(context) => {
                context.check_live()?;
                runtime.set_execution_context(context)?;
                ExecutionMode::Attached
            }
            None => match bound_id {
                Some(run_id) => ExecutionMode::PendingBound {
                    run_id,
                    limits: controls.execution_limits,
                },
                None => ExecutionMode::Independent(controls.execution_limits),
            },
        };
        runtime.init();
        Ok(Self {
            runtime,
            adapter,
            tools,
            tool_policy: controls.tool_policy,
            execution_mode,
            checkpoint_binding,
        })
    }

    /// Rebuild a harness from a previously captured [`Harness::snapshot`].
    ///
    /// Snapshots carry conversation and a scope binding hint, never authority.
    /// Explicitly reattach the matching live/durable execution context before
    /// running. Reattach control, observer, checkpoint sink and tool policy as
    /// required by the host. Restore never creates fresh execution allowance.
    pub fn restore(
        snapshot: AgentRuntimeSnapshot,
        adapter: Arc<dyn ModelAdapter>,
        tools: ToolSet,
    ) -> Self {
        Self {
            runtime: AgentRuntime::from_snapshot(snapshot, adapter.clone()),
            adapter,
            tools,
            tool_policy: None,
            execution_mode: ExecutionMode::Restored,
            checkpoint_binding: None,
        }
    }

    pub fn set_event_listener(&mut self, listener: impl Fn(EngineEvent) + Send + Sync + 'static) {
        self.runtime.set_event_listener(Arc::new(listener));
    }

    /// Bind a fresh harness to one host run. Existing/restored scope identity
    /// cannot be changed through this setter.
    pub fn set_run_id(&mut self, id: impl Into<String>) -> Result<(), String> {
        let id = id.into();
        validate_run_id(&id)?;
        if self
            .runtime
            .snapshot()
            .execution_binding
            .as_ref()
            .is_some_and(|binding| binding.run_id != id)
            || self
                .checkpoint_binding
                .as_ref()
                .is_some_and(|binding| binding.attempt_run_id != id)
        {
            return Err(
                "Run identity does not match the attached execution/checkpoint binding".into(),
            );
        }
        self.execution_mode = match &self.execution_mode {
            ExecutionMode::Independent(limits) | ExecutionMode::PendingBound { limits, .. } => {
                ExecutionMode::PendingBound {
                    run_id: id.clone(),
                    limits: limits.clone(),
                }
            }
            ExecutionMode::Attached => ExecutionMode::Attached,
            ExecutionMode::Restored => ExecutionMode::Restored,
        };
        self.runtime.set_run_id(id);
        Ok(())
    }

    pub fn run_id(&self) -> Option<&str> {
        self.runtime.run_id()
    }

    pub fn set_run_control(&mut self, control: RunControl) {
        self.runtime.set_run_control(control);
    }

    pub fn run_control(&self) -> Option<&RunControl> {
        self.runtime.run_control()
    }

    pub fn set_run_observer(&mut self, observer: Arc<dyn RunObserver>) {
        self.runtime.set_run_observer(observer);
    }

    /// Reattach matching authority. The existing port retains all consumed and
    /// uncertain reservations; a fresh scope cannot replace a snapshot hint.
    pub fn set_execution_context(&mut self, context: ExecutionContext) -> Result<(), String> {
        context.check_live()?;
        if self
            .checkpoint_binding
            .as_ref()
            .is_some_and(|binding| binding.attempt_run_id != context.run_id)
        {
            return Err("Execution context does not match the checkpoint run".into());
        }
        self.runtime.set_execution_context(context)?;
        self.execution_mode = ExecutionMode::Attached;
        Ok(())
    }

    pub fn execution_context(&self) -> Option<&ExecutionContext> {
        self.runtime.execution_context()
    }

    /// Install one host-owned checkpoint attempt. This does not grant tool
    /// approval or replace execution allowance.
    pub fn set_turn_checkpoint_sink(
        &mut self,
        binding: TurnBinding,
        sink: Arc<dyn TurnCheckpointSink>,
    ) -> Result<(), String> {
        binding.validate()?;
        if self
            .runtime
            .run_id()
            .is_some_and(|id| id != binding.attempt_run_id)
            || self
                .runtime
                .snapshot()
                .execution_binding
                .as_ref()
                .is_some_and(|hint| hint.run_id != binding.attempt_run_id)
        {
            return Err("Checkpoint does not match the execution run".into());
        }
        self.set_run_id(binding.attempt_run_id.clone())?;
        self.runtime.set_turn_checkpoint_sink(binding.clone(), sink);
        self.checkpoint_binding = Some(binding);
        Ok(())
    }

    pub fn set_tool_policy(&mut self, policy: Arc<dyn ToolPolicy>) {
        self.tool_policy = Some(policy);
    }

    pub fn id(&self) -> &str {
        self.runtime.id()
    }

    pub fn config(&self) -> &AgentConfig {
        self.runtime.config()
    }

    pub fn state(&self) -> AgentState {
        self.runtime.state()
    }

    pub fn messages(&self) -> &[Message] {
        self.runtime.messages()
    }

    pub fn tools(&self) -> &ToolSet {
        &self.tools
    }

    pub fn adapter(&self) -> &Arc<dyn ModelAdapter> {
        &self.adapter
    }

    /// Capture the runtime state for later [`Harness::restore`]. The event log is capped:
    /// `snapshot().events` holds at most the newest `MAX_RETAINED_EVENTS`, which may be
    /// fewer than `event_count`.
    pub fn snapshot(&self) -> AgentRuntimeSnapshot {
        self.runtime.snapshot()
    }

    /// Run a single task. Tool calls requested by the model are dispatched to
    /// the registered [`ToolSet`] until the model finishes or the tool
    /// iteration limit is hit.
    pub async fn run(&mut self, input: impl Into<String>) -> TaskResult<Content> {
        self.run_input(Vec::new(), text_content(input)).await
    }

    /// Like [`Harness::run`], but includes all previously recorded messages as
    /// conversation history, so the model sees the whole session.
    pub async fn chat(&mut self, input: impl Into<String>) -> TaskResult<Content> {
        let history = self.runtime.messages().to_vec();
        self.run_input(history, text_content(input)).await
    }

    fn prepare_execution(&mut self) -> Result<ExecutionContext, String> {
        match &self.execution_mode {
            ExecutionMode::Independent(limits) => {
                let context = ExecutionContext::ephemeral(
                    uuid::Uuid::new_v4().to_string(),
                    limits.clone(),
                    Default::default(),
                )?;
                self.runtime.begin_new_execution_context(context)?;
            }
            ExecutionMode::PendingBound { run_id, limits } => {
                let context = ExecutionContext::ephemeral(
                    run_id.clone(),
                    limits.clone(),
                    Default::default(),
                )?;
                self.runtime.set_execution_context(context)?;
                self.execution_mode = ExecutionMode::Attached;
            }
            ExecutionMode::Restored => {
                return Err(
                    "Restored harness requires explicit execution scope reattachment".into(),
                )
            }
            ExecutionMode::Attached => {}
        }
        let context = self
            .runtime
            .execution_context()
            .cloned()
            .ok_or("Harness execution context unavailable")?;
        context.check_live()?;
        Ok(context)
    }

    async fn run_input(&mut self, history: Vec<Message>, input: Content) -> TaskResult<Content> {
        let context = match self.prepare_execution() {
            Ok(context) => context,
            Err(error) => return TaskResult::error(error, 0),
        };
        let tools = self.tools.clone();
        let policy = self.tool_policy.clone();
        self.runtime
            .run_with_context_and_tools(history, input, move |state, _, tool_call| {
                let tools = tools.clone();
                let policy = policy.clone();
                let context = context.clone();
                async move {
                    match tools
                        .execute_admitted(state.id, context, policy, tool_call)
                        .await
                    {
                        Ok(content) => TaskResult::success(content, 0),
                        Err(error) => TaskResult::error(error, 0),
                    }
                }
            })
            .await
    }
}

fn validate_run_id(id: &str) -> Result<(), String> {
    if id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        Err("Invalid harness run identity".into())
    } else {
        Ok(())
    }
}

fn text_content(input: impl Into<String>) -> Content {
    Content {
        text: input.into(),
        attachments: None,
        metadata: None,
    }
}
