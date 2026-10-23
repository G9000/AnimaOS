use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize};

use crate::ModelChoice;

/// Owner-configured authority and finite defaults for a run's helper agents.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct DelegationPolicy {
    pub max_concurrent: usize,
    pub max_starts_per_run: usize,
    pub max_depth: usize,
    pub routes: Vec<DelegationRoute>,
    pub allow_context_fork: bool,
    pub max_tool_iterations: usize,
    pub timeout_ms: u64,
    pub max_output_tokens: u32,
}

impl Default for DelegationPolicy {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            max_starts_per_run: 4,
            max_depth: 1,
            routes: Vec::new(),
            allow_context_fork: false,
            max_tool_iterations: 8,
            timeout_ms: 120_000,
            max_output_tokens: 4_096,
        }
    }
}

impl DelegationPolicy {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=8).contains(&self.max_concurrent) {
            return Err("delegation maxConcurrent must be between 1 and 8");
        }
        if !(1..=16).contains(&self.max_starts_per_run) {
            return Err("delegation maxStartsPerRun must be between 1 and 16");
        }
        if !(1..=3).contains(&self.max_depth) {
            return Err("delegation maxDepth must be between 1 and 3");
        }
        if !(1..=32).contains(&self.max_tool_iterations) {
            return Err("delegation maxToolIterations must be between 1 and 32");
        }
        if !(1_000..=600_000).contains(&self.timeout_ms) {
            return Err("delegation timeoutMs must be between 1000 and 600000");
        }
        if !(1..=16_384).contains(&self.max_output_tokens) {
            return Err("delegation maxOutputTokens must be between 1 and 16384");
        }
        if self.routes.len() > 8 {
            return Err("delegation may list at most 8 routes");
        }
        let mut ids = BTreeSet::new();
        for route in &self.routes {
            route.validate()?;
            if !ids.insert(&route.id) {
                return Err("delegation route ids must be unique");
            }
        }
        Ok(())
    }
}

/// A named route chosen by its id; credentials and endpoints stay with the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DelegationRoute {
    pub id: String,
    pub provider: String,
    pub model: String,
    #[serde(default, deserialize_with = "deserialize_fallbacks")]
    pub fallbacks: Vec<ModelChoice>,
}

impl DelegationRoute {
    pub fn validate(&self) -> Result<(), &'static str> {
        let id = self.id.as_bytes();
        if id.is_empty()
            || id.len() > 32
            || !id[0].is_ascii_lowercase()
            || !id.iter().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'_' | b'-')
            })
        {
            return Err("delegation route id must match [a-z][a-z0-9_-]{0,31}");
        }
        validate_model_choice(&self.provider, &self.model)?;
        if self.fallbacks.len() > 3 {
            return Err("delegation routes may list at most 3 fallbacks");
        }
        let mut choices =
            BTreeSet::from([(self.provider.trim().to_ascii_lowercase(), self.model.trim())]);
        for choice in &self.fallbacks {
            validate_model_choice(&choice.provider, &choice.model)?;
            if !choices.insert((
                choice.provider.trim().to_ascii_lowercase(),
                choice.model.trim(),
            )) {
                return Err("delegation route model choices must be unique");
            }
        }
        Ok(())
    }
}

fn validate_model_choice(provider: &str, model: &str) -> Result<(), &'static str> {
    if provider.trim().is_empty() || provider.len() > 64 {
        return Err("delegation provider must contain between 1 and 64 bytes");
    }
    if model.trim().is_empty() || model.len() > 256 {
        return Err("delegation model must contain between 1 and 256 bytes");
    }
    Ok(())
}

