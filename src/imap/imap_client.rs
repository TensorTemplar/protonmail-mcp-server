use crate::imap::types::{AttachmentData, EmailContent, EmailInfo, ImapError, ImapSettings, Result};
use async_native_tls::TlsConnector;
use futures::stream::StreamExt;
use mail_parser::{MessageParser, MimeHeaders};
use secrecy::ExposeSecret;
use std::time::Duration;

#[derive(Debug)]
pub struct ImapClient {
    settings: ImapSettings,
    connection_timeout: Duration,
}

impl ImapClient {
    pub fn new(settings: ImapSettings) -> Result<Self> {
        Ok(Self {
            settings,
            connection_timeout: Duration::from_secs(30),
        })
    }

    async fn connect(
        &self,
    ) -> Result<async_imap::Session<async_native_tls::TlsStream<async_std::net::TcpStream>>> {
        let connect_future = self.connect_internal();
        match async_std::future::timeout(self.connection_timeout, connect_future).await {
            Ok(result) => result,
            Err(_) => {
                log::error!("Connection timed out after {:?}", self.connection_timeout);
                Err(ImapError::ConnectionTimeout(format!(
                    "Connection to {}:{} timed out after {:?}",
                    self.settings.host, self.settings.port, self.connection_timeout
                )))
            }
        }
    }

    async fn connect_internal(
        &self,
    ) -> Result<async_imap::Session<async_native_tls::TlsStream<async_std::net::TcpStream>>> {
        log::info!(
            "Connecting to {}:{} using {}...",
            self.settings.host,
            self.settings.port,
            if self.settings.use_tls { "Direct TLS" } else { "STARTTLS" }
        );

        let tcp_stream =
            async_std::net::TcpStream::connect((self.settings.host.as_str(), self.settings.port))
                .await
                .map_err(ImapError::Io)?;

        let mut tls_builder = TlsConnector::new();
        if self.settings.skip_tls_verify {
            log::info!("TLS certificate verification disabled");
            tls_builder = tls_builder.danger_accept_invalid_certs(true);
        }

        if self.settings.use_tls {
            log::info!("Using direct TLS...");
            let tls_stream = tls_builder
                .connect(&self.settings.host, tcp_stream)
                .await
                .map_err(|e| ImapError::TlsSetup(e.to_string()))?;

            let client = async_imap::Client::new(tls_stream);
            log::info!("Logging in as {}...", self.settings.user);
            client
                .login(&self.settings.user, self.settings.password.expose_secret())
                .await
                .map_err(|(e, _)| ImapError::Login(e.to_string()))
        } else {
            log::info!("Using STARTTLS...");
            let mut client = async_imap::Client::new(tcp_stream);

            client
                .run_command_and_check_ok("STARTTLS", None)
                .await
                .map_err(|_| ImapError::StartTlsNotSupported)?;

            let stream = client.into_inner();
            let tls_stream = tls_builder
                .connect(&self.settings.host, stream)
                .await
                .map_err(|e| ImapError::TlsSetup(e.to_string()))?;

            let new_client = async_imap::Client::new(tls_stream);
            log::info!("Logging in as {}...", self.settings.user);
            new_client
                .login(&self.settings.user, self.settings.password.expose_secret())
                .await
                .map_err(|(e, _)| ImapError::Login(e.to_string()))
        }
    }

    pub async fn list_mailboxes(&self) -> Result<Vec<String>> {
        let mut session = self.connect().await?;
        log::info!("Listing mailboxes...");

        let mailbox_stream = session.list(None, Some("*")).await?;
        let mut names = Vec::new();
        let mut stream = Box::pin(mailbox_stream);

        while let Some(Ok(mailbox)) = stream.next().await {
            names.push(mailbox.name().to_string());
        }
        drop(stream);

        let _ = session.logout().await;
        Ok(names)
    }

