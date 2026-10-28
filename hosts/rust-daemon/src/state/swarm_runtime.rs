use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anima_core::{
    AgentConfig, AgentRuntime, Content, DataValue, EngineEvent, Message, ModelAdapter,
    ModelGenerateRequest, ModelGenerateResponse, ModelStreamFrame, ModelStreamSink, Provider,
    ProviderResult, TokenUsage, ToolDescriptor,
};
use anima_swarm::coordinator::{
    CoordinatorAgentFactoryContext, CoordinatorAgentFactoryFn, CoordinatorAgentShell,
    CoordinatorBudgetCheckFn, CoordinatorInboxFn, CoordinatorParticipantsFn,
};
use async_trait::async_trait;
use tokio::sync::Mutex as AsyncMutex;

use crate::components::default_providers_in_zone;
use crate::events::EventFanout;
use crate::memory_embeddings::SharedMemoryEmbeddings;
use crate::tools::{ToolExecutionContext, ToolRegistry};

use super::runtime_events::publish_runtime_event;
use super::swarm_tools::execute_swarm_tool;
use super::{DaemonState, SharedMemoryStore};

impl DaemonState {
    /// Swarm runs going: a run holds its swarm's run lock through its final
    /// commit and publication (`routes::swarms`).
    pub(crate) fn running_swarm_count(&self) -> usize {
        self.swarm_run_locks
            .values()
            .filter(|lock| lock.try_lock().is_err())
            .count()
    }

    pub(super) fn swarm_agent_factory(
        &self,
        event_stream: EventFanout,
    ) -> Arc<CoordinatorAgentFactoryFn> {
        let memory = Arc::clone(&self.memory);
        let memory_embeddings = Arc::clone(&self.memory_embeddings);
        let memory_store = self.memory_store.clone();
        let model_adapter = self.profile_model_adapter();
        let profile_owner = self.profile_owner();
        let tool_registry = self.tool_registry.clone();
        let process_manager = Arc::clone(&self.process_manager);
        let workspace_root = self.workspace.as_ref().map(|w| w.root_path.clone());
        let calendar = self.calendar_manager.clone();
        let mail = self.mail_manager.clone();
        let drive = self.drive_manager.clone();
        let microsoft_files = self.microsoft_files_manager.clone();
        let document_edits = self.document_edit_service.clone();
        let db = self.db.clone();
        let owner_time_zone = self.owner_time_zone.clone();
        let execution_contexts = self.swarm_execution_contexts.clone();

        Arc::new(move |context: CoordinatorAgentFactoryContext| {
            let memory = Arc::clone(&memory);
            let memory_embeddings = Arc::clone(&memory_embeddings);
            let memory_store = memory_store.clone();
            let model_adapter = Arc::clone(&model_adapter);
            let profile_owner = profile_owner.clone();
            let tool_registry = tool_registry.clone();
            let process_manager = Arc::clone(&process_manager);
            let workspace_root = workspace_root.clone();
            let calendar = calendar.clone();
            let mail = mail.clone();
            let drive = drive.clone();
            let microsoft_files = microsoft_files.clone();
            let document_edits = document_edits.clone();
            let event_stream = event_stream.clone();
            let db = db.clone();
            let owner_time_zone = owner_time_zone.clone();
            let execution_contexts = execution_contexts.clone();
            let swarm_id = context.swarm_id.clone();

            Box::pin(async move {
                let config = with_swarm_messaging_tools(context.config.clone(), &tool_registry);
                let token_usage = Arc::new(Mutex::new(TokenUsage::default()));
                let model_adapter: Arc<dyn ModelAdapter> = Arc::new(BudgetedSwarmModel {
                    inner: model_adapter,
                    check_budget: context.check_budget.clone(),
                    usage: token_usage.clone(),
                });
                let tool_context = ToolExecutionContext::new(
                    Arc::clone(&memory),
                    Arc::clone(&memory_embeddings),
                    memory_store.clone(),
                    tool_registry,
                    process_manager,
                    workspace_root,
                    None,
                )
                .with_profile_owner(profile_owner)
                .with_owner_time_zone(owner_time_zone)
                .with_todo_owner(swarm_task_owner_id(&context.swarm_id, &context.agent_id));
                let runtime_events: Arc<dyn Fn(EngineEvent) + Send + Sync> = Arc::new({
                    let event_stream = event_stream.clone();
                    let agent_name = context.config.name.clone();
                    move |event: EngineEvent| {
                        publish_runtime_event(&event_stream, &agent_name, event);
                    }
                });
                let persistence_agent_id =
                    stable_swarm_persistence_agent_id(&context.swarm_id, &config.name);
                let inbox = context.inbox.clone();
                let participants = context.participants.clone();
                let mut initial_runtime = build_swarm_runtime(
                    persistence_agent_id.clone(),
                    config.clone(),
                    Arc::clone(&model_adapter),
                    Arc::clone(&memory),
                    Arc::clone(&memory_embeddings),
                    tool_context.owner_time_zone.clone(),
                    inbox.clone(),
                    participants.clone(),
                    Arc::clone(&runtime_events),
                );
                if let Some(db) = &db {
                    initial_runtime.set_database(Arc::clone(db));
                }
                if initial_runtime.config().allows_tool("todo_list") {
                    initial_runtime.register_provider(Arc::new(
                        crate::tools::todo::context::CanonicalTaskProvider {
                            root: tool_context.workspace_root.clone(),
                            owner_id: tool_context
                                .todo_owner_id(&initial_runtime.state())
                                .to_string(),
                        },
                    ));
                }
                let runtime = Arc::new(AsyncMutex::new(initial_runtime));
                let config = config.clone();
                let needs_reset = Arc::new(AtomicBool::new(false));

                Ok(CoordinatorAgentShell {
                    run: Arc::new({
                        let runtime = Arc::clone(&runtime);
                        let config = config.clone();
                        let memory = Arc::clone(&memory);
                        let model_adapter = Arc::clone(&model_adapter);
                        let token_usage = Arc::clone(&token_usage);
                        let needs_reset = Arc::clone(&needs_reset);
                        let delegate_task = context.delegate_task.clone();
                        let delegate_tasks = context.delegate_tasks.clone();
                        let send = context.send.clone();
                        let broadcast = context.broadcast.clone();
                        let inbox = inbox.clone();
                        let participants = participants.clone();
                        let tool_context = tool_context.clone();
                        let calendar = calendar.clone();
                        let mail = mail.clone();
                        let drive = drive.clone();
                        let microsoft_files = microsoft_files.clone();
                        let document_edits = document_edits.clone();
                        let runtime_events = Arc::clone(&runtime_events);
                        let db = db.clone();
                        let execution_contexts = execution_contexts.clone();
                        let swarm_id = swarm_id.clone();
                        move |input: Content| {
                            let execution_contexts = execution_contexts.clone();
                            let swarm_id = swarm_id.clone();
                            let runtime = Arc::clone(&runtime);
                            let persistence_agent_id = persistence_agent_id.clone();
                            let config = config.clone();
                            let memory = Arc::clone(&memory);
                            let memory_embeddings = Arc::clone(&memory_embeddings);
                            let model_adapter = Arc::clone(&model_adapter);
                            let token_usage = Arc::clone(&token_usage);
                            let needs_reset = Arc::clone(&needs_reset);
                            let delegate_task = delegate_task.clone();
                            let delegate_tasks = delegate_tasks.clone();
                            let send = send.clone();
                            let broadcast = broadcast.clone();
                            let inbox = inbox.clone();
                            let participants = participants.clone();
                            let tool_context = tool_context.clone();
                            let calendar = calendar.clone();
                            let mail = mail.clone();
                            let drive = drive.clone();
                            let microsoft_files = microsoft_files.clone();
                            let document_edits = document_edits.clone();
                            let runtime_events = Arc::clone(&runtime_events);
                            let db = db.clone();
                            Box::pin(async move {
                                let mut runtime = runtime.lock().await;
                                if needs_reset.swap(false, Ordering::AcqRel) {
                                    let mut new_runtime = build_swarm_runtime(
                                        persistence_agent_id,
                                        config,
                                        Arc::clone(&model_adapter),
                                        Arc::clone(&memory),
                                        Arc::clone(&memory_embeddings),
                                        tool_context.owner_time_zone.clone(),
                                        inbox,
                                        participants.clone(),
                                        Arc::clone(&runtime_events),
                                    );
                                    if let Some(db) = &db {
                                        new_runtime.set_database(Arc::clone(db));
                                    }
                                    if new_runtime.config().allows_tool("todo_list") {
                                        new_runtime.register_provider(Arc::new(
                                            crate::tools::todo::context::CanonicalTaskProvider {
                                                root: tool_context.workspace_root.clone(),
                                                owner_id: tool_context
                                                    .todo_owner_id(&new_runtime.state())
                                                    .to_string(),
                                            },
                                        ));
                                    }
                                    *runtime = new_runtime;
                                }
                                let todo_baseline = crate::tools::todo::startup_revision(
                                    crate::tools::ctx_workspace_root(&tool_context),
                                    tool_context.todo_owner_id(&runtime.state()),
                                    runtime.config(),
                                );
                                let execution = execution_contexts
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .get(&swarm_id)
                                    .cloned();
                                let Some(execution) = execution else {
                                    return anima_core::TaskResult::error(
                                        "Swarm execution authority is unavailable",
                                        0,
                                    );
                                };
                                if let Err(error) =
                                    runtime.begin_new_execution_context(execution.clone())
                                {
                                    return anima_core::TaskResult::error(error, 0);
                                }
                                // Idle shells retain weak handles; only this dispatch
                                // holds connector custody through its physical work.
                                let mut tool_context = tool_context
                                    .with_todo_baseline(todo_baseline)
                                    .with_execution_budget(execution, true)
                                    .with_mail(mail.as_ref().and_then(|manager| manager.upgrade()))
                                    .with_drive(
                                        drive.as_ref().and_then(|manager| manager.upgrade()),
                                    )
                                    .with_microsoft_files(
                                        microsoft_files
                                            .as_ref()
                                            .and_then(|manager| manager.upgrade()),
                                    )
                                    .with_document_edits(
                                        document_edits
                                            .as_ref()
                                            .and_then(|service| service.upgrade()),
                                    );
                                tool_context.calendar =
                                    calendar.as_ref().and_then(|manager| manager.upgrade());
                                let result = runtime
                                    .run_with_tools(input, |agent, user_message, tool_call| {
                                        let delegate_task = delegate_task.clone();
                                        let delegate_tasks = delegate_tasks.clone();
                                        let send = send.clone();
                                        let broadcast = broadcast.clone();
                                        let participants = participants.clone();
                                        let tool_context = tool_context.clone();
                                        async move {
                                            execute_swarm_tool(
                                                send,
                                                broadcast,
                                                participants,
                                                delegate_task,
                                                delegate_tasks,
                                                tool_context,
                                                agent,
                                                user_message,
                                                tool_call,
                                            )
                                            .await
                                        }
                                    })
                                    .await;

                                *token_usage
                                    .lock()
                                    .expect("swarm token mutex should not be poisoned") =
                                    runtime.snapshot().state.token_usage.clone();

                                result
                            })
                        }
                    }),
                    token_usage: Arc::new({
                        let token_usage = Arc::clone(&token_usage);
                        move || {
                            token_usage
                                .lock()
                                .expect("swarm token mutex should not be poisoned")
                                .clone()
                        }
                    }),
                    clear_task_state: Arc::new({
                        let needs_reset = Arc::clone(&needs_reset);
                        let token_usage = Arc::clone(&token_usage);
                        move || {
                            needs_reset.store(true, Ordering::Release);
                            *token_usage
                                .lock()
                                .expect("swarm token mutex should not be poisoned") =
                                TokenUsage::default();
                        }
                    }),
                    stop: Arc::new({
                        let runtime = Arc::clone(&runtime);
                        let token_usage = Arc::clone(&token_usage);
                        let execution_contexts = execution_contexts.clone();
                        let swarm_id = swarm_id.clone();
                        move || {
                            if let Some(execution) = execution_contexts
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .get(&swarm_id)
                            {
                                execution.cancel.cancel();
                            }
                            let runtime = Arc::clone(&runtime);
                            let token_usage = Arc::clone(&token_usage);
                            Box::pin(async move {
                                let mut runtime = runtime.lock().await;
                                runtime.stop();
                                *token_usage
                                    .lock()
                                    .expect("swarm token mutex should not be poisoned") =
                                    runtime.snapshot().state.token_usage.clone();
                            })
                        }
                    }),
                })
            })
        })
    }
}

