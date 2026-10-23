use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use anima_core::execution_limits::{ExecutionContext, ExecutionLimits};
use anima_core::turn_recovery::{TurnBinding, TurnCheckpointSink};
use anima_core::{AgentConfig, AgentSettings, EngineEvent, ModelAdapter, RunControl, RunObserver};
use anima_model_adapters::{
    provider_definitions, ProviderAdapterConfig, ProviderCredential, ProviderDefinition,
    ProviderModelAdapter,
};

use crate::harness::Harness;
use crate::tool::{HarnessTool, ToolPolicy, ToolSet};

/// Errors returned by [`HarnessBuilder::build`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HarnessError {
    /// No model was configured via [`HarnessBuilder::model`].
    MissingModel,
    /// No provider was configured and no custom model adapter was supplied.
    MissingProvider,
    /// The provider id/alias is not in the anima-model-adapters catalog.
    UnknownProvider(String),
    /// Invalid limits or mismatched host execution/checkpoint identity.
    InvalidExecutionOptions(String),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingModel => {
                write!(formatter, "a model is required (use `.model(...)`)")
            }
            Self::MissingProvider => write!(
                formatter,
                "a provider is required (use `.provider(...)` or `.adapter(...)`)"
            ),
            Self::UnknownProvider(provider) => {
                write!(formatter, "unknown model provider: {provider}")
            }
            Self::InvalidExecutionOptions(error) => {
                write!(formatter, "invalid harness execution options: {error}")
            }
        }
    }
}

impl std::error::Error for HarnessError {}

#[derive(Default)]
pub(crate) struct HarnessControls {
    pub run_id: Option<String>,
    pub run_control: Option<RunControl>,
    pub observer: Option<Arc<dyn RunObserver>>,
    pub execution_limits: ExecutionLimits,
    pub execution_context: Option<ExecutionContext>,
    pub checkpoint: Option<(TurnBinding, Arc<dyn TurnCheckpointSink>)>,
    pub tool_policy: Option<Arc<dyn ToolPolicy>>,
}

/// Builder for [`Harness`].
///
/// Credential resolution order for a catalog provider:
/// 1. explicit `.api_key(...)` / `.base_url(...)` overrides,
/// 2. the provider's `api_key_envs` / `base_url_envs` environment variables,
/// 3. the provider's `default_base_url`.
pub struct HarnessBuilder {
    name: String,
    provider: Option<String>,
    model: Option<String>,
    system: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    temperature: Option<f64>,
    max_tokens: Option<u32>,
    max_tool_iterations: Option<usize>,
    tools: ToolSet,
    adapter: Option<Arc<dyn ModelAdapter>>,
    event_listener: Option<Arc<dyn Fn(EngineEvent) + Send + Sync>>,
    controls: HarnessControls,
}

impl HarnessBuilder {
    pub(crate) fn new() -> Self {
        Self {
            name: "harness-agent".to_owned(),
            provider: None,
            model: None,
            system: None,
            api_key: None,
            base_url: None,
            temperature: None,
            max_tokens: None,
            max_tool_iterations: None,
            tools: ToolSet::new(),
            adapter: None,
            event_listener: None,
            controls: HarnessControls::default(),
        }
    }

    /// Agent name. Defaults to `"harness-agent"`.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Provider id or alias from the anima-model-adapters catalog
    /// (e.g. `"anthropic"`, `"openai"`, `"ollama"`, `"gemini"`).
    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Model identifier passed through to the provider (e.g. `"gpt-4o-mini"`).
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// System prompt for the agent.
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Explicit API key override. Wins over the provider's `*_API_KEY` env vars.
    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Explicit base URL override. Wins over the provider's `*_BASE_URL` env vars.
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Cap on tool-calling turns inside a single run, followed by at most one
    /// tool-free wrap-up turn. Zero allows only that text response. Defaults
    /// to the core runtime default when unset.
    pub fn max_tool_iterations(mut self, max_tool_iterations: usize) -> Self {
        self.max_tool_iterations = Some(max_tool_iterations);
        self
    }

    /// Register a tool the model may call.
    pub fn tool(mut self, tool: impl HarnessTool + 'static) -> Self {
        self.tools.register(tool);
        self
    }

    /// Register a shared tool instance.
    pub fn tool_shared(mut self, tool: Arc<dyn HarnessTool>) -> Self {
        self.tools.register_shared(tool);
        self
    }

    /// Register every tool in an existing [`ToolSet`].
    pub fn tools(mut self, tools: ToolSet) -> Self {
        self.tools.extend(tools);
        self
    }

    /// Supply a custom [`ModelAdapter`] instead of the provider-backed one.
    /// Useful for tests and bespoke providers.
    pub fn adapter(mut self, adapter: Arc<dyn ModelAdapter>) -> Self {
        self.adapter = Some(adapter);
        self
    }

    /// Listen to every [`EngineEvent`] the runtime emits (spawn, tool calls,
    /// token usage, completion, ...).
    pub fn on_event(mut self, listener: impl Fn(EngineEvent) + Send + Sync + 'static) -> Self {
        self.event_listener = Some(Arc::new(listener));
        self
    }