fn deserialize_fallbacks<'de, D>(deserializer: D) -> Result<Vec<ModelChoice>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Choice {
        provider: String,
        model: String,
    }
    Ok(Vec::<Choice>::deserialize(deserializer)?
        .into_iter()
        .map(|choice| ModelChoice {
            provider: choice.provider,
            model: choice.model,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn delegation_defaults_are_finite_and_round_trip() {
        let policy: DelegationPolicy = serde_json::from_value(json!({})).unwrap();
        assert_eq!(policy, DelegationPolicy::default());
        assert_eq!(
            (
                policy.max_concurrent,
                policy.max_starts_per_run,
                policy.max_depth
            ),
            (4, 4, 1)
        );
        assert_eq!(
            (
                policy.max_tool_iterations,
                policy.timeout_ms,
                policy.max_output_tokens
            ),
            (8, 120_000, 4_096)
        );
        assert!(!policy.allow_context_fork);
        assert!(policy.validate().is_ok());
        assert_eq!(
            serde_json::from_value::<DelegationPolicy>(serde_json::to_value(&policy).unwrap())
                .unwrap(),
            policy
        );
    }

    #[test]
    fn delegation_rejects_limits_outside_every_finite_bound() {
        for (field, low, high) in [
            ("maxConcurrent", 0, 9),
            ("maxStartsPerRun", 0, 17),
            ("maxDepth", 0, 4),
            ("maxToolIterations", 0, 33),
            ("timeoutMs", 999, 600_001),
            ("maxOutputTokens", 0, 16_385),
        ] {
            for value in [low, high] {
                let policy: DelegationPolicy =
                    serde_json::from_value(json!({(field): value})).unwrap();
                assert!(policy.validate().is_err(), "accepted {field}={value}");
            }
        }
    }

    #[test]
    fn delegation_accepts_the_configured_upper_bounds_and_distinct_fallbacks() {
        let policy = DelegationPolicy {
            max_concurrent: 8,
            max_starts_per_run: 16,
            max_depth: 3,
            max_tool_iterations: 32,
            timeout_ms: 600_000,
            max_output_tokens: 16_384,
            routes: (0..8)
                .map(|index| DelegationRoute {
                    id: format!("route-{index}"),
                    provider: "openai".into(),
                    model: "primary".into(),
                    fallbacks: (0..3)
                        .map(|fallback| ModelChoice {
                            provider: "openai".into(),
                            model: format!("fallback-{fallback}"),
                        })
                        .collect(),
                })
                .collect(),
            ..Default::default()
        };
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn delegation_routes_are_bounded_unique_and_strict() {
        let route = json!({"id":"review", "provider":"openai", "model":"model-a", "fallbacks":[]});
        let policy: DelegationPolicy =
            serde_json::from_value(json!({"routes":[route.clone()]})).unwrap();
        assert!(policy.validate().is_ok());
        for id in [
            "",
            "Review",
            "with space",
            "_review",
            "abcdefghijklmnopqrstuvwxyz1234567",
        ] {
            let mut invalid = route.clone();
            invalid["id"] = json!(id);
            assert!(serde_json::from_value::<DelegationRoute>(invalid)
                .unwrap()
                .validate()
                .is_err());
        }
        for routes in [vec![route.clone(); 2], vec![route.clone(); 9]] {
            assert!(
                serde_json::from_value::<DelegationPolicy>(json!({"routes": routes}))
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        for (provider, model) in [
            (" ".to_string(), "a".to_string()),
            ("p".repeat(65), "a".to_string()),
            ("openai".to_string(), " ".to_string()),
            ("openai".to_string(), "m".repeat(257)),
        ] {
            let mut invalid = route.clone();
            invalid["provider"] = json!(provider);
            invalid["model"] = json!(model);
            assert!(serde_json::from_value::<DelegationRoute>(invalid)
                .unwrap()
                .validate()
                .is_err());
        }
        for fallbacks in [
            json!([{"provider":"OPENAI", "model":"model-a"}]),
            json!([{"provider":"openai", "model":"b"}, {"provider":"openai", "model":"b"}]),
            json!(vec![json!({"provider":"openai", "model":"b"}); 4]),
        ] {
            let mut invalid = route.clone();
            invalid["fallbacks"] = fallbacks;
            assert!(serde_json::from_value::<DelegationRoute>(invalid)
                .unwrap()
                .validate()
                .is_err());
        }
        for invalid in [
            json!({"endpoint":"https://invalid.example"}),
            json!({"routes":[{"id":"review", "provider":"openai", "model":"a", "apiKey":"secret"}]}),
            json!({"routes":[{"id":"review", "provider":"openai", "model":"a", "fallbacks":[{"provider":"openai", "model":"b", "endpoint":"x"}]}]}),
        ] {
            assert!(serde_json::from_value::<DelegationPolicy>(invalid).is_err());
        }
    }
}