struct BudgetedSwarmModel {
    inner: Arc<dyn ModelAdapter>,
    check_budget: Arc<CoordinatorBudgetCheckFn>,
    usage: Arc<Mutex<TokenUsage>>,
}

fn record_model_usage(usage: &Mutex<TokenUsage>, response: &TokenUsage) {
    let mut usage = usage
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    usage.saturating_add(response);
}

#[async_trait]
impl ModelAdapter for BudgetedSwarmModel {
    fn provider(&self) -> &str {
        self.inner.provider()
    }

    async fn generate_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        context: &anima_core::execution_limits::ModelCallContext,
    ) -> Result<ModelGenerateResponse, String> {
        (self.check_budget)()?;
        let response = self
            .inner
            .generate_controlled(config, request, context)
            .await?;
        record_model_usage(&self.usage, &response.usage);
        Ok(response)
    }

    async fn stream_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
        context: &anima_core::execution_limits::ModelCallContext,
    ) -> Result<(), String> {
        (self.check_budget)()?;
        self.inner
            .stream_controlled(
                config,
                request,
                &MeteredSwarmSink {
                    sink,
                    usage: &self.usage,
                },
                context,
            )
            .await
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        (self.check_budget)()?;
        let response = self.inner.generate(config, request).await?;
        // Publish before returning: the manager may delegate before its entire run finishes.
        record_model_usage(&self.usage, &response.usage);
        Ok(response)
    }

    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        (self.check_budget)()?;
        self.inner
            .stream(
                config,
                request,
                &MeteredSwarmSink {
                    sink,
                    usage: &self.usage,
                },
            )
            .await
    }
}

