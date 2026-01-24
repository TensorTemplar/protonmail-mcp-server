pub mod imap_client;
pub mod types;

pub use self::imap_client::ImapClient;
pub use self::types::{
    AttachmentData,
    EmailContent,
    EmailInfo,
    EmailMetadata,
    ImapError,
    ImapSettings,
    MoveEmailStatus,
    Result,
};

use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct ImapConnection {
    pub settings: ImapSettings,
    client: Option<ImapClient>,
    connected: bool,
}

impl ImapConnection {
    pub fn new(settings: ImapSettings) -> Self {
        ImapConnection {
            settings,
            client: None,
            connected: false,
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        log::info!("ImapConnection: Attempting connect...");

        match ImapClient::new(self.settings.clone()) {
            Ok(client) => match client.list_mailboxes().await {
                Ok(_) => {
                    log::info!("ImapConnection: Connect successful.");
                    self.client = Some(client);
                    self.connected = true;
                    Ok(())
                }
                Err(e) => {
                    log::error!("ImapConnection: Connect test failed: {}", e);
                    self.connected = false;
                    Err(e)
                }
            },
            Err(e) => {
                log::error!("ImapConnection: Client creation failed: {}", e);
                self.connected = false;
                Err(e)
            }
        }
    }

    /// Note: async to match existing server mod.rs expectations
    pub async fn is_connected(&self) -> bool {
        self.client.is_some() && self.connected
    }

    pub async fn list_mailboxes(&self) -> Result<Vec<String>> {
        log::debug!("ImapConnection: Listing mailboxes...");

        if let Some(client) = &self.client {
            client.list_mailboxes().await
        } else {
            Err(ImapError::Login("Not connected".to_string()))
        }
    }

    pub async fn search_emails(
        &self,
        mailbox: &str,
        since_date: Option<DateTime<Utc>>,
        limit: Option<usize>,
    ) -> Result<Vec<EmailMetadata>> {
        log::debug!("ImapConnection: Searching emails in '{}'...", mailbox);

        if let Some(client) = &self.client {
            let query = match since_date {
                Some(date) => format!("SINCE {}", date.format("%d-%b-%Y")),
                None => "ALL".to_string(),
            };

            let results = client.search_emails(mailbox, &query, limit.map(|l| l as u32)).await?;

            Ok(results
                .into_iter()
                .map(|info| {
                    let received_time = info.date
                        .as_ref()
                        .and_then(|d| chrono::DateTime::parse_from_rfc2822(d).ok())
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);

                    EmailMetadata {
                        email_id: info.uid.to_string(),
                        sender: info.from.unwrap_or_default(),
                        subject: info.subject.unwrap_or_default(),
                        received_time,
                    }
                })
                .collect())
        } else {
            Err(ImapError::Login("Not connected".to_string()))
        }
    }

    pub async fn get_email_content(&self, mailbox: &str, email_id: &str) -> Result<EmailContent> {
        log::debug!("ImapConnection: Getting content for email {} in '{}'...", email_id, mailbox);

        if let Some(client) = &self.client {
            match client.fetch_email_by_uid(mailbox, email_id).await? {
                Some(content) => Ok(content),
                None => Err(ImapError::MessageNotFound),
            }
        } else {
            Err(ImapError::Login("Not connected".to_string()))
        }
    }

    /// Get available tags/flags for a mailbox
    pub async fn get_available_tags(&self, mailbox: &str) -> Result<Vec<String>> {
        log::debug!("ImapConnection: Getting available tags for '{}'...", mailbox);

        if let Some(client) = &self.client {
            client.get_permanent_flags(mailbox).await
        } else {
            Err(ImapError::Login("Not connected".to_string()))
        }
    }

    /// Get tags currently on an email
    pub async fn get_email_tags(&self, mailbox: &str, email_id: &str) -> Result<Vec<String>> {
        log::debug!("ImapConnection: Getting tags for email {}...", email_id);

        if let Some(client) = &self.client {
            client.fetch_flags(mailbox, email_id).await
        } else {
            Err(ImapError::Login("Not connected".to_string()))
        }
    }

    /// Apply a tag to an email
    pub async fn apply_tag(&self, mailbox: &str, email_id: &str, tag: &str) -> Result<()> {
        log::debug!("ImapConnection: Applying tag '{}' to email {}...", tag, email_id);

        if let Some(client) = &self.client {
            client.store_flag(mailbox, email_id, tag, true).await
        } else {
            Err(ImapError::Login("Not connected".to_string()))
        }
    }

    /// Remove a tag from an email
    pub async fn remove_tag(&self, mailbox: &str, email_id: &str, tag: &str) -> Result<()> {
        log::debug!("ImapConnection: Removing tag '{}' from email {}...", tag, email_id);

        if let Some(client) = &self.client {
            client.store_flag(mailbox, email_id, tag, false).await
        } else {
            Err(ImapError::Login("Not connected".to_string()))
        }
    }

    /// Move an email to another mailbox
    pub async fn move_email(&self, email_id: &str, from_mailbox: &str, to_mailbox: &str) -> Result<()> {
        log::debug!("ImapConnection: Moving email {} from '{}' to '{}'...", email_id, from_mailbox, to_mailbox);

        if let Some(client) = &self.client {
            client.move_email(email_id, from_mailbox, to_mailbox).await
        } else {
            Err(ImapError::Login("Not connected".to_string()))
        }
    }

    /// Move multiple emails to another mailbox
    pub async fn move_emails(
        &self,
        email_ids: &[String],
        from_mailbox: &str,
        to_mailbox: &str,
    ) -> Result<Vec<MoveEmailStatus>> {
        log::debug!(
            "ImapConnection: Moving {} emails from '{}' to '{}'...",
            email_ids.len(),
            from_mailbox,
            to_mailbox
        );

        if let Some(client) = &self.client {
            client.move_emails(email_ids, from_mailbox, to_mailbox).await
        } else {
            Err(ImapError::Login("Not connected".to_string()))
        }
    }

    /// Fetch an attachment from an email
    pub async fn get_attachment(&self, mailbox: &str, email_id: &str, attachment_name: &str) -> Result<Option<AttachmentData>> {
        log::debug!("ImapConnection: Getting attachment '{}' from email {} in '{}'...", attachment_name, email_id, mailbox);

        if let Some(client) = &self.client {
            client.fetch_attachment(mailbox, email_id, attachment_name).await
        } else {
            Err(ImapError::Login("Not connected".to_string()))
        }
    }
}