    pub async fn search_emails(
        &self,
        mailbox: &str,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<EmailInfo>> {
        let mut session = self.connect().await?;

        session.select(mailbox).await
            .map_err(|e| ImapError::MailboxSelect(mailbox.to_string(), e.to_string()))?;

        log::info!("Searching with query: {}", query);
        let search_result = session.uid_search(query).await
            .map_err(|e| ImapError::SearchFailed(query.to_string(), e.to_string()))?;

        let mut uids: Vec<_> = search_result.into_iter().collect();
        uids.sort_by(|a, b| b.cmp(a)); // Newest first

        if let Some(limit) = limit {
            uids.truncate(limit as usize);
        }

        let mut results = Vec::new();
        for uid in uids {
            let uid_str = uid.to_string();
            if let Ok(mut fetch_stream) = session
                .uid_fetch(&uid_str, "BODY.PEEK[HEADER.FIELDS (SUBJECT FROM DATE)]")
                .await
                && let Some(Ok(fetch)) = fetch_stream.next().await
                && let Some(header) = fetch.header()
            {
                let header_str = String::from_utf8_lossy(header).to_string();
                results.push(EmailInfo {
                    uid: fetch.uid.unwrap_or(uid),
                    subject: extract_header(&header_str, "Subject:"),
                    from: extract_header(&header_str, "From:"),
                    date: extract_header(&header_str, "Date:"),
                });
            }
        }

        let _ = session.logout().await;
        Ok(results)
    }

    pub async fn fetch_email_by_uid(
        &self,
        mailbox: &str,
        uid: &str,
    ) -> Result<Option<EmailContent>> {
        let mut session = self.connect().await?;

        session.select(mailbox).await
            .map_err(|e| ImapError::MailboxSelect(mailbox.to_string(), e.to_string()))?;

        let mut content = EmailContent {
            email_id: uid.to_string(),
            ..Default::default()
        };

        if let Ok(mut fetch_stream) = session.uid_fetch(uid, "BODY.PEEK[]").await {
            while let Some(Ok(fetch)) = fetch_stream.next().await {
                if let Some(body) = fetch.body()
                    && let Some(parsed) = MessageParser::default().parse(body)
                {
                    content.subject = parsed.subject()
                        .map(|s| s.to_string())
                        .unwrap_or_default();

                    if let Some(mail_parser::Address::List(list)) = parsed.from()
                        && let Some(addr) = list.first()
                    {
                        content.sender = addr.address
                            .as_ref()
                            .map(|a| a.to_string())
                            .unwrap_or_default();
                    }

                    if let Some(mail_parser::Address::List(list)) = parsed.to() {
                        content.recipients = list.iter()
                            .filter_map(|a| a.address.as_ref().map(|s| s.to_string()))
                            .collect();
                    }

                    if let Some(mail_parser::Address::List(list)) = parsed.cc() {
                        content.cc_recipients = list.iter()
                            .filter_map(|a| a.address.as_ref().map(|s| s.to_string()))
                            .collect();
                    }

                    content.body = parsed.text_bodies()
                        .next()
                        .map(|p| String::from_utf8_lossy(p.contents()).to_string())
                        .or_else(|| parsed.html_bodies()
                            .next()
                            .map(|p| String::from_utf8_lossy(p.contents()).to_string()))
                        .unwrap_or_default();

                    for attachment in parsed.attachments() {
                        if let Some(name) = attachment.attachment_name() {
                            content.attachments.push(name.to_string());
                        }
                    }

                    if let Some(date) = parsed.date() {
                        use chrono::TimeZone;
                        content.received_time = chrono::Utc
                            .with_ymd_and_hms(
                                date.year as i32,
                                date.month as u32,
                                date.day as u32,
                                date.hour as u32,
                                date.minute as u32,
                                date.second as u32,
                            )
                            .single()
                            .unwrap_or_else(chrono::Utc::now);
                    }
                }
            }
        }

        let _ = session.logout().await;
        Ok(Some(content))
    }

    /// Fetch a specific attachment from an email
    pub async fn fetch_attachment(&self, mailbox: &str, uid: &str, attachment_name: &str) -> Result<Option<AttachmentData>> {
        let mut session = self.connect().await?;

        session.select(mailbox).await
            .map_err(|e| ImapError::MailboxSelect(mailbox.to_string(), e.to_string()))?;

        let mut attachment_data: Option<AttachmentData> = None;

        if let Ok(mut fetch_stream) = session.uid_fetch(uid, "BODY.PEEK[]").await {
            while let Some(Ok(fetch)) = fetch_stream.next().await {
                if let Some(body) = fetch.body()
                    && let Some(parsed) = MessageParser::default().parse(body)
                {
                    for attachment in parsed.attachments() {
                        if let Some(name) = attachment.attachment_name()
                            && name == attachment_name
                        {
                            let content_type = attachment.content_type()
                                .map(|ct| format!("{}/{}", ct.c_type, ct.c_subtype.as_deref().unwrap_or("octet-stream")))
                                .unwrap_or_else(|| "application/octet-stream".to_string());

                            attachment_data = Some(AttachmentData {
                                name: name.to_string(),
                                content_type,
                                data: attachment.contents().to_vec(),
                            });
                            break;
                        }
                    }
                }
            }
        }

        let _ = session.logout().await;
        Ok(attachment_data)
    }

    /// Get permanent flags available in a mailbox
    pub async fn get_permanent_flags(&self, mailbox: &str) -> Result<Vec<String>> {
        let mut session = self.connect().await?;

        let mailbox_info = session.select(mailbox).await
            .map_err(|e| ImapError::MailboxSelect(mailbox.to_string(), e.to_string()))?;

        // Standard IMAP flags that are always available
        let mut flags = vec![
            "\\Seen".to_string(),
            "\\Answered".to_string(),
            "\\Flagged".to_string(),
            "\\Deleted".to_string(),
            "\\Draft".to_string(),
        ];

        // Check for custom flags support
        for flag in mailbox_info.permanent_flags.iter() {
            let flag_str = format!("{:?}", flag);
            if !flags.contains(&flag_str) && !flag_str.contains("MayCreate") {
                flags.push(flag_str);
            }
        }

        let _ = session.logout().await;
        Ok(flags)
    }

    /// Get flags currently set on an email
    pub async fn fetch_flags(&self, mailbox: &str, uid: &str) -> Result<Vec<String>> {
        let mut session = self.connect().await?;

        session.select(mailbox).await
            .map_err(|e| ImapError::MailboxSelect(mailbox.to_string(), e.to_string()))?;

        let mut flags = Vec::new();

        if let Ok(mut fetch_stream) = session.uid_fetch(uid, "FLAGS").await
            && let Some(Ok(fetch)) = fetch_stream.next().await
        {
            for flag in fetch.flags() {
                let flag_str = match flag {
                    async_imap::types::Flag::Seen => "\\Seen",
                    async_imap::types::Flag::Answered => "\\Answered",
                    async_imap::types::Flag::Flagged => "\\Flagged",
                    async_imap::types::Flag::Deleted => "\\Deleted",
                    async_imap::types::Flag::Draft => "\\Draft",
                    async_imap::types::Flag::Recent => "\\Recent",
                    async_imap::types::Flag::MayCreate => continue,
                    async_imap::types::Flag::Custom(c) => {
                        flags.push(c.to_string());
                        continue;
                    }
                };
                flags.push(flag_str.to_string());
            }
        }

        let _ = session.logout().await;
        Ok(flags)
    }

    /// Add or remove a flag from an email
    pub async fn store_flag(&self, mailbox: &str, uid: &str, flag: &str, add: bool) -> Result<()> {
        let mut session = self.connect().await?;

        session.select(mailbox).await
            .map_err(|e| ImapError::MailboxSelect(mailbox.to_string(), e.to_string()))?;

        let flag_cmd = if add { "+FLAGS" } else { "-FLAGS" };
        let flag_value = format!("({})", flag);

        {
            let mut store_stream = session.uid_store(uid, format!("{} {}", flag_cmd, flag_value))
                .await
                .map_err(|e| ImapError::FlagOperation(e.to_string()))?;
            // Drain the stream
            while store_stream.next().await.is_some() {}
        }

        let _ = session.logout().await;
        Ok(())
    }

    /// Move an email to another mailbox (COPY + DELETE + EXPUNGE)
    pub async fn move_email(&self, uid: &str, from_mailbox: &str, to_mailbox: &str) -> Result<()> {
        let mut session = self.connect().await?;

        session.select(from_mailbox).await
            .map_err(|e| ImapError::MailboxSelect(from_mailbox.to_string(), e.to_string()))?;

        // Copy to destination
        session.uid_copy(uid, to_mailbox).await
            .map_err(|e| ImapError::FlagOperation(format!("Copy failed: {}", e)))?;

        // Mark original as deleted
        {
            let mut delete_stream = session.uid_store(uid, "+FLAGS (\\Deleted)")
                .await
                .map_err(|e| ImapError::FlagOperation(format!("Delete flag failed: {}", e)))?;
            while delete_stream.next().await.is_some() {}
        }

        // Expunge to remove deleted messages - use pin for the stream
        {
            let expunge_stream = session.expunge().await
                .map_err(|e| ImapError::FlagOperation(format!("Expunge failed: {}", e)))?;
            let mut pinned = Box::pin(expunge_stream);
            while pinned.next().await.is_some() {}
        }

        let _ = session.logout().await;
        Ok(())
    }
}

fn extract_header(data: &str, field: &str) -> Option<String> {
    data.lines()
        .find(|line| line.starts_with(field))
        .map(|line| line[field.len()..].trim().to_string())
}
