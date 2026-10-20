//! Host-independent, bounded IMAP/SMTP email transport.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroizing;

pub const MAX_INBOX_MESSAGES: usize = 25;
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
pub const MAX_TEXT_BYTES: usize = 32 * 1024;
pub const MAX_OUTGOING_TEXT_BYTES: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmailSecurity {
    Tls,
    Starttls,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailEndpoint {
    pub host: String,
    pub port: u16,
    pub security: EmailSecurity,
    pub username: String,
}

impl EmailEndpoint {
    pub fn new(
        host: String,
        port: u16,
        security: EmailSecurity,
        username: String,
    ) -> Result<Self, EmailError> {
        let endpoint = Self {
            host,
            port,
            security,
            username,
        };
        endpoint.validate()?;
        Ok(endpoint)
    }
    pub fn validate(&self) -> Result<(), EmailError> {
        let host_valid = self.host.parse::<std::net::IpAddr>().is_ok()
            || (!self.host.is_empty()
                && self.host.len() <= 253
                && self.host.is_ascii()
                && self.host.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && label
                            .as_bytes()
                            .first()
                            .is_some_and(u8::is_ascii_alphanumeric)
                        && label
                            .as_bytes()
                            .last()
                            .is_some_and(u8::is_ascii_alphanumeric)
                        && label
                            .bytes()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
                }));
        if !host_valid
            || self.port == 0
            || self.username.is_empty()
            || self.username.len() > 512
            || self.username.trim() != self.username
            || self.username.chars().any(char::is_control)
        {
            return Err(EmailError::Invalid);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAccount {
    pub address: String,
    pub imap: EmailEndpoint,
    pub smtp: EmailEndpoint,
}

impl EmailAccount {
    pub fn new(
        address: String,
        imap: EmailEndpoint,
        smtp: EmailEndpoint,
    ) -> Result<Self, EmailError> {
        let account = Self {
            address,
            imap,
            smtp,
        };
        account.validate()?;
        Ok(account)
    }
    pub fn validate(&self) -> Result<(), EmailError> {
        validate_address(&self.address)?;
        self.imap.validate()?;
        self.smtp.validate()
    }
}

#[derive(Clone)]
pub struct EmailSecrets {
    imap_password: Zeroizing<String>,
    smtp_password: Zeroizing<String>,
}
impl EmailSecrets {
    pub fn new(imap_password: impl Into<String>, smtp_password: impl Into<String>) -> Self {
        Self {
            imap_password: Zeroizing::new(imap_password.into()),
            smtp_password: Zeroizing::new(smtp_password.into()),
        }
    }
    pub fn imap_password(&self) -> &str {
        &self.imap_password
    }
    pub fn smtp_password(&self) -> &str {
        &self.smtp_password
    }
}
impl fmt::Debug for EmailSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EmailSecrets([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailCursor {
    pub uid_validity: u32,
    pub last_uid: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailInbox {
    pub uid_validity: u32,
    pub uid_next: u32,
    pub messages: Vec<EmailMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailMessage {
    pub uid: u32,
    pub from: String,
    pub subject: String,
    pub body: String,
    pub received_at: String,
    pub message_id: Option<String>,
    pub automated: bool,
    #[serde(default)]
    pub authentication_results: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmailOutgoing {
    pub id: String,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub automatic: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmailError {
    Invalid,
    Unauthorized,
    Unavailable,
    CursorChanged,
    Unknown,
}
impl fmt::Display for EmailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "Invalid email configuration or message",
            Self::Unauthorized => "Email authentication failed",
            Self::Unavailable => "Email service unavailable",
            Self::CursorChanged => "Email mailbox identity changed",
            Self::Unknown => "Email send outcome is unknown",
        })
    }
}
impl std::error::Error for EmailError {}

#[async_trait]
pub trait EmailTransport: Send + Sync {
    async fn verify(
        &self,
        account: &EmailAccount,
        secrets: &EmailSecrets,
    ) -> Result<EmailInbox, EmailError>;
    async fn inbox(
        &self,
        account: &EmailAccount,
        secrets: &EmailSecrets,
        after: Option<EmailCursor>,
    ) -> Result<EmailInbox, EmailError>;
    async fn send(
        &self,
        account: &EmailAccount,
        secrets: &EmailSecrets,
        message: &EmailOutgoing,
    ) -> Result<(), EmailError>;
}

mod client;
mod imap;
mod mime;
mod smtp;
mod wire;
pub use client::EmailClient;
fn validate_address(address: &str) -> Result<(), EmailError> {
    if address.is_empty()
        || address.len() > 254
        || !address.is_ascii()
        || address
            .bytes()
            .any(|ch| ch.is_ascii_whitespace() || ch.is_ascii_control())
        || address.parse::<lettre::Address>().is_err()
    {
        return Err(EmailError::Invalid);
    }
    Ok(())
}
#[cfg(test)]
mod tests;