struct MeteredSwarmSink<'a> {
    sink: &'a dyn ModelStreamSink,
    usage: &'a Mutex<TokenUsage>,
}

#[async_trait]
impl ModelStreamSink for MeteredSwarmSink<'_> {
    async fn complete_usage(&self, usage: TokenUsage) -> Result<(), String> {
        self.sink.complete_usage(usage).await
    }
    async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
        if let ModelStreamFrame::Final(response) = &frame {
            record_model_usage(self.usage, &response.usage);
        }
        self.sink.emit(frame).await
    }
}

fn build_swarm_runtime(
    persistence_agent_id: String,
    config: AgentConfig,
    model_adapter: Arc<dyn ModelAdapter>,
    memory: SharedMemoryStore,
    memory_embeddings: SharedMemoryEmbeddings,
    owner_time_zone: crate::clock::OwnerTimeZone,
    inbox: Arc<CoordinatorInboxFn>,
    participants: Arc<CoordinatorParticipantsFn>,
    event_listener: Arc<dyn Fn(EngineEvent) + Send + Sync>,
) -> AgentRuntime {
    let mut runtime = AgentRuntime::new(config, model_adapter);
    runtime.set_persistence_agent_id(persistence_agent_id);
    runtime.set_event_listener(event_listener);
    let mut providers = default_providers_in_zone(
        Arc::clone(&memory),
        Arc::clone(&memory_embeddings),
        owner_time_zone,
    );
    providers.push(Arc::new(SwarmInboxProvider { inbox }));
    providers.push(Arc::new(SwarmParticipantsProvider { participants }));
    runtime.set_providers(providers);
    runtime.init();
    runtime
}

fn stable_swarm_persistence_agent_id(swarm_id: &str, agent_name: &str) -> String {
    format!("{swarm_id}:agent:{agent_name}")
}

fn swarm_task_owner_id(swarm_id: &str, member_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let tagged = format!("swarm:{swarm_id}:member:{member_id}");
    format!("swarm:{}", hex::encode(Sha256::digest(tagged.as_bytes())))
}

struct SwarmParticipantsProvider {
    participants: Arc<CoordinatorParticipantsFn>,
}

#[async_trait]
impl Provider for SwarmParticipantsProvider {
    fn name(&self) -> &str {
        "swarm_participants"
    }

    fn description(&self) -> &str {
        "Provides live swarm participant names and coordinator ids"
    }

    async fn get(
        &self,
        _runtime: &AgentRuntime,
        _message: &Message,
    ) -> Result<ProviderResult, String> {
        let participants = (self.participants)().await?;
        let text = if participants.is_empty() {
            "no live swarm participants".to_string()
        } else {
            participants
                .iter()
                .map(|participant| format!("{} ({})", participant.agent_name, participant.agent_id))
                .collect::<Vec<_>>()
                .join(" | ")
        };

        let mut metadata = BTreeMap::new();
        metadata.insert(
            "kind".into(),
            DataValue::String("swarm_participants".into()),
        );
        metadata.insert(
            "participantCount".into(),
            DataValue::Number(participants.len() as f64),
        );

        Ok(ProviderResult {
            text,
            metadata: Some(metadata),
        })
    }
}

struct SwarmInboxProvider {
    inbox: Arc<CoordinatorInboxFn>,
}

#[async_trait]
impl Provider for SwarmInboxProvider {
    fn name(&self) -> &str {
        "swarm_inbox"
    }

    fn description(&self) -> &str {
        "Provides messages delivered to this swarm agent"
    }

    async fn get(
        &self,
        _runtime: &AgentRuntime,
        _message: &Message,
    ) -> Result<ProviderResult, String> {
        let messages = (self.inbox)().await?;
        let text = if messages.is_empty() {
            "no swarm messages".to_string()
        } else {
            messages
                .iter()
                .map(|message| {
                    format!(
                        "from {} to {}: {}",
                        message.from, message.to, message.content.text
                    )
                })
                .collect::<Vec<_>>()
                .join(" | ")
        };

        let mut metadata = BTreeMap::new();
        metadata.insert("kind".into(), DataValue::String("swarm_inbox".into()));
        metadata.insert(
            "messageCount".into(),
            DataValue::Number(messages.len() as f64),
        );

        Ok(ProviderResult {
            text,
            metadata: Some(metadata),
        })
    }
}

fn with_swarm_messaging_tools(
    mut config: AgentConfig,
    tool_registry: &ToolRegistry,
) -> AgentConfig {
    let mut tools = config.tools.take().unwrap_or_default();
    let messaging_tools = tool_registry
        .resolve_descriptors(["send_message", "broadcast_message"])
        .expect("swarm messaging tools must be registered");
    for descriptor in messaging_tools {
        replace_or_push_tool(&mut tools, descriptor);
    }
    config.tools = Some(tools);
    config
}

fn replace_or_push_tool(tools: &mut Vec<ToolDescriptor>, descriptor: ToolDescriptor) {
    let mut replaced = false;
    for existing in tools.iter_mut().filter(|tool| tool.name == descriptor.name) {
        *existing = descriptor.clone();
        replaced = true;
    }
    if !replaced {
        tools.push(descriptor);
    }
}

#[cfg(test)]
mod tests {
    use super::with_swarm_messaging_tools;
    use crate::tools::todo::{read_agent_todos, write_agent_todos, TodoItem};
    use crate::tools::ToolRegistry;
    use anima_core::{
        AgentConfig, Content, DataValue, MessageRole, ModelAdapter, ModelGenerateRequest,
        ModelGenerateResponse, ModelStopReason, TaskStatus, ToolCall, ToolDescriptor,
    };
    use std::collections::{BTreeMap, VecDeque};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    async fn assert_registered_idle_swarm_releases_profile_custody(use_mail: bool) {
        let private = tempfile::tempdir().unwrap();
        let payload = private.path().join("control-plane.json");
        let mut daemon = crate::state::DaemonState::new();
        daemon.profile_owner =
            Some(crate::profile_owner::ProfileOwner::acquire([payload.clone()]).unwrap());
        let state = Arc::new(tokio::sync::RwLock::new(daemon));
        let runs = crate::agent_runs::AgentRunCoordinator::new(
            state.clone(),
            Arc::new(tokio::sync::Semaphore::new(1)),
        );
        let credentials =
            Arc::new(crate::connectors::gcalendar::store::InMemoryGoogleCredentialStore::default());
        let oauth_apps = crate::connectors::oauth_apps::OAuthAppService::in_memory();
        let (calendar, mail) = if use_mail {
            let manager = crate::connectors::mail::MailManager::new(
                &state,
                runs,
                credentials,
                Arc::new(crate::app::DeterministicMailTransport),
                oauth_apps,
            );
            state.write().await.mail_manager = Some(manager.weak());
            (None, Some(manager))
        } else {
            let manager = crate::connectors::gcalendar::CalendarManager::new(
                &state,
                runs,
                credentials,
                Arc::new(crate::connectors::gcalendar::client::UnconfiguredGoogleTransport),
                oauth_apps,
            );
            state
                .write()
                .await
                .set_calendar_manager(Some(manager.clone()));
            (Some(manager), None)
        };
        let (coordinator, events) = state.read().await.build_swarm(m82_config()).unwrap();
        state
            .write()
            .await
            .register_swarm(coordinator.clone(), events);
        coordinator.start().await.unwrap();
        assert!(
            !coordinator.get_state().agent_ids.is_empty(),
            "the registered swarm must retain actual started worker shells",
        );
        let weak_state = Arc::downgrade(&state);

        drop(coordinator);
        drop(calendar);
        drop(mail);
        drop(state);

        assert!(
            weak_state.upgrade().is_none(),
            "registered idle swarm retained daemon state through its connector manager",
        );
        assert!(crate::profile_owner::ProfileOwner::acquire([payload]).is_ok());
    }

