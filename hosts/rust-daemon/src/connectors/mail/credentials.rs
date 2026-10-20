//! Generic IMAP/SMTP passwords have their own vault namespace; snapshots never hold them.
use super::super::credentials::{account_for, CredentialStoreError};
use anima_service_adapters::email::EmailSecrets;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::RwLock};
use zeroize::{Zeroize, Zeroizing};

const SERVICE: &str = "animaos.connector.email";

#[async_trait]
pub(crate) trait EmailCredentialStore: Send + Sync {
    async fn load(&self, id: &str) -> Result<Option<EmailSecrets>, CredentialStoreError>;
    async fn put(&self, id: &str, secrets: EmailSecrets) -> Result<(), CredentialStoreError>;
    async fn delete(&self, id: &str) -> Result<(), CredentialStoreError>;
}

#[derive(Default)]
#[allow(dead_code)]
pub(crate) struct InMemoryEmailCredentialStore {
    values: RwLock<HashMap<String, EmailSecrets>>,
}
#[async_trait]
impl EmailCredentialStore for InMemoryEmailCredentialStore {
    async fn load(&self, id: &str) -> Result<Option<EmailSecrets>, CredentialStoreError> {
        Ok(self
            .values
            .read()
            .map_err(|_| CredentialStoreError::BackendUnavailable)?
            .get(&account_for(id)?)
            .cloned())
    }
    async fn put(&self, id: &str, secrets: EmailSecrets) -> Result<(), CredentialStoreError> {
        self.values
            .write()
            .map_err(|_| CredentialStoreError::BackendUnavailable)?
            .insert(account_for(id)?, secrets);
        Ok(())
    }
    async fn delete(&self, id: &str) -> Result<(), CredentialStoreError> {
        self.values
            .write()
            .map_err(|_| CredentialStoreError::BackendUnavailable)?
            .remove(&account_for(id)?);
        Ok(())
    }
}

pub(crate) struct OsKeyringEmailCredentialStore;
impl OsKeyringEmailCredentialStore {
    pub(crate) fn new() -> Self {
        Self
    }
}
#[derive(Serialize)]
struct Stored<'a> {
    version: u8,
    imap_password: &'a str,
    smtp_password: &'a str,
}
#[derive(Deserialize)]
struct Loaded {
    version: u8,
    imap_password: String,
    smtp_password: String,
}
impl Drop for Loaded {
    fn drop(&mut self) {
        self.imap_password.zeroize();
        self.smtp_password.zeroize();
    }
}
fn entry(id: &str) -> Result<keyring::Entry, CredentialStoreError> {
    keyring::Entry::new(&crate::keychain::service_name(SERVICE), &account_for(id)?)
        .map_err(|_| CredentialStoreError::BackendUnavailable)
}
#[async_trait]
impl EmailCredentialStore for OsKeyringEmailCredentialStore {
    async fn load(&self, id: &str) -> Result<Option<EmailSecrets>, CredentialStoreError> {
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || {
            let payload = match entry(&id)?.get_password() {
                Ok(v) => Zeroizing::new(v),
                Err(keyring::Error::NoEntry) => return Ok(None),
                Err(_) => return Err(CredentialStoreError::BackendUnavailable),
            };
            let mut decoded: Loaded = serde_json::from_str(payload.as_str())
                .map_err(|_| CredentialStoreError::InvalidPayload)?;
            if decoded.version != 1 {
                return Err(CredentialStoreError::UnsupportedPayloadVersion);
            }
            Ok(Some(EmailSecrets::new(
                std::mem::take(&mut decoded.imap_password),
                std::mem::take(&mut decoded.smtp_password),
            )))
        })
        .await
        .map_err(|_| CredentialStoreError::OperationCancelled)?
    }
    async fn put(&self, id: &str, secrets: EmailSecrets) -> Result<(), CredentialStoreError> {
        let id = id.to_owned();
        let payload = Zeroizing::new(
            serde_json::to_string(&Stored {
                version: 1,
                imap_password: secrets.imap_password(),
                smtp_password: secrets.smtp_password(),
            })
            .map_err(|_| CredentialStoreError::InvalidPayload)?,
        );
        tokio::task::spawn_blocking(move || {
            let entry = entry(&id)?;
            entry
                .set_password(payload.as_str())
                .map_err(|_| CredentialStoreError::BackendUnavailable)?;
            let observed = Zeroizing::new(
                entry
                    .get_password()
                    .map_err(|_| CredentialStoreError::CredentialStateUncertain)?,
            );
            if *observed == *payload {
                Ok(())
            } else {
                Err(CredentialStoreError::CredentialStateUncertain)
            }
        })
        .await
        .map_err(|_| CredentialStoreError::CredentialStateUncertain)?
    }
    async fn delete(&self, id: &str) -> Result<(), CredentialStoreError> {
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || {
            let entry = entry(&id)?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(_) => return Err(CredentialStoreError::BackendUnavailable),
            }
            match entry.get_password() {
                Err(keyring::Error::NoEntry) => Ok(()),
                Ok(value) => {
                    let _value = Zeroizing::new(value);
                    Err(CredentialStoreError::CredentialStateUncertain)
                }
                Err(_) => Err(CredentialStoreError::CredentialStateUncertain),
            }
        })
        .await
        .map_err(|_| CredentialStoreError::CredentialStateUncertain)?
    }
}
