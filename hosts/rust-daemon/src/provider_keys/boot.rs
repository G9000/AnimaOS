//! Opening the pool at daemon boot, and plugging it into the model adapters.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use anima_core::ModelFailure;
use anima_model_adapters::{KeySource as ProviderKeySource, KeysResting};

use super::{KeyOutcome, KeyPool};

pub(crate) const KEYS_FILE_ENV: &str = "ANIMAOS_RS_PROVIDER_KEYS_FILE";
const CONTROL_PLANE_FILE_ENV: &str = "ANIMAOS_RS_CONTROL_PLANE_FILE";
const DEFAULT_FILE_NAME: &str = "provider-keys.json";

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

/// Where the key file lives: the explicit path, else next to the control-plane
/// file, else nowhere (keys added at runtime then last until restart).
pub(super) fn keys_file_path(
    explicit: Option<OsString>,
    control_plane: Option<OsString>,
) -> Option<PathBuf> {
    if let Some(path) = explicit.filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let control_plane = PathBuf::from(control_plane.filter(|path| !path.is_empty())?);
    Some(control_plane.with_file_name(DEFAULT_FILE_NAME))
}

/// Opens the pool from the environment. A key file that cannot be read or
/// parsed is left untouched: the daemon warns and runs with an in-memory pool
/// holding the environment keys only.
pub(crate) fn open_from_env(env: &BTreeMap<String, String>) -> KeyPool {
    open_at(
        keys_file_path(
            std::env::var_os(KEYS_FILE_ENV),
            std::env::var_os(CONTROL_PLANE_FILE_ENV),
        ),
        env,
    )
}

pub(super) fn open_at(path: Option<PathBuf>, env: &BTreeMap<String, String>) -> KeyPool {
    match KeyPool::open(path.clone(), env) {
        Ok(pool) => pool,
        Err(_) => {
            // The parse error is left out of the log: serde may quote a value
            // from the file, and the file holds secrets.
            tracing::warn!(
                file = %path.as_deref().map(|path| path.display().to_string()).unwrap_or_default(),
                "provider key file is unreadable, malformed, or an unsupported version; \
                 leaving it untouched and using environment keys only"
            );
            KeyPool::open(None, env).unwrap_or_else(|error| {
                unreachable!("an in-memory key pool opens without I/O: {error}")
            })
        }
    }
}

impl ProviderKeySource for KeyPool {
    fn pick(&self, provider: &str) -> Option<(String, String)> {
        KeyPool::pick(self, provider, now_ms()).map(|picked| (picked.id, picked.key))
    }

    fn holds_keys(&self, provider: &str) -> bool {
        KeyPool::holds_keys(self, provider)
    }

    fn pick_untried(&self, provider: &str, tried: &[String]) -> Option<(String, String)> {
        KeyPool::pick_untried(self, provider, tried, now_ms()).map(|picked| (picked.id, picked.key))
    }