    #[tokio::test]
    async fn profile_owner_idle_registered_swarm_does_not_retain_calendar_manager_custody() {
        assert_registered_idle_swarm_releases_profile_custody(false).await;
    }

    #[tokio::test]
    async fn profile_owner_idle_registered_swarm_does_not_retain_mail_manager_custody() {
        assert_registered_idle_swarm_releases_profile_custody(true).await;
    }

    struct M82Step {
        expected: Option<(&'static str, String)>,
        next: Option<ToolCall>,
        barrier: Option<Box<dyn Fn() + Send + Sync>>,
    }

    #[derive(Default)]
    struct M82TodoAdapter {
        steps: Mutex<VecDeque<M82Step>>,
        requests: Mutex<Vec<String>>,
        results: AtomicUsize,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ModelAdapter for M82TodoAdapter {
        fn single_attempt_contract(&self) -> bool {
            true
        }
        fn generated_usage_is_complete(&self, _: &anima_core::ModelGenerateResponse) -> bool {
            true
        }
        fn provider(&self) -> &str {
            "m82-script"
        }

        async fn generate(
            &self,
            _: &AgentConfig,
            request: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            let step = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("finite M82 script");
            self.calls.fetch_add(1, Ordering::SeqCst);
            let last = request.messages.last().expect("model request message");
            if let Some((status, text)) = step.expected {
                assert_eq!(last.role, MessageRole::Tool);
                let metadata = last.content.metadata.as_ref().expect("tool metadata");
                let DataValue::Object(result) = &metadata["taskResult"] else {
                    panic!("tool result envelope");
                };
                assert_eq!(result["status"], DataValue::String(status.into()));
                if status == "success" {
                    assert_eq!(last.content.text, text);
                } else {
                    assert!(last.content.text.contains(&text), "{}", last.content.text);
                }
                self.results.fetch_add(1, Ordering::SeqCst);
            } else {
                assert_ne!(
                    last.role,
                    MessageRole::Tool,
                    "a new run must begin with input"
                );
            }
            // Fixture owner updates happen only at this deterministic point,
            // after the actual preceding result has been inspected.
            if let Some(barrier) = step.barrier {
                barrier();
            }
            let tool_calls = step.next.map(|call| {
                self.requests.lock().unwrap().push(call.name.clone());
                vec![call]
            });
            let stop_reason = if tool_calls.is_some() {
                ModelStopReason::ToolCall
            } else {
                ModelStopReason::End
            };
            Ok(ModelGenerateResponse {
                content: Content {
                    text: "done".into(),
                    ..Default::default()
                },
                tool_calls,
                usage: Default::default(),
                stop_reason,
            })
        }
    }

    fn m82_step(expected: Option<(&'static str, String)>, next: Option<ToolCall>) -> M82Step {
        M82Step {
            expected,
            next,
            barrier: None,
        }
    }

    fn m82_tasks(contents: &[&str]) -> Vec<TodoItem> {
        contents
            .iter()
            .map(|content| TodoItem {
                parent_specified: false,
                id: None,
                parent: None,
                content: (*content).into(),
                active_form: (*content).into(),
                status: "pending".into(),
            })
            .collect()
    }

    fn m82_write(id: &str, contents: &[&str]) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "todo_write".into(),
            args: BTreeMap::from([(
                "todos".into(),
                DataValue::Array(
                    contents
                        .iter()
                        .map(|content| {
                            DataValue::Object(BTreeMap::from([
                                ("content".into(), DataValue::String((*content).into())),
                                ("activeForm".into(), DataValue::String((*content).into())),
                                ("status".into(), DataValue::String("pending".into())),
                            ]))
                        })
                        .collect(),
                ),
            )]),
        }
    }

    fn m82_read(id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "todo_read".into(),
            args: BTreeMap::new(),
        }
    }

    fn m82_render(contents: &[&str]) -> String {
        if contents.is_empty() {
            return "No todos set.".into();
        }
        contents
            .iter()
            .enumerate()
            .map(|(i, text)| format!("[ ] {}. [pending] {text}", i + 1))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn m82_updated(count: usize) -> String {
        format!("Todos updated (0 completed, 0 in progress, {count} pending). Proceed with current tasks.")
    }

    fn m82_read_write(before: &[&str], after: &[&str], tag: &str) -> Vec<M82Step> {
        vec![
            m82_step(None, Some(m82_read(&format!("{tag}-read")))),
            m82_step(
                Some(("success", m82_render(before))),
                Some(m82_write(&format!("{tag}-write"), after)),
            ),
            m82_step(Some(("success", m82_updated(after.len()))), None),
        ]
    }

    fn m82_state(root: &Path, adapter: Arc<M82TodoAdapter>) -> crate::state::DaemonState {
        let mut state = crate::state::DaemonState::with_model_adapter(adapter);
        state.workspace = Some(crate::control_plane_store::WorkspaceConfig {
            root_path: root.into(),
            company_name: "M82 fixture".into(),
            mission: String::new(),
            values: vec![],
        });
        state
    }

    /// These factory tests intentionally bypass HTTP/durable admission. Supply
    /// explicit finite ephemeral authority for one independent dispatch, shared
    /// by its members, without adding any production fallback authority.
    async fn dispatch_with_test_execution(
        state: &crate::state::DaemonState,
        coordinator: &anima_swarm::coordinator::SwarmCoordinator,
        task: &str,
    ) -> anima_core::TaskResult<Content> {
        let swarm_id = coordinator.get_state().id;
        let execution = anima_core::execution_limits::ExecutionContext::ephemeral(
            format!("test-swarm:{swarm_id}:{}", uuid::Uuid::new_v4()),
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let previous = state
            .swarm_execution_contexts
            .lock()
            .unwrap()
            .insert(swarm_id.clone(), execution);
        assert!(
            previous.is_none(),
            "a test must not replace active authority"
        );
        let result = coordinator.dispatch(task).await;
        state
            .swarm_execution_contexts
            .lock()
            .unwrap()
            .remove(&swarm_id)
            .expect("the dispatched operation retains its context")
            .cancel
            .cancel();
        result
    }

    #[tokio::test]
    async fn swarm_direct_dispatch_without_explicit_execution_context_does_not_call_model() {
        let root = tempfile::tempdir().unwrap();
        let adapter = Arc::new(M82TodoAdapter::default());
        let state = m82_state(root.path(), adapter.clone());
        let (coordinator, _) = state.build_swarm(m82_config()).unwrap();
        coordinator.start().await.unwrap();
        let result = coordinator.dispatch("unadmitted direct input").await;
        assert_eq!(result.status, TaskStatus::Error);
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("Swarm execution authority is unavailable"));
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 0);
        assert!(state.swarm_execution_contexts.lock().unwrap().is_empty());
        coordinator.stop().await.unwrap();
    }

