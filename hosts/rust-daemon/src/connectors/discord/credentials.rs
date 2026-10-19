//! Independent, generation-bound native Discord vault. Never shares Telegram accounts.
use super::model::DiscordSnowflake;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};
use zeroize::Zeroizing;

const SERVICE: &str = "animaos.connector.discord";
pub(crate) struct DiscordBotToken(Zeroizing<String>);
impl DiscordBotToken {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, DiscordCredentialError> {
        let value = Zeroizing::new(value.into());
        if value.is_empty()
            || value.len() > 256
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(DiscordCredentialError::InvalidInput);
        }
        Ok(Self(value))
    }
    pub(crate) fn expose(&self) -> &str {
        self.0.as_str()
    }
}
impl Clone for DiscordBotToken {
    fn clone(&self) -> Self {
        Self(Zeroizing::new(self.0.to_string()))
    }
}
impl PartialEq for DiscordBotToken {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}
impl Eq for DiscordBotToken {}
impl fmt::Debug for DiscordBotToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DiscordBotToken([REDACTED])")
    }
}
impl fmt::Display for DiscordBotToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DiscordCredentialError {
    InvalidInput,
    Unavailable,
    Uncertain,
    InvalidPayload,
}
impl fmt::Display for DiscordCredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "invalid Discord credential binding",
            Self::Unavailable => "Discord credential vault unavailable",
            Self::Uncertain => "Discord credential vault outcome uncertain",
            Self::InvalidPayload => "invalid Discord credential payload",
        })
    }
}
impl std::error::Error for DiscordCredentialError {}
#[async_trait]
pub(crate) trait DiscordCredentialStore: Send + Sync {
    async fn load(
        &self,
        connector_id: &str,
        generation: u64,
        bot_id: &str,
    ) -> Result<Option<DiscordBotToken>, DiscordCredentialError>;
    /// The manager owns every mutation to completion, independently of HTTP cancellation.
    async fn put(
        &self,
        connector_id: &str,
        generation: u64,
        bot_id: &str,
        token: &DiscordBotToken,
    ) -> Result<(), DiscordCredentialError>;
    async fn delete(
        &self,
        connector_id: &str,
        generation: u64,
    ) -> Result<(), DiscordCredentialError>;
}
fn account(connector_id: &str, generation: u64) -> Result<String, DiscordCredentialError> {
    if generation == 0
        || connector_id.is_empty()
        || connector_id.len() > 128
        || !connector_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(DiscordCredentialError::InvalidInput);
    }
    Ok(format!("connector:{connector_id}:generation:{generation}"))
}
fn binding(
    connector_id: &str,
    generation: u64,
    bot_id: &str,
) -> Result<String, DiscordCredentialError> {
    if DiscordSnowflake::parse(bot_id).is_none() {
        return Err(DiscordCredentialError::InvalidInput);
    }
    account(connector_id, generation)
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Payload<'a> {
    version: u64,
    connector_id: &'a str,
    generation: u64,
    bot_id: &'a str,
    #[serde(borrow)]
    token: &'a str,
}
fn decode(
    payload: &str,
    id: &str,
    generation: u64,
    bot: &str,
) -> Result<DiscordBotToken, DiscordCredentialError> {
    let value: Payload<'_> =
        serde_json::from_str(payload).map_err(|_| DiscordCredentialError::InvalidPayload)?;
    if value.version != 1
        || value.connector_id != id
        || value.generation != generation
        || value.bot_id != bot
    {
        return Err(DiscordCredentialError::InvalidPayload);
    }
    DiscordBotToken::parse(value.token).map_err(|_| DiscordCredentialError::InvalidPayload)
}
#[derive(Default)]
pub(crate) struct InMemoryDiscordCredentialStore {
    values: RwLock<HashMap<String, Zeroizing<String>>>,
}
#[async_trait]
impl DiscordCredentialStore for InMemoryDiscordCredentialStore {
    async fn load(
        &self,
        id: &str,
        generation: u64,
        bot: &str,
    ) -> Result<Option<DiscordBotToken>, DiscordCredentialError> {
        let key = binding(id, generation, bot)?;
        let values = self
            .values
            .read()
            .map_err(|_| DiscordCredentialError::Unavailable)?;
        values
            .get(&key)
            .map(|v| decode(v, id, generation, bot))
            .transpose()
    }
    async fn put(
        &self,
        id: &str,
        generation: u64,
        bot: &str,
        token: &DiscordBotToken,
    ) -> Result<(), DiscordCredentialError> {
        let key = binding(id, generation, bot)?;
        let payload = Zeroizing::new(
            serde_json::to_string(&Payload {
                version: 1,
                connector_id: id,
                generation,
                bot_id: bot,
                token: token.expose(),
            })
            .map_err(|_| DiscordCredentialError::InvalidPayload)?,
        );
        self.values
            .write()
            .map_err(|_| DiscordCredentialError::Unavailable)?
            .insert(key, payload);
        Ok(())
    }
    async fn delete(&self, id: &str, generation: u64) -> Result<(), DiscordCredentialError> {
        let key = account(id, generation)?;
        self.values
            .write()
            .map_err(|_| DiscordCredentialError::Unavailable)?
            .remove(&key);
        Ok(())
    }
}

