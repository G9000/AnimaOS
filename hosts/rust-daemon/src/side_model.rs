//! The workspace side-task model: titles, compaction, the smart check and
//! learning reviews try it before the agent's own model.

use std::future::Future;

use anima_core::AgentConfig;
use tokio::time::Instant;
use tracing::warn;

use crate::app::SharedDaemonState;
use crate::fallback_model::call_with_fallbacks;
use crate::runtime_model::canonical_provider;

/// Runs `call` once per model to try. With a side model set, the first try
/// is `agent_config` with the side model's provider and model swapped in;
/// any error there, or no answer by halfway to `deadline`, runs `call` down
/// `agent_config`'s own fallback chain (the side model left out of it), whose
/// error then stands. Without one, or when it names the agent's own model,
/// only the chain runs. The caller still enforces `deadline` itself.
///
/// `call` must act on the config it is given and never on the agent's: a
/// metered call is priced by the config its adapter saw (`Metered` keeps it
/// per call), so a failed side try costs nothing and each later try is priced
/// as the model it ran.
pub(crate) async fn side_call<F, Fut, T>(
    state: &SharedDaemonState,
    agent_config: &AgentConfig,
    deadline: Instant,
    call: F,
) -> Result<T, String>
where
    F: Fn(AgentConfig) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let side = state.read().await.side_model.clone();
    let mut agent_config = agent_config.clone();
    if let Some(choice) = side {
        let is_side = |provider: &str, model: &str| {
            canonical_provider(provider) == canonical_provider(&choice.provider)
                && model.trim() == choice.model
        };
        let own_provider = agent_config.provider.as_deref().unwrap_or_default();
        if !is_side(own_provider, &agent_config.model) {
            let side_config = AgentConfig {
                provider: Some(choice.provider.clone()),
                model: choice.model.clone(),
                ..agent_config.clone()
            };
            // A hanging side model leaves the agent's models half the time.
            let now = Instant::now();
            let side_deadline = now + deadline.saturating_duration_since(now) / 2;
            let tried = tokio::time::timeout_at(side_deadline, call(side_config))
                .await
                .unwrap_or_else(|_| Err("the side-task model timed out".to_owned()));
            match tried {
                Ok(value) => return Ok(value),
                Err(error) => {
                    warn!(
                        provider = %choice.provider,
                        model = %choice.model,
                        error = %error,
                        "the side-task model failed; using the agent's own models"
                    );
                }
            }
            agent_config
                .fallbacks
                .retain(|fallback| !is_side(&fallback.provider, &fallback.model));
        }
    }
    call_with_fallbacks(&agent_config, call).await
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use anima_core::ModelChoice;
    use tokio::sync::RwLock;

    use super::*;
    use crate::agent_runs::test_support::companion_config;
    use crate::state::DaemonState;

    fn state_with(side: Option<(&str, &str)>) -> SharedDaemonState {
        let mut daemon = DaemonState::new();
        daemon.side_model = side.map(|(provider, model)| ModelChoice {
            provider: provider.into(),
            model: model.into(),
        });
        Arc::new(RwLock::new(daemon))
    }

    const RATE_LIMITED: &str = "OpenAI API error (429): slow down";

    /// Runs `side_call` with a call that fails for the models in `failing`
    /// and returns what it answered with and the configs it saw.
    async fn run(
        side: Option<(&str, &str)>,
        failing: &[&str],
    ) -> (Result<String, String>, Vec<(String, String)>) {
        let failing: Vec<(&str, String)> = failing
            .iter()
            .map(|model| (*model, format!("{model} failed")))
            .collect();
        run_on(companion_config("agent"), side, &failing).await
    }

    /// Like `run`, for `agent` and with each failing model's own error.
    async fn run_on(
        agent: AgentConfig,
        side: Option<(&str, &str)>,
        failing: &[(&str, String)],
    ) -> (Result<String, String>, Vec<(String, String)>) {
        let state = state_with(side);
        let seen = Mutex::new(Vec::new());
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        let result = side_call(&state, &agent, deadline, |config| {
            let model = config.model.clone();
            seen.lock()
                .unwrap()
                .push((config.provider.clone().unwrap_or_default(), model.clone()));
            let error = failing
                .iter()
                .find(|(failing, _)| *failing == model)
                .map(|(_, error)| error.clone());
            async move {
                match error {
                    Some(error) => Err(error),
                    None => Ok(model),
                }
            }
        })
        .await;
        (result, seen.into_inner().unwrap())
    }

    fn with_fallbacks(fallbacks: &[(&str, &str)]) -> AgentConfig {
        AgentConfig {
            fallbacks: fallbacks
                .iter()
                .map(|(provider, model)| ModelChoice {
                    provider: (*provider).into(),
                    model: (*model).into(),
                })
                .collect(),
            ..companion_config("agent")
        }
    }

    fn models(seen: &[(String, String)]) -> Vec<&str> {
        seen.iter().map(|(_, model)| model.as_str()).collect()
    }

    #[tokio::test]
    async fn after_the_side_model_the_agents_fallback_chain_is_tried() {
        let agent = with_fallbacks(&[("google", "gemini-x")]);
        let failing = [
            ("haiku", RATE_LIMITED.into()),
            ("gpt-5.4", RATE_LIMITED.into()),
        ];
        let (result, seen) = run_on(agent, Some(("anthropic", "haiku")), &failing).await;
        assert_eq!(result.as_deref(), Ok("gemini-x"));
        assert_eq!(models(&seen), ["haiku", "gpt-5.4", "gemini-x"]);
    }

    #[tokio::test]
    async fn without_a_side_model_the_agents_fallback_chain_is_tried() {
        let agent = with_fallbacks(&[("google", "gemini-x")]);
        let (result, seen) = run_on(agent, None, &[("gpt-5.4", RATE_LIMITED.into())]).await;
        assert_eq!(result.as_deref(), Ok("gemini-x"));
        assert_eq!(models(&seen), ["gpt-5.4", "gemini-x"]);
    }

    #[tokio::test]
    async fn the_chain_follows_the_runs_rules_for_whether_to_fall_back() {
        let agent = with_fallbacks(&[("google", "gemini-x")]);
        let (result, seen) = run_on(agent, None, &[("gpt-5.4", "gpt-5.4 failed".into())]).await;
        assert_eq!(result, Err("gpt-5.4 failed".to_string()));
        assert_eq!(models(&seen), ["gpt-5.4"], "a non-fallback error stands");
    }

    #[tokio::test]
    async fn a_side_model_that_failed_is_not_tried_again_from_the_chain() {
        let agent = with_fallbacks(&[("gemini", "haiku-g"), ("Anthropic", "haiku")]);
        let failing = [
            ("haiku", RATE_LIMITED.into()),
            ("gpt-5.4", RATE_LIMITED.into()),
            ("haiku-g", RATE_LIMITED.into()),
        ];
        let (result, seen) = run_on(agent, Some(("anthropic", "haiku")), &failing).await;
        assert_eq!(models(&seen), ["haiku", "gpt-5.4", "haiku-g"]);
        assert!(result.unwrap_err().starts_with(RATE_LIMITED));
    }

    #[tokio::test]
    async fn without_a_side_model_the_agents_config_runs_once() {
        let (result, seen) = run(None, &[]).await;
        assert_eq!(result.as_deref(), Ok("gpt-5.4"));
        assert_eq!(seen, [("openai".to_string(), "gpt-5.4".to_string())]);
        let (result, seen) = run(None, &["gpt-5.4"]).await;
        assert_eq!(result, Err("gpt-5.4 failed".to_string()));
        assert_eq!(seen.len(), 1, "no retry without a side model");
    }

    #[tokio::test]
    async fn the_side_model_runs_first_with_everything_else_the_agents() {
        let (result, seen) = run(Some(("anthropic", "haiku")), &[]).await;
        assert_eq!(result.as_deref(), Ok("haiku"));
        assert_eq!(seen, [("anthropic".to_string(), "haiku".to_string())]);
    }

    #[tokio::test]
    async fn a_failing_side_model_falls_back_to_the_agents_and_its_error_stands() {
        let (result, seen) = run(Some(("anthropic", "haiku")), &["haiku"]).await;
        assert_eq!(result.as_deref(), Ok("gpt-5.4"));
        assert_eq!(
            seen,
            [
                ("anthropic".to_string(), "haiku".to_string()),
                ("openai".to_string(), "gpt-5.4".to_string())
            ]
        );
        let (result, _) = run(Some(("anthropic", "haiku")), &["haiku", "gpt-5.4"]).await;
        assert_eq!(result, Err("gpt-5.4 failed".to_string()));
    }

    #[tokio::test]
    async fn a_hanging_side_model_gets_half_the_deadline_and_the_agents_model_the_rest() {
        let state = state_with(Some(("anthropic", "haiku")));
        let started = tokio::time::Instant::now();
        let deadline = started + std::time::Duration::from_millis(400);
        let answered = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            side_call(
                &state,
                &companion_config("agent"),
                deadline,
                |config| async move {
                    if config.model == "haiku" {
                        std::future::pending::<()>().await;
                    }
                    Ok::<_, String>(config.model)
                },
            ),
        )
        .await
        .expect("the side model's hang must not take the whole deadline");
        assert_eq!(answered.as_deref(), Ok("gpt-5.4"));
        let waited = started.elapsed();
        assert!(
            waited >= std::time::Duration::from_millis(200),
            "{waited:?}"
        );
        assert!(waited < std::time::Duration::from_millis(400), "{waited:?}");
    }

    #[tokio::test]
    async fn a_side_model_equal_to_the_agents_own_is_not_tried_twice() {
        let (result, seen) = run(Some(("OpenAI", "gpt-5.4")), &["gpt-5.4"]).await;
        assert_eq!(result, Err("gpt-5.4 failed".to_string()));
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn a_saved_control_plane_without_a_side_model_loads_without_one() {
        let mut payload =
            serde_json::to_value(DaemonState::new().control_plane_snapshot()).unwrap();
        payload.as_object_mut().unwrap().remove("sideModel");
        let mut restored = DaemonState::new();
        restored.side_model = Some(ModelChoice {
            provider: "openai".into(),
            model: "stale".into(),
        });
        restored
            .restore_control_plane_snapshot(serde_json::from_value(payload).unwrap())
            .expect("an old snapshot restores");
        assert_eq!(restored.side_model, None);
    }

    #[test]
    fn the_side_model_round_trips_through_the_control_plane() {
        let mut source = DaemonState::new();
        source.set_side_model(Some(ModelChoice {
            provider: "anthropic".into(),
            model: "haiku".into(),
        }));
        let payload = serde_json::to_value(source.control_plane_snapshot()).unwrap();
        assert_eq!(
            payload["sideModel"],
            serde_json::json!({"provider": "anthropic", "model": "haiku"})
        );
        let mut restored = DaemonState::new();
        restored
            .restore_control_plane_snapshot(serde_json::from_value(payload).unwrap())
            .unwrap();
        assert_eq!(restored.side_model, source.side_model);
    }
}