    fn m82_config() -> anima_swarm::SwarmConfig {
        let registry = ToolRegistry::new();
        let mut member = agent_config();
        member.tools = Some(
            ["todo_read", "todo_write"]
                .into_iter()
                .map(|name| registry.descriptor(name).unwrap())
                .collect(),
        );
        anima_swarm::SwarmConfig {
            strategy: anima_swarm::SwarmStrategy::RoundRobin,
            manager: member.clone(),
            workers: vec![member],
            max_concurrent_agents: None,
            max_parallel_delegations: None,
            max_turns: Some(1),
            token_budget: None,
        }
    }

    fn m82_expected_owner(swarm_id: &str, member_id: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(b"swarm:");
        hash.update(swarm_id.as_bytes());
        hash.update(b":member:");
        hash.update(member_id.as_bytes());
        let digest: String = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("swarm:{digest}")
    }

    fn m82_owner(coordinator: &anima_swarm::coordinator::SwarmCoordinator) -> String {
        let state = coordinator.get_state();
        assert_eq!(state.agent_ids.len(), 1, "one real pooled member");
        m82_expected_owner(&state.id, &state.agent_ids[0])
    }

    fn m82_path(root: &Path, owner: &str) -> PathBuf {
        let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
        root.join(".animaos-swarm/agent-tasks")
            .join(format!("{encoded}.json"))
    }

