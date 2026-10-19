//! Static owner-entered bearer credentials. Storage follows the connector vault
//! pattern, with a separate service and fresh, endpoint-bound MCP slots.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};
use zeroize::Zeroizing;

const SERVICE: &str = "animaos.mcp.bearer";
pub(crate) const MAX_BEARER_BYTES: usize = 4096;

#[derive(Clone)]
pub(crate) struct McpBearerToken(Zeroizing<String>);
impl McpBearerToken {
    pub(crate) fn parse(value: String) -> Result<Self, McpCredentialError> {
        let value = Zeroizing::new(value);
        if value.is_empty()
            || value.len() > MAX_BEARER_BYTES
            || !value.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(McpCredentialError::Invalid);
        }
        Ok(Self(value))
    }
    pub(crate) fn expose(&self) -> &str {
        self.0.as_str()
    }
}
impl fmt::Debug for McpBearerToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("McpBearerToken([REDACTED])")
    }
}
impl<'de> Deserialize<'de> for McpBearerToken {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?)
            .map_err(|_| serde::de::Error::custom("invalid MCP credential"))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum McpCredentialError {
    Invalid,
    Unavailable,
    Uncertain,
}
impl fmt::Display for McpCredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MCP credential unavailable or invalid")
    }
}
impl std::error::Error for McpCredentialError {}

pub(crate) fn valid_reference(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .is_ok_and(|id| id.get_version_num() == 4 && id.to_string() == value)
}
fn account(connection: &str, reference: &str) -> Result<String, McpCredentialError> {
    if !valid_reference(connection) || !valid_reference(reference) {
        return Err(McpCredentialError::Invalid);
    }
    Ok(format!("connection:{connection}:credential:{reference}"))
}
fn endpoint_valid(endpoint: &str) -> bool {
    endpoint.starts_with("https://")
        && super::normalized_endpoint(endpoint).is_ok_and(|value| value == endpoint)
}

/// Mutations are awaited by McpManager's owned task, including blocking OS work.
#[async_trait]
pub(crate) trait McpCredentialStore: Send + Sync {
    async fn load(
        &self,
        connection: &str,
        endpoint: &str,
        reference: &str,
    ) -> Result<Option<McpBearerToken>, McpCredentialError>;
    async fn put(
        &self,
        connection: &str,
        endpoint: &str,
        reference: &str,
        token: McpBearerToken,
    ) -> Result<(), McpCredentialError>;
    async fn delete(&self, connection: &str, reference: &str) -> Result<(), McpCredentialError>;
}

#[derive(Default)]
pub(crate) struct InMemoryMcpCredentialStore {
    values: RwLock<HashMap<String, (String, McpBearerToken)>>,
}
#[async_trait]
impl McpCredentialStore for InMemoryMcpCredentialStore {
    async fn load(
        &self,
        connection: &str,
        endpoint: &str,
        reference: &str,
    ) -> Result<Option<McpBearerToken>, McpCredentialError> {
        let account = account(connection, reference)?;
        if !endpoint_valid(endpoint) {
            return Err(McpCredentialError::Invalid);
        }
        self.values
            .read()
            .map_err(|_| McpCredentialError::Unavailable)?
            .get(&account)
            .map(|(bound, token)| {
                if bound == endpoint {
                    Ok(token.clone())
                } else {
                    Err(McpCredentialError::Invalid)
                }
            })
            .transpose()
    }
    async fn put(
        &self,
        connection: &str,
        endpoint: &str,
        reference: &str,
        token: McpBearerToken,
    ) -> Result<(), McpCredentialError> {
        let account = account(connection, reference)?;
        if !endpoint_valid(endpoint) {
            return Err(McpCredentialError::Invalid);
        }
        let mut values = self
            .values
            .write()
            .map_err(|_| McpCredentialError::Unavailable)?;
        if values.contains_key(&account) {
            return Err(McpCredentialError::Invalid);
        }
        values.insert(account, (endpoint.into(), token));
        Ok(())
    }
    async fn delete(&self, connection: &str, reference: &str) -> Result<(), McpCredentialError> {
        self.values
            .write()
            .map_err(|_| McpCredentialError::Unavailable)?
            .remove(&account(connection, reference)?);
        Ok(())
    }
}

