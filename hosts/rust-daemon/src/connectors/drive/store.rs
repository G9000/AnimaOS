//! Independent generation-bound Drive vault; Gmail/Calendar stores remain separate.
use super::model::{DriveError, DriveTokenGrant};
use crate::connectors::gcalendar::store::GoogleOAuthTokens;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use zeroize::Zeroizing;
const SERVICE: &str = "animaos.connector.drive";

#[async_trait]
pub(crate) trait DriveCredentialStore: Send + Sync {
    async fn load(
        &self,
        connector_id: &str,
        generation: &str,
    ) -> Result<Option<DriveTokenGrant>, DriveError>;
    /// The manager owns this mutation to completion after HTTP cancellation.
    async fn put(
        &self,
        connector_id: &str,
        generation: &str,
        grant: DriveTokenGrant,
    ) -> Result<(), DriveError>;
    async fn delete(&self, connector_id: &str, generation: &str) -> Result<(), DriveError>;
}
fn account(id: &str, generation: &str) -> Result<String, DriveError> {
    for value in [id, generation] {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        {
            return Err(DriveError::InvalidInput);
        }
    }
    Ok(format!("connector:{id}:generation:{generation}"))
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredWrite<'a> {
    version: u64,
    connector_id: &'a str,
    generation: &'a str,
    access_token: &'a str,
    refresh_token: &'a str,
    expires_at_ms: u64,
    granted_scopes: &'a [String],
}
struct SecretText(Zeroizing<String>);
impl<'de> Deserialize<'de> for SecretText {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(|s| Self(Zeroizing::new(s)))
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredRead {
    version: u64,
    connector_id: String,
    generation: String,
    access_token: SecretText,
    refresh_token: SecretText,
    expires_at_ms: u64,
    granted_scopes: Vec<String>,
}
fn encode(
    id: &str,
    generation: &str,
    grant: &DriveTokenGrant,
) -> Result<Zeroizing<String>, DriveError> {
    grant.validate()?;
    serde_json::to_string(&StoredWrite {
        version: 1,
        connector_id: id,
        generation,
        access_token: grant.tokens.access_token(),
        refresh_token: grant.tokens.refresh_token(),
        expires_at_ms: grant.tokens.expires_at_ms(),
        granted_scopes: &grant.granted_scopes,
    })
    .map(Zeroizing::new)
    .map_err(|_| DriveError::Credential)
}
fn decode(payload: &str, id: &str, generation: &str) -> Result<DriveTokenGrant, DriveError> {
    let value: StoredRead = serde_json::from_str(payload).map_err(|_| DriveError::Credential)?;
    if value.version != 1 || value.connector_id != id || value.generation != generation {
        return Err(DriveError::Credential);
    }
    let grant = DriveTokenGrant {
        tokens: GoogleOAuthTokens::new(
            value.access_token.0,
            value.refresh_token.0,
            value.expires_at_ms,
        ),
        granted_scopes: value.granted_scopes,
    };
    grant.validate()?;
    Ok(grant)
}
#[derive(Default)]
pub(crate) struct InMemoryDriveCredentialStore {
    values: RwLock<HashMap<String, Zeroizing<String>>>,
}
#[async_trait]
impl DriveCredentialStore for InMemoryDriveCredentialStore {
    async fn load(
        &self,
        id: &str,
        generation: &str,
    ) -> Result<Option<DriveTokenGrant>, DriveError> {
        let account = account(id, generation)?;
        self.values
            .read()
            .map_err(|_| DriveError::Credential)?
            .get(&account)
            .map(|s| decode(s, id, generation))
            .transpose()
    }
    async fn put(
        &self,
        id: &str,
        generation: &str,
        grant: DriveTokenGrant,
    ) -> Result<(), DriveError> {
        let account = account(id, generation)?;
        let payload = encode(id, generation, &grant)?;
        self.values
            .write()
            .map_err(|_| DriveError::Credential)?
            .insert(account, payload);
        Ok(())
    }
    async fn delete(&self, id: &str, generation: &str) -> Result<(), DriveError> {
        let account = account(id, generation)?;
        self.values
            .write()
            .map_err(|_| DriveError::Credential)?
            .remove(&account);
        Ok(())
    }
}
trait Backend: Send + Sync {
    fn load(&self, account: &str) -> Result<Option<Zeroizing<String>>, DriveError>;
    fn put(&self, account: &str, payload: &str) -> Result<(), DriveError>;
    fn delete(&self, account: &str) -> Result<(), DriveError>;
}
struct KeyringBackend;
impl KeyringBackend {
    fn entry(account: &str) -> Result<keyring::Entry, DriveError> {
        keyring::Entry::new(&crate::keychain::service_name(SERVICE), account)
            .map_err(|_| DriveError::Credential)
    }
}
impl Backend for KeyringBackend {
    fn load(&self, account: &str) -> Result<Option<Zeroizing<String>>, DriveError> {
        match Self::entry(account)?.get_password() {
            Ok(s) => Ok(Some(Zeroizing::new(s))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(DriveError::Credential),
        }
    }
    fn put(&self, account: &str, payload: &str) -> Result<(), DriveError> {
        Self::entry(account)?
            .set_password(payload)
            .map_err(|_| DriveError::Credential)
    }
    fn delete(&self, account: &str) -> Result<(), DriveError> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(DriveError::Credential),
        }
    }
}
pub(crate) struct OsKeyringDriveCredentialStore {
    backend: Arc<dyn Backend>,
}
impl OsKeyringDriveCredentialStore {
    pub(crate) fn new() -> Self {
        Self {
            backend: Arc::new(KeyringBackend),
        }
    }
}
impl Default for OsKeyringDriveCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl DriveCredentialStore for OsKeyringDriveCredentialStore {
    async fn load(
        &self,
        id: &str,
        generation: &str,
    ) -> Result<Option<DriveTokenGrant>, DriveError> {
        let account = account(id, generation)?;
        let backend = self.backend.clone();
        let payload = tokio::task::spawn_blocking(move || backend.load(&account))
            .await
            .map_err(|_| DriveError::Credential)??;
        payload.map(|s| decode(&s, id, generation)).transpose()
    }
    async fn put(
        &self,
        id: &str,
        generation: &str,
        grant: DriveTokenGrant,
    ) -> Result<(), DriveError> {
        let account = account(id, generation)?;
        let payload = encode(id, generation, &grant)?;
        let backend = self.backend.clone();
        tokio::task::spawn_blocking(move || {
            let previous = backend.load(&account)?;
            if backend.put(&account, &payload).is_ok() {
                let observed = backend.load(&account)?;
                return if observed.as_ref().map(|s| s.as_str()) == Some(payload.as_str()) {
                    Ok(())
                } else {
                    Err(DriveError::Credential)
                };
            }
            match previous.as_ref() {
                Some(s) => backend.put(&account, s)?,
                None => backend.delete(&account)?,
            };
            let observed = backend.load(&account)?;
            if observed != previous {
                return Err(DriveError::Credential);
            }
            Err(DriveError::Credential)
        })
        .await
        .map_err(|_| DriveError::Credential)?
    }
    async fn delete(&self, id: &str, generation: &str) -> Result<(), DriveError> {
        let account = account(id, generation)?;
        let backend = self.backend.clone();
        tokio::task::spawn_blocking(move || {
            let _result = backend.delete(&account);
            if backend.load(&account)?.is_none() {
                Ok(())
            } else {
                Err(DriveError::Credential)
            }
        })
        .await
        .map_err(|_| DriveError::Credential)?
    }
}
#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