trait Backend: Send + Sync {
    fn load(&self, account: &str) -> Result<Option<Zeroizing<String>>, DiscordCredentialError>;
    fn put(&self, account: &str, payload: &str) -> Result<(), DiscordCredentialError>;
    fn delete(&self, account: &str) -> Result<(), DiscordCredentialError>;
}
struct KeyringBackend;
impl KeyringBackend {
    fn entry(account: &str) -> Result<keyring::Entry, DiscordCredentialError> {
        keyring::Entry::new(&crate::keychain::service_name(SERVICE), account)
            .map_err(|_| DiscordCredentialError::Unavailable)
    }
}
impl Backend for KeyringBackend {
    fn load(&self, account: &str) -> Result<Option<Zeroizing<String>>, DiscordCredentialError> {
        match Self::entry(account)?.get_password() {
            Ok(v) => Ok(Some(Zeroizing::new(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(DiscordCredentialError::Unavailable),
        }
    }
    fn put(&self, account: &str, payload: &str) -> Result<(), DiscordCredentialError> {
        Self::entry(account)?
            .set_password(payload)
            .map_err(|_| DiscordCredentialError::Unavailable)
    }
    fn delete(&self, account: &str) -> Result<(), DiscordCredentialError> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(DiscordCredentialError::Unavailable),
        }
    }
}
pub(crate) struct OsKeyringDiscordCredentialStore {
    backend: Arc<dyn Backend>,
}
impl OsKeyringDiscordCredentialStore {
    pub(crate) fn new() -> Self {
        Self {
            backend: Arc::new(KeyringBackend),
        }
    }
}
impl Default for OsKeyringDiscordCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl DiscordCredentialStore for OsKeyringDiscordCredentialStore {
    async fn load(
        &self,
        id: &str,
        generation: u64,
        bot: &str,
    ) -> Result<Option<DiscordBotToken>, DiscordCredentialError> {
        let key = binding(id, generation, bot)?;
        let backend = self.backend.clone();
        let payload = tokio::task::spawn_blocking(move || backend.load(&key))
            .await
            .map_err(|_| DiscordCredentialError::Unavailable)??;
        payload
            .map(|value| decode(&value, id, generation, bot))
            .transpose()
    }
    async fn put(
        &self,
        id: &str,
        generation: u64,
        bot: &str,
        token: &DiscordBotToken,
    ) -> Result<(), DiscordCredentialError> {
        let key = binding(id, generation, bot)?;
        let backend = self.backend.clone();
        let payload = Zeroizing::new(
            serde_json::to_string(&Payload {
                version: 1,
                connector_id: id,
                generation,
                bot_id: bot,
                token: token.expose(),
            })
            .map_err(|_| DiscordCredentialError::InvalidPayload)?,
        );
        tokio::task::spawn_blocking(move || {
            let previous = backend.load(&key)?;
            if backend.put(&key, &payload).is_ok() {
                let observed = backend
                    .load(&key)
                    .map_err(|_| DiscordCredentialError::Uncertain)?;
                return if observed.as_ref().map(|v| v.as_str()) == Some(payload.as_str()) {
                    Ok(())
                } else {
                    Err(DiscordCredentialError::Uncertain)
                };
            }
            let rollback = match previous.as_ref() {
                Some(v) => backend.put(&key, v),
                None => backend.delete(&key),
            };
            if rollback.is_err() {
                return Err(DiscordCredentialError::Uncertain);
            }
            let observed = backend
                .load(&key)
                .map_err(|_| DiscordCredentialError::Uncertain)?;
            if observed == previous {
                Err(DiscordCredentialError::Unavailable)
            } else {
                Err(DiscordCredentialError::Uncertain)
            }
        })
        .await
        .map_err(|_| DiscordCredentialError::Uncertain)?
    }
    async fn delete(&self, id: &str, generation: u64) -> Result<(), DiscordCredentialError> {
        let key = account(id, generation)?;
        let backend = self.backend.clone();
        tokio::task::spawn_blocking(move || {
            let previous = backend.load(&key)?;
            let result = backend.delete(&key);
            let observed = backend
                .load(&key)
                .map_err(|_| DiscordCredentialError::Uncertain)?;
            if observed.is_none() {
                Ok(())
            } else if result.is_err() && observed == previous {
                Err(DiscordCredentialError::Unavailable)
            } else {
                Err(DiscordCredentialError::Uncertain)
            }
        })
        .await
        .map_err(|_| DiscordCredentialError::Uncertain)?
    }
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod tests;