trait VaultBackend: Send + Sync {
    fn load(&self, account: &str) -> Result<Option<Zeroizing<String>>, McpCredentialError>;
    fn put(&self, account: &str, value: &str) -> Result<(), McpCredentialError>;
    fn delete(&self, account: &str) -> Result<(), McpCredentialError>;
}
struct OsBackend;
impl OsBackend {
    fn entry(account: &str) -> Result<keyring::Entry, McpCredentialError> {
        keyring::Entry::new(&crate::keychain::service_name(SERVICE), account)
            .map_err(|_| McpCredentialError::Unavailable)
    }
}
impl VaultBackend for OsBackend {
    fn load(&self, account: &str) -> Result<Option<Zeroizing<String>>, McpCredentialError> {
        match Self::entry(account)?.get_password() {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(McpCredentialError::Unavailable),
        }
    }
    fn put(&self, account: &str, value: &str) -> Result<(), McpCredentialError> {
        Self::entry(account)?
            .set_password(value)
            .map_err(|_| McpCredentialError::Unavailable)
    }
    fn delete(&self, account: &str) -> Result<(), McpCredentialError> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(McpCredentialError::Unavailable),
        }
    }
}
pub(crate) struct OsKeyringMcpCredentialStore {
    backend: Arc<dyn VaultBackend>,
}
impl OsKeyringMcpCredentialStore {
    pub(crate) fn new() -> Self {
        Self {
            backend: Arc::new(OsBackend),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload<'a> {
    version: u8,
    #[serde(borrow)]
    connection: &'a str,
    #[serde(borrow)]
    endpoint: &'a str,
    #[serde(borrow)]
    reference: &'a str,
    #[serde(borrow)]
    token: &'a str,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadedPayload {
    version: u8,
    connection: String,
    endpoint: String,
    reference: String,
    token: McpBearerToken,
}
#[async_trait]
impl McpCredentialStore for OsKeyringMcpCredentialStore {
    async fn load(
        &self,
        connection: &str,
        endpoint: &str,
        reference: &str,
    ) -> Result<Option<McpBearerToken>, McpCredentialError> {
        let account = account(connection, reference)?;
        if !endpoint_valid(endpoint) {
            return Err(McpCredentialError::Invalid);
        }
        let backend = self.backend.clone();
        let value = tokio::task::spawn_blocking(move || backend.load(&account))
            .await
            .map_err(|_| McpCredentialError::Unavailable)??;
        let Some(value) = value else {
            return Ok(None);
        };
        let payload: LoadedPayload =
            serde_json::from_str(&value).map_err(|_| McpCredentialError::Invalid)?;
        if payload.version != 1
            || payload.connection != connection
            || payload.endpoint != endpoint
            || payload.reference != reference
        {
            return Err(McpCredentialError::Invalid);
        }
        Ok(Some(payload.token))
    }
    async fn put(
        &self,
        connection: &str,
        endpoint: &str,
        reference: &str,
        token: McpBearerToken,
    ) -> Result<(), McpCredentialError> {
        let account = account(connection, reference)?;
        if !endpoint_valid(endpoint) {
            return Err(McpCredentialError::Invalid);
        }
        let value = Zeroizing::new(
            serde_json::to_string(&Payload {
                version: 1,
                connection,
                endpoint,
                reference,
                token: token.expose(),
            })
            .map_err(|_| McpCredentialError::Invalid)?,
        );
        let backend = self.backend.clone();
        tokio::task::spawn_blocking(move || {
            if backend.load(&account)?.is_some() {
                return Err(McpCredentialError::Invalid);
            }
            let written = backend.put(&account, &value);
            let observed = backend
                .load(&account)
                .map_err(|_| McpCredentialError::Uncertain)?;
            if written.is_ok() && observed.as_ref().map(|v| v.as_str()) == Some(value.as_str()) {
                return Ok(());
            }
            // This slot was fresh: compensate only this new material, never another credential.
            if backend.delete(&account).is_err()
                || backend
                    .load(&account)
                    .map_err(|_| McpCredentialError::Uncertain)?
                    .is_some()
            {
                return Err(McpCredentialError::Uncertain);
            }
            Err(McpCredentialError::Unavailable)
        })
        .await
        .map_err(|_| McpCredentialError::Uncertain)?
    }
    async fn delete(&self, connection: &str, reference: &str) -> Result<(), McpCredentialError> {
        let account = account(connection, reference)?;
        let backend = self.backend.clone();
        tokio::task::spawn_blocking(move || {
            let previous = backend.load(&account)?;
            let result = backend.delete(&account);
            let observed = backend
                .load(&account)
                .map_err(|_| McpCredentialError::Uncertain)?;
            if observed.is_none() {
                return Ok(());
            }
            if result.is_err()
                && observed.as_ref().map(|v| v.as_str()) == previous.as_ref().map(|v| v.as_str())
            {
                Err(McpCredentialError::Unavailable)
            } else {
                Err(McpCredentialError::Uncertain)
            }
        })
        .await
        .map_err(|_| McpCredentialError::Uncertain)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };
    const TOKEN: &str = "m65-synthetic-vault-secret";
    const ENDPOINT: &str = "https://m65.invalid/mcp";
    fn token() -> McpBearerToken {
        McpBearerToken::parse(TOKEN.into()).unwrap()
    }
    #[test]
    fn m65_bearer_bounds_and_debug_do_not_expose_input() {
        for value in [
            "".into(),
            " ".into(),
            "bad\r\nheader".into(),
            "\0".into(),
            "é".into(),
            "x".repeat(MAX_BEARER_BYTES + 1),
        ] {
            let error = McpBearerToken::parse(value).unwrap_err();
            assert_eq!(error, McpCredentialError::Invalid);
            assert!(!format!("{error:?} {error}").contains(TOKEN));
        }
        assert_eq!(
            McpBearerToken::parse("x".repeat(MAX_BEARER_BYTES))
                .unwrap()
                .expose()
                .len(),
            MAX_BEARER_BYTES
        );
        assert!(!format!("{:?}", token()).contains(TOKEN));
    }
    #[tokio::test]
    async fn m65_memory_slots_are_fresh_endpoint_and_connection_bound() {
        let store = InMemoryMcpCredentialStore::default();
        let id = uuid::Uuid::new_v4().to_string();
        let slot = uuid::Uuid::new_v4().to_string();
        assert!(store.load(&id, ENDPOINT, &slot).await.unwrap().is_none());
        store.put(&id, ENDPOINT, &slot, token()).await.unwrap();
        assert_eq!(
            store
                .load(&id, ENDPOINT, &slot)
                .await
                .unwrap()
                .unwrap()
                .expose(),
            TOKEN
        );
        assert_eq!(
            store.put(&id, ENDPOINT, &slot, token()).await.unwrap_err(),
            McpCredentialError::Invalid
        );
        assert!(store
            .load(&id, "https://m65.invalid/other", &slot)
            .await
            .is_err());
        assert!(store
            .load(&uuid::Uuid::new_v4().to_string(), ENDPOINT, &slot)
            .await
            .unwrap()
            .is_none());
        assert!(store
            .put(
                &id,
                "http://127.0.0.1/mcp",
                &uuid::Uuid::new_v4().to_string(),
                token()
            )
            .await
            .is_err());
        assert!(store
            .put("../foreign", ENDPOINT, &slot, token())
            .await
            .is_err());
        store.delete(&id, &slot).await.unwrap();
        store.delete(&id, &slot).await.unwrap();
        assert!(store.load(&id, ENDPOINT, &slot).await.unwrap().is_none());
    }
    #[derive(Default)]
    struct FakeVault {
        values: Mutex<HashMap<String, Zeroizing<String>>>,
        mode: AtomicUsize,
        writes: AtomicUsize,
    }
    impl VaultBackend for FakeVault {
        fn load(&self, account: &str) -> Result<Option<Zeroizing<String>>, McpCredentialError> {
            if self.mode.load(Ordering::SeqCst) == 3 && self.writes.load(Ordering::SeqCst) > 0 {
                return Err(McpCredentialError::Unavailable);
            }
            Ok(self.values.lock().unwrap().get(account).cloned())
        }
        fn put(&self, account: &str, value: &str) -> Result<(), McpCredentialError> {
            let mode = self.mode.load(Ordering::SeqCst);
            if mode == 1 {
                return Err(McpCredentialError::Unavailable);
            }
            self.values
                .lock()
                .unwrap()
                .insert(account.into(), Zeroizing::new(value.into()));
            self.writes.fetch_add(1, Ordering::SeqCst);
            if mode == 2 {
                Err(McpCredentialError::Unavailable)
            } else {
                Ok(())
            }
        }
        fn delete(&self, account: &str) -> Result<(), McpCredentialError> {
            match self.mode.load(Ordering::SeqCst) {
                4 => Err(McpCredentialError::Unavailable),
                6 => Ok(()),
                mode => {
                    self.values.lock().unwrap().remove(account);
                    if mode == 5 {
                        Err(McpCredentialError::Unavailable)
                    } else {
                        Ok(())
                    }
                }
            }
        }
    }
    #[tokio::test]
    async fn m65_os_adapter_fake_backend_verifies_binding_compensation_and_uncertainty() {
        assert_eq!(SERVICE, "animaos.mcp.bearer");
        for mode in 0..=3 {
            let backend = Arc::new(FakeVault::default());
            backend.mode.store(mode, Ordering::SeqCst);
            let store = OsKeyringMcpCredentialStore {
                backend: backend.clone(),
            };
            let id = uuid::Uuid::new_v4().to_string();
            let slot = uuid::Uuid::new_v4().to_string();
            let result = store.put(&id, ENDPOINT, &slot, token()).await;
            match mode {
                0 => {
                    result.unwrap();
                    assert_eq!(
                        store
                            .load(&id, ENDPOINT, &slot)
                            .await
                            .unwrap()
                            .unwrap()
                            .expose(),
                        TOKEN
                    );
                    assert!(store
                        .load(&id, "https://m65.invalid/changed", &slot)
                        .await
                        .is_err());
                    assert!(store.put(&id, ENDPOINT, &slot, token()).await.is_err());
                }
                1 | 2 => {
                    assert_eq!(result.unwrap_err(), McpCredentialError::Unavailable);
                    assert!(backend.values.lock().unwrap().is_empty());
                }
                3 => assert_eq!(result.unwrap_err(), McpCredentialError::Uncertain),
                _ => unreachable!(),
            }
        }
        for mode in [0, 4, 5, 6] {
            let backend = Arc::new(FakeVault::default());
            let store = OsKeyringMcpCredentialStore {
                backend: backend.clone(),
            };
            let id = uuid::Uuid::new_v4().to_string();
            let slot = uuid::Uuid::new_v4().to_string();
            store.put(&id, ENDPOINT, &slot, token()).await.unwrap();
            backend.mode.store(mode, Ordering::SeqCst);
            let result = store.delete(&id, &slot).await;
            match mode {
                0 | 5 => {
                    result.unwrap();
                    assert!(store.load(&id, ENDPOINT, &slot).await.unwrap().is_none());
                }
                4 => assert_eq!(result.unwrap_err(), McpCredentialError::Unavailable),
                6 => assert_eq!(result.unwrap_err(), McpCredentialError::Uncertain),
                _ => unreachable!(),
            }
        }
    }
}
