//! Environment handed to the embedded daemon. Precedence: process env > config > packaged default.

use std::collections::BTreeMap;

use crate::config::Config;
use crate::paths::AnimaPaths;

/// The port the server listens on: a usable `ANIMAOS_RS_PORT` in the process
/// environment, else the config's.
pub fn effective_port(cfg: &Config, process_env: &BTreeMap<String, String>) -> u16 {
    process_env
        .get("ANIMAOS_RS_PORT")
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .unwrap_or(cfg.port)
}

/// Returns only the variables the process environment doesn't already set.
pub fn server_env(
    cfg: &Config,
    paths: &AnimaPaths,
    process_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let port = effective_port(cfg, process_env);
    let mut out = BTreeMap::new();
    let mut put = |key: &str, value: String| {
        if !process_env.contains_key(key) {
            out.insert(key.to_string(), value);
        }
    };
    let data = |name: &str| paths.data.join(name).to_string_lossy().into_owned();

    put("ANIMAOS_RS_HOST", "127.0.0.1".to_string());
    put("ANIMAOS_RS_PORT", cfg.port.to_string());
    if let Some(ws) = &cfg.workspace {
        put("ANIMAOS_WORKSPACE_ROOT", ws.to_string_lossy().into_owned());
    }
    put("ANIMAOS_RS_CONTROL_PLANE_FILE", data("control-plane.json"));
    put("ANIMAOS_RS_HISTORY_SQLITE_FILE", data("history.sqlite"));
    put("ANIMAOS_RS_PROVIDER_KEYS_FILE", data("provider-keys.json"));
    // The daemon rejects having both memory file variables set.
    if !process_env.contains_key("ANIMAOS_RS_MEMORY_FILE") {
        put("ANIMAOS_RS_MEMORY_SQLITE_FILE", data("memory.sqlite"));
    }
    put(
        "ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE",
        data("embeddings.sqlite"),
    );
    put(
        "ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR",
        data("embeddings-cache"),
    );
    if cfg!(windows) {
        put("ANIMAOS_RS_CHECKPOINT_ROOT", data("checkpoints"));
    }
    put(
        "ANIMA_ALLOWED_UI_ORIGINS",
        format!("http://127.0.0.1:{0},http://localhost:{0}", port),
    );
    put(
        "ANIMA_PUBLIC_BASE_URL",
        format!("http://127.0.0.1:{}", port),
    );
    put("ANIMAOS_RS_KEYCHAIN_NAMESPACE", "installed".to_string());
    if let Ok(token) = std::fs::read_to_string(&paths.admin_token) {
        let token = token.trim();
        if !token.is_empty() {
            put("ANIMA_LOCAL_ADMIN_TOKEN", token.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, AnimaPaths) {
        let home = tempfile::tempdir().unwrap();
        let paths = AnimaPaths::from_home(home.path());
        paths.ensure().unwrap();
        (home, paths)
    }

    #[test]
    fn server_env_fills_packaged_defaults() {
        let (_home, paths) = setup();
        let env = server_env(&Config::default(), &paths, &BTreeMap::new());
        assert_eq!(env["ANIMAOS_RS_HOST"], "127.0.0.1");
        assert_eq!(env["ANIMAOS_RS_PORT"], "8787");
        assert!(env["ANIMAOS_RS_CONTROL_PLANE_FILE"].ends_with("data/control-plane.json"));
        assert!(env["ANIMAOS_RS_HISTORY_SQLITE_FILE"].ends_with("data/history.sqlite"));
        assert!(env["ANIMAOS_RS_PROVIDER_KEYS_FILE"].ends_with("data/provider-keys.json"));
        assert!(env["ANIMAOS_RS_MEMORY_SQLITE_FILE"].ends_with("data/memory.sqlite"));
        assert!(env["ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE"].ends_with("data/embeddings.sqlite"));
        assert!(env["ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR"].ends_with("data/embeddings-cache"));
        assert_eq!(env["ANIMAOS_RS_KEYCHAIN_NAMESPACE"], "installed");
        assert_eq!(env["ANIMA_PUBLIC_BASE_URL"], "http://127.0.0.1:8787");
        assert_eq!(
            env["ANIMA_ALLOWED_UI_ORIGINS"],
            "http://127.0.0.1:8787,http://localhost:8787"
        );
        assert!(!env.contains_key("ANIMAOS_WORKSPACE_ROOT"));
        assert!(!env.contains_key("ANIMA_LOCAL_ADMIN_TOKEN"));
        assert_eq!(
            env.contains_key("ANIMAOS_RS_CHECKPOINT_ROOT"),
            cfg!(windows)
        );
    }

    #[test]
    fn process_env_wins() {
        let (_home, paths) = setup();
        let process = BTreeMap::from([("ANIMAOS_RS_PORT".to_string(), "9000".to_string())]);
        let env = server_env(&Config::default(), &paths, &process);
        assert!(!env.contains_key("ANIMAOS_RS_PORT"));
        assert!(env.contains_key("ANIMAOS_RS_HOST"));
    }

    #[test]
    fn memory_json_env_suppresses_sqlite_default() {
        let (_home, paths) = setup();
        let process = BTreeMap::from([("ANIMAOS_RS_MEMORY_FILE".to_string(), "x".to_string())]);
        let env = server_env(&Config::default(), &paths, &process);
        assert!(!env.contains_key("ANIMAOS_RS_MEMORY_SQLITE_FILE"));
        assert!(!env.contains_key("ANIMAOS_RS_MEMORY_FILE"));
    }

    #[test]
    fn config_workspace_and_admin_token_are_passed_through() {
        let (_home, paths) = setup();
        std::fs::write(&paths.admin_token, "  secret-token\n").unwrap();
        let cfg = Config {
            workspace: Some("/work/space".into()),
            port: 9100,
            ..Config::default()
        };
        let env = server_env(&cfg, &paths, &BTreeMap::new());
        assert_eq!(env["ANIMAOS_WORKSPACE_ROOT"], "/work/space");
        assert_eq!(env["ANIMA_LOCAL_ADMIN_TOKEN"], "secret-token");
        assert_eq!(env["ANIMAOS_RS_PORT"], "9100");
        assert_eq!(env["ANIMA_PUBLIC_BASE_URL"], "http://127.0.0.1:9100");
    }

    #[test]
    fn origins_follow_the_effective_port() {
        let (_home, paths) = setup();
        let process = BTreeMap::from([("ANIMAOS_RS_PORT".to_string(), "9000".to_string())]);
        let env = server_env(&Config::default(), &paths, &process);
        assert_eq!(env["ANIMA_PUBLIC_BASE_URL"], "http://127.0.0.1:9000");
        assert_eq!(
            env["ANIMA_ALLOWED_UI_ORIGINS"],
            "http://127.0.0.1:9000,http://localhost:9000"
        );
        assert_eq!(effective_port(&Config::default(), &process), 9000);

        // An unusable override falls back to the config's port.
        for bad in ["not-a-port", "0", "70000"] {
            let process = BTreeMap::from([("ANIMAOS_RS_PORT".to_string(), bad.to_string())]);
            let env = server_env(&Config::default(), &paths, &process);
            assert_eq!(
                env["ANIMA_PUBLIC_BASE_URL"], "http://127.0.0.1:8787",
                "{bad}"
            );
            assert_eq!(effective_port(&Config::default(), &process), 8787, "{bad}");
        }
    }
}
