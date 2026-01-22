use chrono::{DateTime, Utc};
use secrecy::Secret;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ImapSettings {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Secret<String>,
    pub use_tls: bool,
    pub skip_tls_verify: bool,
}

impl ImapSettings {
    pub fn new(user: String, password: String, host: String, port: u16, use_tls: bool) -> Self {
        Self::new_with_tls_options(user, password, host, port, use_tls, true)
    }

    pub fn new_with_tls_options(
        user: String,
        password: String,
        host: String,
        port: u16,
        use_tls: bool,
        skip_tls_verify: bool,
    ) -> Self {
        ImapSettings {
            host,
            port,
            user,
            password: Secret::new(password),
            use_tls,
            skip_tls_verify,
        }
    }
}

impl Default for ImapSettings {
    fn default() -> Self {
        ImapSettings {
            host: "127.0.0.1".to_string(),
            port: 1143,
            user: String::new(),
            password: Secret::new(String::new()),
            use_tls: false,
            skip_tls_verify: true,
        }
    }
}

#[derive(Debug, Error)]
pub enum ImapError {
    #[error("IMAP connection error: {0}")]
    Connection(#[from] async_imap::error::Error),
    #[error("IMAP login failed: {0}")]
    Login(String),
    #[error("TLS setup error: {0}")]
    TlsSetup(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid UTF-8 in message body")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("Message not found")]
    MessageNotFound,
    #[error("Server does not support STARTTLS")]
    StartTlsNotSupported,
    #[error("Connection timeout: {0}")]
    ConnectionTimeout(String),
    #[error("Failed to select mailbox '{0}': {1}")]
    MailboxSelect(String, String),
    #[error("Failed to search with query '{0}': {1}")]
    SearchFailed(String, String),
    #[error("Flag operation failed: {0}")]
    FlagOperation(String),
}

pub type Result<T> = std::result::Result<T, ImapError>;

/// Internal email info from IMAP fetch
#[derive(Debug, Clone)]
pub struct EmailInfo {
    pub uid: u32,
    pub subject: Option<String>,
    pub from: Option<String>,
    pub date: Option<String>,
}

/// Email metadata for search results (matches existing server mod.rs expectations)
#[derive(Debug, Clone, Serialize)]
pub struct EmailMetadata {
    pub email_id: String,
    pub sender: String,
    pub subject: String,
    pub received_time: DateTime<Utc>,
}

/// Full email content (matches existing server mod.rs expectations)
#[derive(Debug, Clone, Default, Serialize)]
pub struct EmailContent {
    pub email_id: String,
    pub sender: String,
    pub recipients: Vec<String>,
    pub cc_recipients: Vec<String>,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<String>,
    pub received_time: DateTime<Utc>,
}

/// Attachment data with content
#[derive(Debug, Clone, Serialize)]
pub struct AttachmentData {
    pub name: String,
    pub content_type: String,
    pub data: Vec<u8>,
}