    fn resting(&self, provider: &str) -> Option<KeysResting> {
        let now = now_ms();
        let (kind, ready_at_ms) = self.resting_state(provider, now)?;
        let ready_at = i64::try_from(ready_at_ms)
            .ok()
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string());
        Some(KeysResting {
            kind,
            ready_in_s: ready_at_ms.saturating_sub(now).div_ceil(1_000).max(1),
            ready_at,
        })
    }

    fn report(&self, provider: &str, key_id: &str, failure: Option<&ModelFailure>) {
        let outcome = match failure {
            None => KeyOutcome::Ok,
            Some(failure) => KeyOutcome::Failed(*failure),
        };
        KeyPool::report(self, provider, key_id, outcome, now_ms());
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use anima_core::{ModelFailure, ModelFailureKind};
    use anima_model_adapters::KeySource as ProviderKeySource;

    use super::*;

    #[test]
    fn the_key_file_defaults_next_to_the_control_plane_file() {
        let os = |text: &str| Some(OsString::from(text));
        assert_eq!(
            keys_file_path(os("/data/keys.json"), os("/cp/control.json")),
            Some(PathBuf::from("/data/keys.json"))
        );
        assert_eq!(
            keys_file_path(None, os("/cp/control.json")),
            Some(PathBuf::from("/cp/provider-keys.json"))
        );
        assert_eq!(
            keys_file_path(os(""), os("control.json")),
            Some(PathBuf::from("provider-keys.json"))
        );
        assert_eq!(keys_file_path(None, None), None);
        assert_eq!(keys_file_path(None, os("")), None);
    }

    #[test]
    fn a_malformed_key_file_is_left_alone_and_the_pool_keeps_env_keys() {
        let dir = std::env::temp_dir().join(format!("anima-keys-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("provider-keys.json");
        std::fs::write(&file, "{ not json").unwrap();
        let env = BTreeMap::from([("openai".to_owned(), "env-key".to_owned())]);

        let pool = open_at(Some(file.clone()), &env);

        assert_eq!(
            ProviderKeySource::pick(&pool, "openai"),
            Some(("env".to_owned(), "env-key".to_owned()))
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
        // Keys added now stay in memory; the bad file is never rewritten.
        pool.add("openai", None, "added".into(), 0)
            .expect("an in-memory pool accepts keys");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_pool_serves_the_adapters_as_a_key_source() {
        let env = BTreeMap::from([("openai".to_owned(), "env-key".to_owned())]);
        let pool = KeyPool::open(None, &env).unwrap();
        pool.add("openai", None, "second".into(), 0).unwrap();
        let source: &dyn ProviderKeySource = &pool;

        let (first, key) = source.pick("openai").expect("a key");
        assert_eq!((first.as_str(), key.as_str()), ("env", "env-key"));
        let failure = ModelFailure {
            kind: ModelFailureKind::RateLimited,
            status: Some(429),
            retry_after_s: None,
        };
        source.report("openai", &first, Some(&failure));

        let (next, key) = source.pick("openai").expect("the next key");
        assert_ne!(next, "env");
        assert_eq!(key, "second");
        assert_eq!(source.pick("anthropic"), None);
        source.report("openai", &next, None);
    }

    #[test]
    fn holds_keys_stays_true_while_every_key_rests() {
        let failure = ModelFailure {
            kind: ModelFailureKind::RateLimited,
            status: Some(429),
            retry_after_s: None,
        };
        // Env key only.
        let env = BTreeMap::from([("openai".to_owned(), "env-key".to_owned())]);
        let pool = KeyPool::open(None, &env).unwrap();
        assert!(ProviderKeySource::holds_keys(&pool, "openai"));
        assert!(!ProviderKeySource::holds_keys(&pool, "anthropic"));
        ProviderKeySource::report(&pool, "openai", "env", Some(&failure));
        assert_eq!(ProviderKeySource::pick(&pool, "openai"), None);
        assert!(ProviderKeySource::holds_keys(&pool, "openai"));

        // File-style keys only (no env key).
        let pool = KeyPool::open(None, &BTreeMap::new()).unwrap();
        assert!(!ProviderKeySource::holds_keys(&pool, "openai"));
        let key = pool.add("openai", None, "added".into(), 0).unwrap();
        ProviderKeySource::report(&pool, "openai", &key.id, Some(&failure));
        assert_eq!(ProviderKeySource::pick(&pool, "openai"), None);
        assert!(ProviderKeySource::holds_keys(&pool, "openai"));
    }

    async fn every_key_resting_error(failure: ModelFailure) -> String {
        use anima_core::{AgentConfig, ModelAdapter, ModelGenerateRequest};
        use anima_model_adapters::{
            ProviderAdapterConfig, ProviderCredential, ProviderModelAdapter,
        };
        let env = BTreeMap::from([("openai".to_owned(), "env-key".to_owned())]);
        let pool = KeyPool::open(None, &env).unwrap();
        ProviderKeySource::report(&pool, "openai", "env", Some(&failure));
        // Nothing listens here: a request would fail as a transport error.
        let adapter = ProviderModelAdapter::new(ProviderAdapterConfig {
            providers: BTreeMap::from([(
                "openai".into(),
                ProviderCredential {
                    api_key: Some("env-key".into()),
                    base_url: "http://127.0.0.1:9/v1".into(),
                },
            )]),
            key_source: Some(std::sync::Arc::new(pool)),
        });
        let config: AgentConfig = serde_json::from_value(serde_json::json!({
            "name": "agent", "model": "gpt-x", "provider": "openai"
        }))
        .unwrap();
        let request = ModelGenerateRequest {
            system: String::new(),
            messages: Vec::new(),
            temperature: None,
            max_tokens: None,
        };
        adapter.generate(&config, &request).await.unwrap_err()
    }

    #[tokio::test]
    async fn a_billing_rested_env_key_reports_402_until_when_without_a_request() {
        let error = every_key_resting_error(ModelFailure {
            kind: ModelFailureKind::Billing,
            status: Some(402),
            retry_after_s: None,
        })
        .await;
        assert!(
            error.starts_with(
                "OpenAI API error (402): every key is resting \u{2014} out of credit until "
            ),
            "{error}"
        );
        assert!(!error.contains("env-key"), "{error}");
        let failure = anima_core::classify_model_error(&error);
        assert_eq!(failure.kind, ModelFailureKind::Billing);
        let hint = failure.retry_after_s.expect("a retry hint");
        assert!((3_590..=3_600).contains(&hint), "{hint}");
    }

    #[tokio::test]
    async fn an_auth_rested_env_key_reports_401() {
        let error = every_key_resting_error(ModelFailure {
            kind: ModelFailureKind::AuthFailed,
            status: Some(401),
            retry_after_s: None,
        })
        .await;
        assert!(
            error.starts_with(
                "OpenAI API error (401): every key is resting \u{2014} key rejected until "
            ),
            "{error}"
        );
        let failure = anima_core::classify_model_error(&error);
        assert_eq!(failure.kind, ModelFailureKind::AuthFailed);
        assert!(failure
            .retry_after_s
            .is_some_and(|s| (290..=300).contains(&s)));
    }
}
