//! Read-only reuse of Anima model configuration. Never logs, refreshes, or writes credentials.
mod subscription;
use anima_harness::{
    anima_core::ModelAdapter,
    anima_model_adapters::{
        provider_definitions, ProviderAdapterConfig, ProviderCredential, ProviderModelAdapter,
    },
};
use anima_village::{HarnessNpc, NpcEngine, RuntimeInfo};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use zeroize::Zeroizing;

pub struct ResolvedConnection {
    pub engine: Option<Arc<dyn NpcEngine>>,
    pub adapter: Option<Arc<dyn ModelAdapter>>,
    pub runtime: RuntimeInfo,
    pub reason: Option<String>,
}
pub fn resolve() -> ResolvedConnection {
    let selected = selection();
    let (provider, model, profile) = match selected {
        Ok(v) => v,
        Err(reason) => {
            return ResolvedConnection {
                engine: None,
                adapter: None,
                runtime: RuntimeInfo::unavailable(),
                reason: Some(reason),
            }
        }
    };
    let mut runtime = RuntimeInfo {
        mode: "live".into(),
        engine: "anima-harness".into(),
        provider: provider.clone(),
        model: model.clone(),
    };
    let adapter = if std::env::var("VILLAGE_REVIEWED_ONLY").is_ok_and(|v| v == "1" || v == "true") {
        Err("Reviewed-only mode was explicitly requested".into())
    } else if provider == "chatgpt" {
        subscription::adapter()
    } else {
        api_adapter(&provider, &profile)
    };
    match adapter {
        Ok(adapter) => ResolvedConnection {
            adapter: Some(adapter.clone()),
            engine: Some(Arc::new(HarnessNpc {
                adapter,
                provider,
                model,
            })),
            runtime,
            reason: None,
        },
        Err(reason) => {
            runtime.mode = "reviewed".into();
            ResolvedConnection {
                engine: None,
                adapter: None,
                runtime,
                reason: Some(reason),
            }
        }
    }
}
fn home() -> std::result::Result<PathBuf, String> {
    std::env::var_os("ANIMA_HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or("Cannot locate the Anima profile; set VILLAGE_ANIMA_PROFILE".into())
}
fn selection() -> std::result::Result<(String, String, PathBuf), String> {
    let profile = match std::env::var_os("VILLAGE_ANIMA_PROFILE") {
        Some(path) => PathBuf::from(path),
        None => home()?.join(".animaos-swarm"),
    };
    let explicit = (
        std::env::var("VILLAGE_PROVIDER").ok(),
        std::env::var("VILLAGE_MODEL").ok(),
    );
    let (provider,model)=match explicit {
        (Some(provider),Some(model)) if !provider.trim().is_empty()&&!model.trim().is_empty()=>(provider,model),
        (None,None)=>{
            let path=std::env::var_os("ANIMAOS_RS_CONTROL_PLANE_FILE").map(PathBuf::from).unwrap_or(profile.join("data/control-plane.json"));
            primary_from_file(&path,std::env::var("VILLAGE_ANIMA_AGENT").ok().as_deref())?
        },
        _=>return Err("Set both VILLAGE_PROVIDER and VILLAGE_MODEL to explicitly select an existing connection".into()),
    };
    let requested = provider.trim().to_ascii_lowercase();
    let canonical = provider_definitions()
        .iter()
        .find(|d| d.id == requested || d.aliases.contains(&requested.as_str()))
        .map(|d| d.id.to_string())
        .unwrap_or(requested);
    Ok((canonical, model, profile))
}
pub(crate) fn primary_from_file(
    path: &Path,
    agent: Option<&str>,
) -> std::result::Result<(String, String), String> {
    let text = read_private(path, 64 * 1024 * 1024)
        .map_err(|_| "Cannot read the existing Anima model configuration".to_string())?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| "Existing Anima model configuration is malformed".to_string())?;
    let agents = value["agents"]
        .as_array()
        .ok_or("Existing Anima profile has no agents")?;
    let config = agents
        .iter()
        .map(|a| &a["state"]["config"])
        .find(|c| c["name"].as_str() == Some(agent.unwrap_or("Anima")))
        .or_else(|| {
            if agent.is_none() && agents.len() == 1 {
                Some(&agents[0]["state"]["config"])
            } else {
                None
            }
        })
        .ok_or("Select an existing Anima agent with VILLAGE_ANIMA_AGENT")?;
    let provider = config["provider"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("Existing agent has no provider")?;
    let model = config["model"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("Existing agent has no model")?;
    Ok((provider.into(), model.into()))
}
fn read_private(path: &Path, limit: u64) -> std::result::Result<Zeroizing<String>, String> {
    if std::fs::metadata(path)
        .map_err(|_| "Cannot read connection file")?
        .len()
        > limit
    {
        return Err("Connection file exceeds its size limit".into());
    }
    std::fs::read_to_string(path)
        .map(Zeroizing::new)
        .map_err(|_| "Cannot read connection file".into())
}
fn env_file() -> std::result::Result<BTreeMap<String, String>, String> {
    let Some(path) = std::env::var_os("VILLAGE_ENV_FILE") else {
        return Ok(BTreeMap::new());
    };
    let text = read_private(Path::new(&path), 1024 * 1024)?;
    Ok(parse_env(&text))
}
fn parse_env(text: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let Some((name, value)) = line.strip_prefix("export ").unwrap_or(line).split_once('=')
        else {
            continue;
        };
        let value = value.trim();
        let value = if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        values.insert(name.trim().to_owned(), value.to_owned());
    }
    values
}
fn api_adapter(
    provider: &str,
    profile: &Path,
) -> std::result::Result<Arc<dyn ModelAdapter>, String> {
    let definition = provider_definitions()
        .iter()
        .find(|d| d.id == provider)
        .ok_or("The existing model provider is not supported by anima-model-adapters")?;
    let file = env_file()?;
    let lookup = |names: &[&str]| {
        names.iter().find_map(|name| {
            std::env::var(name)
                .ok()
                .or_else(|| file.get(*name).cloned())
                .filter(|v| !v.trim().is_empty())
        })
    };
    let mut key = lookup(definition.api_key_envs);
    if key.is_none() && definition.requires_key {
        let path = std::env::var_os("ANIMAOS_RS_PROVIDER_KEYS_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| profile.join("data/provider-keys.json"));
        if path.exists() {
            let text = read_private(&path, 16 * 1024 * 1024)?;
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|_| "Existing provider key file is malformed")?;
            if value["version"].as_u64() != Some(1) {
                return Err("Existing provider key file has an unsupported version".into());
            }
            let now = subscription::now_ms();
            key = value["providers"][provider]["keys"]
                .as_array()
                .and_then(|keys| {
                    keys.iter().find(|k| {
                        k["restingUntilMs"].as_u64().is_none_or(|t| t <= now)
                            && k["key"].as_str().is_some_and(|s| !s.is_empty())
                    })
                })
                .and_then(|k| k["key"].as_str())
                .map(str::to_owned);
        }
    }
    if definition.requires_key && key.is_none() {
        return Err(format!("No ready existing {provider} credential is available; supply the existing environment or VILLAGE_ENV_FILE"));
    }
    let credential = ProviderCredential {
        api_key: key,
        base_url: lookup(definition.base_url_envs)
            .unwrap_or_else(|| definition.default_base_url.into()),
    };
    ProviderModelAdapter::try_new(ProviderAdapterConfig {
        providers: BTreeMap::from([(provider.into(), credential)]),
        ..Default::default()
    })
    .map(|adapter| Arc::new(adapter) as Arc<dyn ModelAdapter>)
    .map_err(|_| "Cannot initialize existing provider transport".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_existing_primary_model_without_rewriting_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control-plane.json");
        let body = r#"{"agents":[{"state":{"config":{"name":"helper","provider":"anthropic","model":"helper-model"}}},{"state":{"config":{"name":"Anima","provider":"chatgpt","model":"existing-model"}}}]}"#;
        std::fs::write(&path, body).unwrap();
        assert_eq!(
            primary_from_file(&path, None).unwrap(),
            ("chatgpt".into(), "existing-model".into())
        );
        assert_eq!(
            primary_from_file(&path, Some("helper")).unwrap(),
            ("anthropic".into(), "helper-model".into())
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), body);
    }
    #[test]
    fn malformed_single_quote_in_existing_env_does_not_panic() {
        let values =
            parse_env("SINGLE='\nDOUBLE=\"\nGOOD='synthetic-value'\nexport OTHER=\"fixture\"\n");
        assert_eq!(values.get("SINGLE").map(String::as_str), Some("'"));
        assert_eq!(values.get("DOUBLE").map(String::as_str), Some("\""));
        assert_eq!(
            values.get("GOOD").map(String::as_str),
            Some("synthetic-value")
        );
    }
}