    fn m82_assert_store(root: &Path, owner: &str, contents: &[&str]) -> String {
        use std::hash::{Hash, Hasher};
        let tasks = m82_tasks(contents);
        let bytes =
            std::fs::read(m82_path(root, owner)).expect("tagged coordinator/member task file");
        assert_eq!(bytes, serde_json::to_vec_pretty(&tasks).unwrap());
        let saved = read_agent_todos(Some(root), owner).unwrap();
        assert_eq!(saved.tasks, tasks);
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hash);
        assert_eq!(saved.revision, format!("{:016x}", hash.finish()));
        saved.revision
    }

    fn m82_assert_files(root: &Path, owners: &[&str]) {
        let mut actual = std::fs::read_dir(root.join(".animaos-swarm/agent-tasks"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        let mut expected = owners
            .iter()
            .map(|owner| m82_path(root, owner))
            .collect::<Vec<_>>();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected, "no transient-runtime task files");
    }

    fn m82_assert_script(adapter: &M82TodoAdapter, requests: &[&str], runs: usize) {
        assert!(adapter.steps.lock().unwrap().is_empty());
        assert_eq!(
            adapter
                .requests
                .lock()
                .unwrap()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            requests.to_vec()
        );
        assert_eq!(adapter.results.load(Ordering::SeqCst), requests.len());
        assert_eq!(adapter.calls.load(Ordering::SeqCst), requests.len() + runs);
    }

    #[tokio::test]
    async fn m82_real_dispatch_reset_retains_tasks_and_recaptures_external_baseline() {
        let root = tempfile::tempdir().unwrap();
        let adapter = Arc::new(M82TodoAdapter::default());
        let state = m82_state(root.path(), adapter.clone());
        let (coordinator, _) = state.build_swarm(m82_config()).unwrap();
        coordinator.start().await.unwrap();
        let owner = m82_owner(&coordinator);
        adapter.steps.lock().unwrap().extend([
            m82_step(None, Some(m82_write("first-write", &["task A"]))),
            m82_step(Some(("success", m82_updated(1))), None),
        ]);
        assert_eq!(
            dispatch_with_test_execution(&state, &coordinator, "first dispatch")
                .await
                .status,
            TaskStatus::Success
        );
        let first_snapshot = read_agent_todos(Some(root.path()), &owner).unwrap();
        let first_bytes = std::fs::read(m82_path(root.path(), &owner)).ok();

        adapter.steps.lock().unwrap().extend(m82_read_write(
            &["task A"],
            &["task A", "task B"],
            "second",
        ));
        assert_eq!(
            dispatch_with_test_execution(
                &state,
                &coordinator,
                "second dispatch resets the inner runtime",
            )
            .await
            .status,
            TaskStatus::Success
        );
        assert_eq!(m82_owner(&coordinator), owner);
        assert_eq!(first_snapshot.tasks, m82_tasks(&["task A"]));
        assert_eq!(
            first_bytes,
            Some(serde_json::to_vec_pretty(&m82_tasks(&["task A"])).unwrap())
        );
        let second_revision = m82_assert_store(root.path(), &owner, &["task A", "task B"]);
        assert_ne!(first_snapshot.revision, second_revision);

        write_agent_todos(
            Some(root.path()),
            &owner,
            &m82_tasks(&["external owner task"]),
            None,
        )
        .unwrap();
        // No todo_read precedes this write: only fresh startup capture can
        // authorize replacing the externally updated revision on this run.
        adapter.steps.lock().unwrap().extend([
            m82_step(
                None,
                Some(m82_write(
                    "fresh-baseline",
                    &["external owner task", "task C"],
                )),
            ),
            m82_step(
                Some(("success", m82_updated(2))),
                Some(m82_read("read-merged")),
            ),
            m82_step(
                Some(("success", m82_render(&["external owner task", "task C"]))),
                None,
            ),
        ]);
        assert_eq!(
            dispatch_with_test_execution(&state, &coordinator, "fresh startup baseline")
                .await
                .status,
            TaskStatus::Success
        );
        coordinator.stop().await.unwrap();
        m82_assert_store(root.path(), &owner, &["external owner task", "task C"]);
        m82_assert_files(root.path(), &[&owner]);
        m82_assert_script(
            &adapter,
            &[
                "todo_write",
                "todo_read",
                "todo_write",
                "todo_write",
                "todo_read",
            ],
            3,
        );
    }

    #[tokio::test]
    async fn m82_malformed_stable_tasks_refuse_until_explicit_successful_read() {
        let root = tempfile::tempdir().unwrap();
        let adapter = Arc::new(M82TodoAdapter::default());
        let state = m82_state(root.path(), adapter.clone());
        let (coordinator, _) = state.build_swarm(m82_config()).unwrap();
        coordinator.start().await.unwrap();
        let owner = m82_owner(&coordinator);
        let path = m82_path(root.path(), &owner);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{malformed stable tasks").unwrap();
        let refusal = "No task revision was observed for this run, so nothing was saved.";
        let mut steps = vec![
            m82_step(
                None,
                Some(m82_write("blind-malformed", &["blind replacement"])),
            ),
            m82_step(
                Some(("error", refusal.into())),
                Some(m82_read("read-malformed")),
            ),
            m82_step(
                Some(("error", "Could not decode agent tasks".into())),
                Some(m82_write("blind-repaired", &["blind replacement"])),
            ),
            m82_step(
                Some(("error", refusal.into())),
                Some(m82_read("read-repaired")),
            ),
            m82_step(
                Some(("success", m82_render(&["other owner task"]))),
                Some(m82_write(
                    "explicit-merge",
                    &["other owner task", "model task"],
                )),
            ),
            m82_step(Some(("success", m82_updated(2))), None),
        ];
        let unchanged = path.clone();
        steps[1].barrier = Some(Box::new(move || {
            assert_eq!(
                std::fs::read(&unchanged).unwrap(),
                b"{malformed stable tasks"
            )
        }));
        let repair_root = root.path().to_owned();
        let repair_owner = owner.clone();
        let malformed = path.clone();
        steps[2].barrier = Some(Box::new(move || {
            assert_eq!(
                std::fs::read(&malformed).unwrap(),
                b"{malformed stable tasks"
            );
            write_agent_todos(
                Some(&repair_root),
                &repair_owner,
                &m82_tasks(&["other owner task"]),
                None,
            )
            .unwrap();
        }));
        let repaired = path.clone();
        steps[3].barrier = Some(Box::new(move || {
            assert_eq!(
                std::fs::read(&repaired).unwrap(),
                serde_json::to_vec_pretty(&m82_tasks(&["other owner task"])).unwrap()
            )
        }));
        adapter.steps.lock().unwrap().extend(steps);
        assert_eq!(
            dispatch_with_test_execution(&state, &coordinator, "malformed repair recovery")
                .await
                .status,
            TaskStatus::Success
        );
        coordinator.stop().await.unwrap();
        m82_assert_store(root.path(), &owner, &["other owner task", "model task"]);
        m82_assert_files(root.path(), &[&owner]);
        m82_assert_script(
            &adapter,
            &[
                "todo_write",
                "todo_read",
                "todo_write",
                "todo_read",
                "todo_write",
            ],
            1,
        );
    }

    #[tokio::test]
    async fn m82_stale_cas_refreshes_the_same_stable_owner_before_explicit_merge() {
        let root = tempfile::tempdir().unwrap();
        let adapter = Arc::new(M82TodoAdapter::default());
        let state = m82_state(root.path(), adapter.clone());
        let (coordinator, _) = state.build_swarm(m82_config()).unwrap();
        coordinator.start().await.unwrap();
        let owner = m82_owner(&coordinator);
        write_agent_todos(
            Some(root.path()),
            &owner,
            &m82_tasks(&["startup task"]),
            None,
        )
        .unwrap();
        let mut initial = m82_step(None, Some(m82_write("stale-write", &["model task"])));
        let update_root = root.path().to_owned();
        let update_owner = owner.clone();
        initial.barrier = Some(Box::new(move || {
            write_agent_todos(
                Some(&update_root),
                &update_owner,
                &m82_tasks(&["new stable owner task"]),
                None,
            )
            .unwrap();
        }));
        let mut conflict = m82_step(
            Some(("error", "[pending] new stable owner task".into())),
            Some(m82_write(
                "merged-write",
                &["new stable owner task", "model task"],
            )),
        );
        let unchanged = m82_path(root.path(), &owner);
        conflict.barrier = Some(Box::new(move || {
            assert_eq!(
                std::fs::read(&unchanged).unwrap(),
                serde_json::to_vec_pretty(&m82_tasks(&["new stable owner task"])).unwrap()
            )
        }));
        adapter.steps.lock().unwrap().extend([
            initial,
            conflict,
            m82_step(Some(("success", m82_updated(2))), None),
        ]);
        assert_eq!(
            dispatch_with_test_execution(&state, &coordinator, "stale CAS")
                .await
                .status,
            TaskStatus::Success
        );
        coordinator.stop().await.unwrap();
        m82_assert_store(
            root.path(),
            &owner,
            &["new stable owner task", "model task"],
        );
        m82_assert_files(root.path(), &[&owner]);
        m82_assert_script(&adapter, &["todo_write", "todo_write"], 1);
    }

    #[tokio::test]
    async fn m82_same_name_members_in_different_swarms_keep_distinct_lists() {
        let root = tempfile::tempdir().unwrap();
        let adapter = Arc::new(M82TodoAdapter::default());
        let state = m82_state(root.path(), adapter.clone());
        let (first, _) = state.build_swarm(m82_config()).unwrap();
        let (second, _) = state.build_swarm(m82_config()).unwrap();
        first.start().await.unwrap();
        second.start().await.unwrap();
        let a = m82_owner(&first);
        let b = m82_owner(&second);
        assert_ne!(first.get_state().id, second.get_state().id);
        adapter
            .steps
            .lock()
            .unwrap()
            .extend(m82_read_write(&[], &["swarm A task"], "a-first"));
        assert_eq!(
            dispatch_with_test_execution(&state, &first, "first A")
                .await
                .status,
            TaskStatus::Success
        );
        adapter
            .steps
            .lock()
            .unwrap()
            .extend(m82_read_write(&[], &["swarm B task"], "b-first"));
        assert_eq!(
            dispatch_with_test_execution(&state, &second, "first B")
                .await
                .status,
            TaskStatus::Success
        );
        adapter.steps.lock().unwrap().extend(m82_read_write(
            &["swarm A task"],
            &["swarm A task", "A second task"],
            "a-second",
        ));
        assert_eq!(
            dispatch_with_test_execution(&state, &first, "repeat A")
                .await
                .status,
            TaskStatus::Success
        );
        adapter.steps.lock().unwrap().extend(m82_read_write(
            &["swarm B task"],
            &["swarm B task", "B second task"],
            "b-second",
        ));
        assert_eq!(
            dispatch_with_test_execution(&state, &second, "repeat B")
                .await
                .status,
            TaskStatus::Success
        );
        first.stop().await.unwrap();
        second.stop().await.unwrap();
        let a_revision = m82_assert_store(root.path(), &a, &["swarm A task", "A second task"]);
        let b_revision = m82_assert_store(root.path(), &b, &["swarm B task", "B second task"]);
        assert_ne!(a_revision, b_revision);
        m82_assert_files(root.path(), &[&a, &b]);
        m82_assert_script(
            &adapter,
            &[
                "todo_read",
                "todo_write",
                "todo_read",
                "todo_write",
                "todo_read",
                "todo_write",
                "todo_read",
                "todo_write",
            ],
            4,
        );
    }

    #[tokio::test]
    async fn m82_distinct_same_name_members_and_new_ephemeral_members_never_inherit_tasks() {
        use anima_swarm::coordinator::{CoordinatorStrategyFn, SwarmCoordinator};
        let root = tempfile::tempdir().unwrap();
        let adapter = Arc::new(M82TodoAdapter::default());
        let state = m82_state(root.path(), adapter.clone());
        let observed = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
        let members = observed.clone();
        let strategy: Arc<CoordinatorStrategyFn> = Arc::new(move |context| {
            let members = members.clone();
            Box::pin(async move {
                // No worker pool: use actual run references to distinguish two
                // equal display names without changing name routing.
                let first = context
                    .spawn_agent(context.manager_config().clone())
                    .await
                    .unwrap();
                let second = context
                    .spawn_agent(context.manager_config().clone())
                    .await
                    .unwrap();
                members
                    .lock()
                    .unwrap()
                    .push(vec![first.id.clone(), second.id.clone()]);
                assert_eq!(
                    first.run("first member".into()).await.status,
                    TaskStatus::Success
                );
                assert_eq!(
                    second.run("second member".into()).await.status,
                    TaskStatus::Success
                );
                first.run("first member again".into()).await
            })
        });
        let mut config = m82_config();
        config.workers.clear();
        let coordinator = SwarmCoordinator::with_hooks(
            config,
            strategy,
            state.swarm_agent_factory(crate::events::EventFanout::new(32)),
        );
        coordinator.start().await.unwrap();
        let swarm_id = coordinator.get_state().id;
        for (tag, first, second, extra) in [
            ("old", "old A", "old B", "old A second"),
            ("new", "new A", "new B", "new A second"),
        ] {
            let mut script = m82_read_write(&[], &[first], &format!("{tag}-a"));
            script.extend(m82_read_write(&[], &[second], &format!("{tag}-b")));
            script.extend(m82_read_write(
                &[first],
                &[first, extra],
                &format!("{tag}-a-again"),
            ));
            adapter.steps.lock().unwrap().extend(script);
            assert_eq!(
                dispatch_with_test_execution(&state, &coordinator, tag)
                    .await
                    .status,
                TaskStatus::Success
            );
        }
        coordinator.stop().await.unwrap();
        let ids = observed
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 4);
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            4
        );
        let owners = ids
            .iter()
            .map(|id| m82_expected_owner(&swarm_id, id))
            .collect::<Vec<_>>();
        for (owner, contents) in owners.iter().zip([
            vec!["old A", "old A second"],
            vec!["old B"],
            vec!["new A", "new A second"],
            vec!["new B"],
        ]) {
            m82_assert_store(root.path(), owner, &contents);
        }
        m82_assert_files(
            root.path(),
            &owners.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        m82_assert_script(&adapter, &["todo_read", "todo_write"].repeat(6), 6);
    }

    #[tokio::test]
    async fn m82_long_member_name_keeps_real_task_tools_and_reset_continuity_usable() {
        let root = tempfile::tempdir().unwrap();
        let adapter = Arc::new(M82TodoAdapter::default());
        let state = m82_state(root.path(), adapter.clone());
        let mut config = m82_config();
        config.manager.name = "x".repeat(100);
        config.workers[0].name = config.manager.name.clone();
        let (coordinator, _) = state.build_swarm(config).unwrap();
        coordinator.start().await.unwrap();
        let owner = m82_owner(&coordinator);
        adapter.steps.lock().unwrap().extend(m82_read_write(
            &[],
            &["long-name task A"],
            "long-first",
        ));
        assert_eq!(
            dispatch_with_test_execution(&state, &coordinator, "long name first dispatch")
                .await
                .status,
            TaskStatus::Success
        );
        adapter.steps.lock().unwrap().extend(m82_read_write(
            &["long-name task A"],
            &["long-name task A", "long-name task B"],
            "long-second",
        ));
        assert_eq!(
            dispatch_with_test_execution(&state, &coordinator, "long name second dispatch")
                .await
                .status,
            TaskStatus::Success
        );
        assert_eq!(m82_owner(&coordinator), owner);
        coordinator.stop().await.unwrap();
        assert!(
            m82_path(root.path(), &owner)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .len()
                <= 255
        );
        m82_assert_store(
            root.path(),
            &owner,
            &["long-name task A", "long-name task B"],
        );
        m82_assert_files(root.path(), &[&owner]);
        m82_assert_script(
            &adapter,
            &["todo_read", "todo_write", "todo_read", "todo_write"],
            2,
        );
    }

    struct DelegatingAdapter(std::sync::atomic::AtomicUsize);

    #[async_trait::async_trait]
    impl anima_core::ModelAdapter for DelegatingAdapter {
        fn single_attempt_contract(&self) -> bool {
            true
        }
        fn generated_usage_is_complete(&self, _: &anima_core::ModelGenerateResponse) -> bool {
            true
        }
        fn provider(&self) -> &str {
            "test"
        }

        async fn generate(
            &self,
            config: &AgentConfig,
            request: &anima_core::ModelGenerateRequest,
        ) -> Result<anima_core::ModelGenerateResponse, String> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let delegate = config.name == "manager"
                && !request
                    .messages
                    .iter()
                    .any(|m| m.role == anima_core::MessageRole::Tool);
            Ok(anima_core::ModelGenerateResponse {
                content: anima_core::Content {
                    text: "done".into(),
                    ..Default::default()
                },
                tool_calls: delegate.then(|| {
                    vec![anima_core::ToolCall {
                        id: "delegate-1".into(),
                        name: "delegate_task".into(),
                        args: std::collections::BTreeMap::from([
                            (
                                "worker_name".into(),
                                anima_core::DataValue::String("worker".into()),
                            ),
                            (
                                "task".into(),
                                anima_core::DataValue::String("research".into()),
                            ),
                        ]),
                    }]
                }),
                usage: anima_core::TokenUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                    ..anima_core::TokenUsage::default()
                },
                stop_reason: if delegate {
                    anima_core::ModelStopReason::ToolCall
                } else {
                    anima_core::ModelStopReason::End
                },
            })
        }
    }

    async fn run_budgeted_supervisor(budget: u64) -> (anima_core::TaskStatus, usize, u64) {
        let adapter =
            std::sync::Arc::new(DelegatingAdapter(std::sync::atomic::AtomicUsize::new(0)));
        let state = crate::state::DaemonState::with_model_adapter(adapter.clone());
        let mut manager = agent_config();
        manager.name = "manager".into();
        let (coordinator, _) = state
            .build_swarm(anima_swarm::SwarmConfig {
                strategy: anima_swarm::SwarmStrategy::Supervisor,
                manager,
                workers: vec![agent_config()],
                max_concurrent_agents: None,
                max_parallel_delegations: Some(1),
                max_turns: Some(4),
                token_budget: Some(budget),
            })
            .unwrap();
        coordinator.start().await.unwrap();
        let result = dispatch_with_test_execution(&state, &coordinator, "work").await;
        (
            result.status,
            adapter.0.load(std::sync::atomic::Ordering::SeqCst),
            coordinator.get_state().token_usage.total_tokens,
        )
    }

    #[tokio::test]
    async fn manager_usage_is_visible_before_delegation_and_blocks_more_model_calls() {
        assert_eq!(
            run_budgeted_supervisor(5).await,
            (anima_core::TaskStatus::Error, 1, 5)
        );
    }

    #[tokio::test]
    async fn worker_usage_blocks_manager_follow_up_but_sufficient_budget_completes() {
        assert_eq!(
            run_budgeted_supervisor(10).await,
            (anima_core::TaskStatus::Error, 2, 10)
        );
        assert_eq!(
            run_budgeted_supervisor(20).await,
            (anima_core::TaskStatus::Success, 3, 15)
        );
    }

    #[tokio::test]
    async fn budget_wrapper_preserves_stream_frames_and_accounts_before_next_request() {
        use super::*;
        struct StreamingOnly;
        #[async_trait]
        impl ModelAdapter for StreamingOnly {
            fn single_attempt_contract(&self) -> bool {
                true
            }
            fn generated_usage_is_complete(&self, _: &anima_core::ModelGenerateResponse) -> bool {
                true
            }
            fn provider(&self) -> &str {
                "streaming-test"
            }
            async fn generate(
                &self,
                _: &AgentConfig,
                _: &ModelGenerateRequest,
            ) -> Result<ModelGenerateResponse, String> {
                Err("stream must not fall back to generate".into())
            }
            async fn stream(
                &self,
                _: &AgentConfig,
                _: &ModelGenerateRequest,
                sink: &dyn ModelStreamSink,
            ) -> Result<(), String> {
                sink.emit(ModelStreamFrame::TextDelta("hello".into()))
                    .await?;
                let response = ModelGenerateResponse {
                    content: Content {
                        text: "hello".into(),
                        ..Default::default()
                    },
                    tool_calls: None,
                    usage: TokenUsage {
                        prompt_tokens: 3,
                        completion_tokens: 2,
                        total_tokens: 5,
                        ..TokenUsage::default()
                    },
                    stop_reason: anima_core::ModelStopReason::End,
                };
                sink.complete_usage(response.usage.clone()).await?;
                sink.emit(ModelStreamFrame::Final(response)).await
            }
        }

        struct RecordingSink(Mutex<Vec<ModelStreamFrame>>);
        #[async_trait]
        impl ModelStreamSink for RecordingSink {
            async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
                self.0.lock().unwrap().push(frame);
                Ok(())
            }
        }
        let usage = Arc::new(Mutex::new(TokenUsage::default()));
        let observed = usage.clone();
        let model = BudgetedSwarmModel {
            inner: Arc::new(StreamingOnly),
            usage: usage.clone(),
            check_budget: Arc::new(move || {
                if observed.lock().unwrap().total_tokens >= 5 {
                    Err("budget exhausted".into())
                } else {
                    Ok(())
                }
            }),
        };
        let request = ModelGenerateRequest {
            system: String::new(),
            messages: vec![],
            temperature: None,
            max_tokens: None,
        };
        let sink = RecordingSink(Mutex::new(Vec::new()));
        assert_eq!(model.provider(), "streaming-test");
        model
            .stream(&agent_config(), &request, &sink)
            .await
            .unwrap();
        assert_eq!(usage.lock().unwrap().total_tokens, 5);
        assert_eq!(
            sink.0.lock().unwrap()[0],
            ModelStreamFrame::TextDelta("hello".into())
        );
        assert!(model
            .stream(&agent_config(), &request, &sink)
            .await
            .is_err());
        assert_eq!(sink.0.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn budgeted_swarm_model_records_cached_and_reasoning_tokens() {
        use super::*;
        struct WithCacheAndReasoning;
        #[async_trait]
        impl ModelAdapter for WithCacheAndReasoning {
            fn single_attempt_contract(&self) -> bool {
                true
            }
            fn generated_usage_is_complete(&self, _: &anima_core::ModelGenerateResponse) -> bool {
                true
            }
            fn provider(&self) -> &str {
                "cache-test"
            }
            async fn generate(
                &self,
                _: &AgentConfig,
                _: &ModelGenerateRequest,
            ) -> Result<ModelGenerateResponse, String> {
                Ok(ModelGenerateResponse {
                    content: Content {
                        text: "hello".into(),
                        ..Default::default()
                    },
                    tool_calls: None,
                    usage: TokenUsage {
                        prompt_tokens: 3,
                        completion_tokens: 2,
                        total_tokens: 5,
                        cached_prompt_tokens: 7,
                        reasoning_tokens: 9,
                    },
                    stop_reason: anima_core::ModelStopReason::End,
                })
            }
            async fn stream(
                &self,
                _: &AgentConfig,
                _: &ModelGenerateRequest,
                _: &dyn ModelStreamSink,
            ) -> Result<(), String> {
                Err("generate-only adapter".into())
            }
        }

        let usage = Arc::new(Mutex::new(TokenUsage::default()));
        let model = BudgetedSwarmModel {
            inner: Arc::new(WithCacheAndReasoning),
            usage: usage.clone(),
            check_budget: Arc::new(|| Ok(())),
        };
        let request = ModelGenerateRequest {
            system: String::new(),
            messages: vec![],
            temperature: None,
            max_tokens: None,
        };

        model.generate(&agent_config(), &request).await.unwrap();

        let recorded = usage.lock().unwrap();
        assert_eq!(recorded.total_tokens, 5);
        assert_eq!(recorded.cached_prompt_tokens, 7);
        assert_eq!(recorded.reasoning_tokens, 9);
    }

    #[test]
    fn swarm_messaging_tools_use_registry_descriptors() {
        let registry = ToolRegistry::new();
        let config = AgentConfig {
            name: "worker".into(),
            model: "deterministic".into(),
            bio: None,
            lore: None,
            knowledge: None,
            topics: None,
            adjectives: None,
            style: None,
            provider: None,
            system: None,
            tools: None,
            plugins: None,
            settings: None,
            fallbacks: Vec::new(),
        };

        let config = with_swarm_messaging_tools(config, &registry);
        let tools = config.tools.expect("swarm tools");

        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["send_message", "broadcast_message"]
        );
        for name in ["send_message", "broadcast_message"] {
            assert_eq!(
                tools.iter().find(|tool| tool.name == name),
                registry.descriptor(name).as_ref(),
                "swarm runtime descriptor for {name} drifted from the registry"
            );
        }
    }

    #[test]
    fn swarm_messaging_tools_replace_existing_entries_in_place() {
        let registry = ToolRegistry::new();
        let mut config = agent_config();
        config.tools = Some(vec![
            untrusted_descriptor("broadcast_message"),
            untrusted_descriptor("custom_tool"),
            untrusted_descriptor("send_message"),
            untrusted_descriptor("send_message"),
        ]);

        let config = with_swarm_messaging_tools(config, &registry);
        let tools = config.tools.expect("swarm tools");

        assert_eq!(tools.len(), 4);
        assert_eq!(
            tools[0],
            registry
                .descriptor("broadcast_message")
                .expect("broadcast descriptor")
        );
        assert_eq!(tools[1], untrusted_descriptor("custom_tool"));
        assert_eq!(
            tools[2],
            registry
                .descriptor("send_message")
                .expect("send descriptor")
        );
        assert_eq!(
            tools[3],
            registry
                .descriptor("send_message")
                .expect("duplicate send descriptor")
        );
    }

    fn untrusted_descriptor(name: &str) -> ToolDescriptor {
        ToolDescriptor {
            name: name.into(),
            description: "Caller-provided descriptor".into(),
            parameters_schema: Default::default(),
            examples: None,
        }
    }

    fn agent_config() -> AgentConfig {
        AgentConfig {
            name: "worker".into(),
            model: "deterministic".into(),
            bio: None,
            lore: None,
            knowledge: None,
            topics: None,
            adjectives: None,
            style: None,
            provider: None,
            system: None,
            tools: None,
            plugins: None,
            settings: None,
            fallbacks: Vec::new(),
        }
    }
}