    /// Bind this harness to one host run. Repeated calls share its finite
    /// allowance; omit this for independent ephemeral scopes per input.
    pub fn run_id(mut self, id: impl Into<String>) -> Self {
        self.controls.run_id = Some(id.into());
        self
    }

    /// Attach the host's Stop signal and steering inbox.
    pub fn run_control(mut self, control: RunControl) -> Self {
        self.controls.run_control = Some(control);
        self
    }

    /// Observe model usage, streaming output and tool results inline.
    pub fn run_observer(mut self, observer: Arc<dyn RunObserver>) -> Self {
        self.controls.observer = Some(observer);
        self
    }

    /// Finite limits for ephemeral scopes. Defaults to core ExecutionLimits.
    /// An explicit execution context retains its host's existing allowance.
    pub fn execution_limits(mut self, limits: ExecutionLimits) -> Self {
        self.controls.execution_limits = limits;
        self
    }

    /// Attach an existing shared allowance, including its current consumption.
    /// Calls share this context; the builder never resets or replaces it.
    pub fn execution_context(mut self, context: ExecutionContext) -> Self {
        self.controls.execution_context = Some(context);
        self
    }

    /// Attach trusted host checkpoint custody for one attempt. This selects a
    /// bound context whose run ID must equal `binding.attempt_run_id`.
    pub fn turn_checkpoint(
        mut self,
        binding: TurnBinding,
        sink: Arc<dyn TurnCheckpointSink>,
    ) -> Self {
        self.controls.checkpoint = Some((binding, sink));
        self
    }

    /// Require host approval and live revalidation for every actual tool call.
    /// Without a policy, registering a tool is trusted embedder authority.
    pub fn tool_policy(mut self, policy: Arc<dyn ToolPolicy>) -> Self {
        self.controls.tool_policy = Some(policy);
        self
    }

    pub fn build(self) -> Result<Harness, HarnessError> {
        let model = self
            .model
            .filter(|model| !model.trim().is_empty())
            .ok_or(HarnessError::MissingModel)?;

        let (adapter, provider) = match self.adapter {
            Some(adapter) => {
                let provider = self
                    .provider
                    .unwrap_or_else(|| adapter.provider().to_owned());
                (adapter, provider)
            }
            None => {
                let requested = self
                    .provider
                    .as_deref()
                    .map(str::trim)
                    .filter(|provider| !provider.is_empty())
                    .ok_or(HarnessError::MissingProvider)?
                    .to_ascii_lowercase();
                let definition = resolve_provider_definition(&requested)
                    .ok_or_else(|| HarnessError::UnknownProvider(requested.clone()))?;
                let credential = resolve_credential(definition, self.api_key, self.base_url);
                let adapter = ProviderModelAdapter::new(ProviderAdapterConfig {
                    providers: BTreeMap::from([(definition.id.to_owned(), credential)]),
                    ..ProviderAdapterConfig::default()
                });
                (
                    Arc::new(adapter) as Arc<dyn ModelAdapter>,
                    definition.id.to_owned(),
                )
            }
        };

        let has_tools = !self.tools.is_empty();
        let has_settings = self.temperature.is_some()
            || self.max_tokens.is_some()
            || self.max_tool_iterations.is_some();

        let config = AgentConfig {
            name: self.name,
            model,
            bio: None,
            lore: None,
            knowledge: None,
            topics: None,
            adjectives: None,
            style: None,
            provider: Some(provider),
            system: self.system,
            tools: has_tools.then(|| self.tools.descriptors()),
            plugins: None,
            settings: has_settings.then(|| AgentSettings {
                temperature: self.temperature,
                max_tokens: self.max_tokens,
                timeout_ms: None,
                max_retries: None,
                max_tool_iterations: self.max_tool_iterations,
                execution_limits: Some(self.controls.execution_limits.clone()),
                additional: BTreeMap::new(),
            }),
            fallbacks: Vec::new(),
        };

        Harness::new(
            config,
            adapter,
            self.tools,
            self.event_listener,
            self.controls,
        )
        .map_err(HarnessError::InvalidExecutionOptions)
    }
}

fn resolve_provider_definition(requested: &str) -> Option<&'static ProviderDefinition> {
    provider_definitions()
        .iter()
        .find(|definition| definition.id == requested || definition.aliases.contains(&requested))
}

fn resolve_credential(
    definition: &ProviderDefinition,
    api_key: Option<String>,
    base_url: Option<String>,
) -> ProviderCredential {
    let api_key = api_key
        .filter(|key| !key.trim().is_empty())
        .or_else(|| first_env(definition.api_key_envs));
    let base_url = base_url
        .filter(|url| !url.trim().is_empty())
        .or_else(|| first_env(definition.base_url_envs))
        .unwrap_or_else(|| definition.default_base_url.to_owned());
    ProviderCredential { api_key, base_url }
}

fn first_env(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .filter(|value| !value.trim().is_empty())
}
