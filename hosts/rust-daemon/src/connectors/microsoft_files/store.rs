//! Generation-bound Microsoft file tokens, isolated from Outlook credentials.
use super::model::MicrosoftFileMode;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::RwLock};
use zeroize::Zeroizing;

const SERVICE: &str = "animaos.connector.microsoft-files";

#[derive(Clone)]
pub(crate) struct MicrosoftFileGrant {
    pub(crate) mode: MicrosoftFileMode,
    pub(crate) access_token: Zeroizing<String>,
    pub(crate) refresh_token: Zeroizing<String>,
    pub(crate) expires_at_ms: u64,
    pub(crate) scopes: Vec<String>,
}
impl std::fmt::Debug for MicrosoftFileGrant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MicrosoftFileGrant([REDACTED])")
    }
}
impl MicrosoftFileGrant {
    pub(crate) fn validate(&self) -> Result<(), MicrosoftCredentialError> {
        let required = match self.mode {
            MicrosoftFileMode::OneDrive => "Files.Read",
            MicrosoftFileMode::SharePoint => "Sites.Selected",
        };
        if self.access_token.is_empty()
            || self.access_token.len() > 16_384
            || self.refresh_token.is_empty()
            || self.refresh_token.len() > 16_384
            || self.expires_at_ms == 0
            || self.scopes.len() > 32
            || !self.scopes.iter().any(|scope| {
                scope == required || scope == &format!("https://graph.microsoft.com/{required}")
            })
            || self.scopes.iter().any(|scope| {
                scope.is_empty() || scope.len() > 2048 || scope.chars().any(char::is_whitespace)
            })
            || self.scopes.iter().any(|scope| {
                let scope = scope
                    .strip_prefix("https://graph.microsoft.com/")
                    .unwrap_or(scope);
                !matches!(
                    scope,
                    "User.Read" | "offline_access" | "openid" | "profile" | "email"
                ) && scope != required
            })
            || self.access_token.chars().any(char::is_control)
            || self.refresh_token.chars().any(char::is_control)
        {
            return Err(MicrosoftCredentialError);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MicrosoftCredentialError;

#[async_trait]
pub(crate) trait MicrosoftFileCredentialStore: Send + Sync {
    async fn load(
        &self,
        id: &str,
        generation: &str,
    ) -> Result<Option<MicrosoftFileGrant>, MicrosoftCredentialError>;
    async fn put(
        &self,
        id: &str,
        generation: &str,
        grant: MicrosoftFileGrant,
    ) -> Result<(), MicrosoftCredentialError>;
    async fn delete(&self, id: &str, generation: &str) -> Result<(), MicrosoftCredentialError>;
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredGrant {
    version: u8,
    connector_id: String,
    generation: String,
    mode: MicrosoftFileMode,
    access_token: String,
    refresh_token: String,
    expires_at_ms: u64,
    scopes: Vec<String>,
}
impl Drop for StoredGrant {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}
fn account(id: &str, generation: &str) -> Result<String, MicrosoftCredentialError> {
    if id.is_empty()
        || id.len() > 128
        || generation.is_empty()
        || generation.len() > 128
        || id.chars().any(char::is_control)
        || generation.chars().any(char::is_control)
    {
        return Err(MicrosoftCredentialError);
    }
    let mut digest = Sha256::new();
    digest.update((id.len() as u64).to_be_bytes());
    digest.update(id.as_bytes());
    digest.update((generation.len() as u64).to_be_bytes());
    digest.update(generation.as_bytes());
    Ok(hex::encode(digest.finalize()))
}
fn encode(
    id: &str,
    generation: &str,
    grant: &MicrosoftFileGrant,
) -> Result<Zeroizing<String>, MicrosoftCredentialError> {
    grant.validate()?;
    let value = StoredGrant {
        version: 1,
        connector_id: id.into(),
        generation: generation.into(),
        mode: grant.mode,
        access_token: grant.access_token.to_string(),
        refresh_token: grant.refresh_token.to_string(),
        expires_at_ms: grant.expires_at_ms,
        scopes: grant.scopes.clone(),
    };
    serde_json::to_string(&value)
        .map(Zeroizing::new)
        .map_err(|_| MicrosoftCredentialError)
}
fn decode(
    id: &str,
    generation: &str,
    payload: &str,
) -> Result<MicrosoftFileGrant, MicrosoftCredentialError> {
    let mut value: StoredGrant =
        serde_json::from_str(payload).map_err(|_| MicrosoftCredentialError)?;
    if value.version != 1 || value.connector_id != id || value.generation != generation {
        return Err(MicrosoftCredentialError);
    }
    let grant = MicrosoftFileGrant {
        mode: value.mode,
        access_token: Zeroizing::new(std::mem::take(&mut value.access_token)),
        refresh_token: Zeroizing::new(std::mem::take(&mut value.refresh_token)),
        expires_at_ms: value.expires_at_ms,
        scopes: std::mem::take(&mut value.scopes),
    };
    grant.validate()?;
    Ok(grant)
}

#[derive(Default)]
pub(crate) struct InMemoryMicrosoftFileCredentialStore {
    values: RwLock<HashMap<String, Zeroizing<String>>>,
}
#[async_trait]
impl MicrosoftFileCredentialStore for InMemoryMicrosoftFileCredentialStore {
    async fn load(
        &self,
        id: &str,
        generation: &str,
    ) -> Result<Option<MicrosoftFileGrant>, MicrosoftCredentialError> {
        let key = account(id, generation)?;
        self.values
            .read()
            .map_err(|_| MicrosoftCredentialError)?
            .get(&key)
            .map(|payload| decode(id, generation, payload))
            .transpose()
    }
    async fn put(
        &self,
        id: &str,
        generation: &str,
        grant: MicrosoftFileGrant,
    ) -> Result<(), MicrosoftCredentialError> {
        self.values
            .write()
            .map_err(|_| MicrosoftCredentialError)?
            .insert(account(id, generation)?, encode(id, generation, &grant)?);
        Ok(())
    }
    async fn delete(&self, id: &str, generation: &str) -> Result<(), MicrosoftCredentialError> {
        self.values
            .write()
            .map_err(|_| MicrosoftCredentialError)?
            .remove(&account(id, generation)?);
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct OsMicrosoftFileCredentialStore;
#[async_trait]
impl MicrosoftFileCredentialStore for OsMicrosoftFileCredentialStore {
    async fn load(
        &self,
        id: &str,
        generation: &str,
    ) -> Result<Option<MicrosoftFileGrant>, MicrosoftCredentialError> {
        let key = account(id, generation)?;
        let id = id.to_owned();
        let generation = generation.to_owned();
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(&crate::keychain::service_name(SERVICE), &key)
                .map_err(|_| MicrosoftCredentialError)?;
            match entry.get_password() {
                Ok(payload) => decode(&id, &generation, &Zeroizing::new(payload)).map(Some),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err(MicrosoftCredentialError),
            }
        })
        .await
        .map_err(|_| MicrosoftCredentialError)?
    }
    async fn put(
        &self,
        id: &str,
        generation: &str,
        grant: MicrosoftFileGrant,
    ) -> Result<(), MicrosoftCredentialError> {
        let key = account(id, generation)?;
        let payload = encode(id, generation, &grant)?;
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(&crate::keychain::service_name(SERVICE), &key)
                .map_err(|_| MicrosoftCredentialError)?;
            let previous = match entry.get_password() {
                Ok(value) => Some(Zeroizing::new(value)),
                Err(keyring::Error::NoEntry) => None,
                Err(_) => return Err(MicrosoftCredentialError),
            };
            let written = entry.set_password(&payload).is_ok();
            let observed = entry.get_password().ok().map(Zeroizing::new);
            if written && observed.as_ref().map(|value| value.as_str()) == Some(payload.as_str()) {
                return Ok(());
            }
            match previous.as_deref() {
                Some(old) => entry
                    .set_password(old)
                    .map_err(|_| MicrosoftCredentialError)?,
                None => {
                    let _ = entry.delete_credential();
                }
            }
            match (previous, entry.get_password().map(Zeroizing::new)) {
                (Some(old), Ok(observed)) if old.as_str() == observed.as_str() => {
                    Err(MicrosoftCredentialError)
                }
                (None, Err(keyring::Error::NoEntry)) => Err(MicrosoftCredentialError),
                _ => Err(MicrosoftCredentialError),
            }
        })
        .await
        .map_err(|_| MicrosoftCredentialError)?
    }
    async fn delete(&self, id: &str, generation: &str) -> Result<(), MicrosoftCredentialError> {
        let key = account(id, generation)?;
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(&crate::keychain::service_name(SERVICE), &key)
                .map_err(|_| MicrosoftCredentialError)?;
            let _ = entry.delete_credential();
            match entry.get_password() {
                Err(keyring::Error::NoEntry) => Ok(()),
                _ => Err(MicrosoftCredentialError),
            }
        })
        .await
        .map_err(|_| MicrosoftCredentialError)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(mode: MicrosoftFileMode, scopes: &[&str]) -> MicrosoftFileGrant {
        MicrosoftFileGrant {
            mode,
            access_token: Zeroizing::new("private-access".into()),
            refresh_token: Zeroizing::new("private-refresh".into()),
            expires_at_ms: 1,
            scopes: scopes.iter().map(|value| (*value).into()).collect(),
        }
    }

    #[tokio::test]
    async fn credentials_are_scoped_to_the_connector_generation() {
        let store = InMemoryMicrosoftFileCredentialStore::default();
        store
            .put(
                "connector",
                "old",
                grant(MicrosoftFileMode::OneDrive, &["Files.Read"]),
            )
            .await
            .unwrap();
        assert!(store.load("connector", "new").await.unwrap().is_none());
        let loaded = store.load("connector", "old").await.unwrap().unwrap();
        assert!(!format!("{loaded:?}").contains("private-access"));
        store.delete("connector", "old").await.unwrap();
        assert!(store.load("connector", "old").await.unwrap().is_none());
    }

    #[test]
    fn broad_or_cross_service_grants_are_rejected() {
        assert!(grant(MicrosoftFileMode::OneDrive, &["Files.Read"])
            .validate()
            .is_ok());
        assert!(grant(MicrosoftFileMode::SharePoint, &["Sites.Selected"])
            .validate()
            .is_ok());
        assert!(grant(
            MicrosoftFileMode::OneDrive,
            &["Files.Read", "Files.Read.All"]
        )
        .validate()
        .is_err());
        assert!(grant(
            MicrosoftFileMode::SharePoint,
            &["Sites.Selected", "Mail.Read"]
        )
        .validate()
        .is_err());
    }
}
