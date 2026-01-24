use std::sync::Arc;
use chrono::{DateTime, Utc};
use rmcp::{
    ErrorData as McpError,
    ServerHandler,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::Parameters,
    },
    model::*,
    schemars::{self, JsonSchema},
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use tokio::sync::Mutex;

use crate::imap::{EmailMetadata, ImapConnection, ImapSettings, MoveEmailStatus};

fn not_connected_error() -> McpError {
    let message = "Not connected to IMAP server. Use connect() first.";
    log::error!("{}", message);
    McpError::internal_error(message, None)
}

fn invalid_input(
    field: &str,
    reason: &str,
    message: &str,
    expected: Option<&str>,
    hint: Option<&str>,
    value: Option<JsonValue>,
) -> McpError {
    let error_message = format!("Invalid input for {}: {}", field, message);
    log::error!("{}", error_message);

    let mut data = JsonMap::new();
    data.insert("field".to_string(), JsonValue::String(field.to_string()));
    data.insert("reason".to_string(), JsonValue::String(reason.to_string()));
    if let Some(expected) = expected {
        data.insert("expected".to_string(), JsonValue::String(expected.to_string()));
    }
    if let Some(hint) = hint {
        data.insert("hint".to_string(), JsonValue::String(hint.to_string()));
    }
    if let Some(value) = value {
        data.insert("value".to_string(), value);
    }

    McpError::invalid_params(error_message, Some(JsonValue::Object(data)))
}

fn validate_non_empty(field: &str, value: &str) -> Result<(), McpError> {
    if value.trim().is_empty() {
        return Err(invalid_input(
            field,
            "empty",
            "cannot be empty",
            Some("non-empty string"),
            Some("Provide a value."),
            None,
        ));
    }
    Ok(())
}

fn validate_non_empty_list(field: &str, values: &[String]) -> Result<(), McpError> {
    if values.is_empty() {
        return Err(invalid_input(
            field,
            "empty",
            "cannot be empty",
            Some("non-empty list"),
            Some("Provide at least one item."),
            None,
        ));
    }

    for (index, value) in values.iter().enumerate() {
        if value.trim().is_empty() {
            return Err(invalid_input(
                &format!("{}[{}]", field, index),
                "empty",
                "cannot be empty",
                Some("non-empty string"),
                Some("Remove empty entries from the list."),
                None,
            ));
        }
    }

    Ok(())
}

fn validate_limit(limit: usize) -> Result<(), McpError> {
    if limit == 0 {
        return Err(invalid_input(
            "limit",
            "invalid_range",
            "must be greater than zero",
            Some("greater than 0"),
            Some("Use a positive limit."),
            Some(JsonValue::Number(limit.into())),
        ));
    }
    Ok(())
}

/// IMAP Mailbox MCP Server
#[derive(Clone)]
pub struct ImapMailboxServer {
    connection: Arc<Mutex<ImapConnection>>,
    #[allow(dead_code)] // Reserved for potential future use (connection pooling, reconnection)
    settings: Arc<Mutex<ImapSettings>>,
    auto_connect: bool,
    tool_router: ToolRouter<ImapMailboxServer>,
}

impl Default for ImapMailboxServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Request to connect to an IMAP server
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConnectRequest {
    #[schemars(description = "Username for authentication")]
    pub username: String,

    #[schemars(description = "Password for authentication")]
    pub password: String,

    #[schemars(description = "IMAP server hostname")]
    pub host: String,

    #[schemars(description = "IMAP server port")]
    #[serde(default = "default_imap_port")]
    pub port: u16,

    #[schemars(description = "Whether to use TLS")]
    #[serde(default = "default_use_tls")]
    pub use_tls: bool,
}

fn default_imap_port() -> u16 {
    993
}

fn default_use_tls() -> bool {
    true
}

/// Request to search for emails
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchEmailsRequest {
    #[schemars(description = "Mailbox to search in")]
    #[serde(default = "default_inbox")]
    pub mailbox: String,

    #[schemars(description = "Only include emails after this date (ISO format)")]
    pub since_date: Option<String>,

    #[schemars(description = "Maximum number of emails to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_inbox() -> String {
    "INBOX".to_string()
}

fn default_limit() -> usize {
    30
}

/// Request to fetch email content
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetEmailRequest {
    #[schemars(description = "Email ID to fetch")]
    pub email_id: String,

    #[schemars(description = "Mailbox containing the email")]
    #[serde(default = "default_inbox")]
    pub mailbox: String,
}

/// Request to send a reply
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendReplyRequest {
    #[schemars(description = "Email ID to reply to")]
    pub email_id: String,

    #[schemars(description = "Reply text to send")]
    pub reply_text: String,
}

/// Request to list tags in a mailbox
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTagsRequest {
    #[schemars(description = "Mailbox to get available tags from")]
    #[serde(default = "default_inbox")]
    pub mailbox: String,
}

/// Request to get tags on an email
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetEmailTagsRequest {
    #[schemars(description = "Email ID to get tags for")]
    pub email_id: String,

    #[schemars(description = "Mailbox containing the email")]
    #[serde(default = "default_inbox")]
    pub mailbox: String,
}

/// Request to apply or remove a tag
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ModifyTagRequest {
    #[schemars(description = "Email ID to modify")]
    pub email_id: String,

    #[schemars(description = "Mailbox containing the email")]
    #[serde(default = "default_inbox")]
    pub mailbox: String,

    #[schemars(description = "Tag to apply or remove (e.g., \\\\Seen, \\\\Flagged, \\\\Answered)")]
    pub tag: String,
}

/// Request to move an email
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MoveEmailRequest {
    #[schemars(description = "Email ID to move")]
    pub email_id: String,

    #[schemars(description = "Source mailbox")]
    #[serde(default = "default_inbox")]
    pub from_mailbox: String,

    #[schemars(description = "Destination mailbox/folder")]
    pub to_mailbox: String,
}

/// Request to move multiple emails
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MoveEmailsRequest {
    #[schemars(description = "Email IDs to move")]
    pub email_ids: Vec<String>,

    #[schemars(description = "Source mailbox")]
    #[serde(default = "default_inbox")]
    pub from_mailbox: String,

    #[schemars(description = "Destination mailbox/folder")]
    pub to_mailbox: String,
}

/// Request to get an attachment
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAttachmentRequest {
    #[schemars(description = "Email ID containing the attachment")]
    pub email_id: String,

    #[schemars(description = "Mailbox containing the email")]
    #[serde(default = "default_inbox")]
    pub mailbox: String,

    #[schemars(description = "Name of the attachment to retrieve")]
    pub attachment_name: String,

    #[schemars(description = "Path to save the attachment (optional, returns base64 if not provided)")]
    pub save_path: Option<String>,
}

#[derive(Serialize)]
struct ListMailboxesResponse {
    mailboxes: Vec<String>,
}

#[derive(Serialize)]
struct SearchEmailsResponse {
    count: usize,
    emails: Vec<EmailMetadata>,
}

#[derive(Serialize)]
struct CurrentDateResponse {
    timestamp: String,
    iso8601: String,
}

#[derive(Serialize)]
struct ListTagsResponse {
    mailbox: String,
    tags: Vec<String>,
}

#[derive(Serialize)]
struct EmailTagsResponse {
    email_id: String,
    tags: Vec<String>,
}

#[derive(Serialize)]
struct TagOperationResponse {
    success: bool,
    email_id: String,
    tag: String,
}

#[derive(Serialize)]
struct MoveEmailResponse {
    success: bool,
    email_id: String,
    from_mailbox: String,
    to_mailbox: String,
}

#[derive(Serialize)]
struct MoveEmailsResponse {
    success: bool,
    moved: usize,
    failed: usize,
    from_mailbox: String,
    to_mailbox: String,
    results: Vec<MoveEmailStatus>,
}

#[derive(Serialize)]
struct AttachmentResponse {
    name: String,
    content_type: String,
    size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    saved_path: Option<String>,
}

#[tool_router]
impl ImapMailboxServer {
    pub fn new() -> Self {
        let settings = ImapSettings::default();
        Self {
            connection: Arc::new(Mutex::new(ImapConnection::new(settings.clone()))),
            settings: Arc::new(Mutex::new(settings)),
            auto_connect: false,
            tool_router: Self::tool_router(),
        }
    }

    pub fn with_config(config: crate::config::Config) -> Self {
        let settings = config.to_imap_settings();
        Self {
            connection: Arc::new(Mutex::new(ImapConnection::new(settings.clone()))),
            settings: Arc::new(Mutex::new(settings)),
            auto_connect: true,
            tool_router: Self::tool_router(),
        }
    }

    pub fn is_auto_connect(&self) -> bool {
        self.auto_connect
    }

    pub async fn auto_connect(&self) -> Result<(), crate::imap::ImapError> {
        let mut conn = self.connection.lock().await;
        conn.connect().await
    }

    /// Ensures connection is established, auto-connecting if configured
    async fn ensure_connected(&self) -> Result<(), McpError> {
        {
            let conn = self.connection.lock().await;
            if conn.is_connected().await {
                return Ok(());
            }
        }

        // Not connected - try auto-connect if enabled
        if self.auto_connect {
            let mut conn = self.connection.lock().await;
            conn.connect().await
                .map_err(|e| {
                    log::error!("Auto-connect failed: {}", e);
                    McpError::internal_error(format!("Auto-connect failed: {}", e), None)
                })?;
            Ok(())
        } else {
            Err(not_connected_error())
        }
    }

    #[tool(description = "List available mailboxes", annotations(read_only_hint = true))]
    async fn list_mailboxes(&self) -> Result<CallToolResult, McpError> {
        self.ensure_connected().await?;
        let connection = self.connection.lock().await;

        let mailboxes = connection.list_mailboxes().await
            .map_err(|e| {
                log::error!("Failed to list mailboxes: {}", e);
                McpError::internal_error(e.to_string(), None)
            })?;

        let response = ListMailboxesResponse { mailboxes };
        Ok(CallToolResult::success(vec![Content::json(response)?]))
    }

    #[tool(description = "Search for emails in a mailbox", annotations(read_only_hint = true))]
    async fn search_emails(&self, Parameters(req): Parameters<SearchEmailsRequest>) -> Result<CallToolResult, McpError> {
        validate_non_empty("mailbox", &req.mailbox)?;
        validate_limit(req.limit)?;
        if let Some(date_str) = &req.since_date {
            validate_non_empty("since_date", date_str)?;
        }

        self.ensure_connected().await?;
        let connection = self.connection.lock().await;

        let since_date = if let Some(date_str) = req.since_date {
            Some(DateTime::parse_from_rfc3339(&date_str)
                .map_err(|e| {
                    let message = format!("Invalid date format: {}. Use ISO 8601 format.", e);
                    log::error!("{}", message);
                    McpError::invalid_params(
                        message,
                        Some(JsonValue::Object({
                            let mut data = JsonMap::new();
                            data.insert("field".to_string(), JsonValue::String("since_date".to_string()));
                            data.insert("reason".to_string(), JsonValue::String("invalid_format".to_string()));
                            data.insert("expected".to_string(), JsonValue::String("ISO 8601 timestamp".to_string()));
                            data.insert("hint".to_string(), JsonValue::String("Example: 2025-01-31T10:15:00Z".to_string()));
                            data.insert("value".to_string(), JsonValue::String(date_str));
                            data
                        })),
                    )
                })?
                .with_timezone(&Utc))
        } else {
            None
        };

        let emails = connection.search_emails(&req.mailbox, since_date, Some(req.limit)).await
            .map_err(|e| {
                log::error!("Failed to search emails in {}: {}", req.mailbox, e);
                McpError::internal_error(e.to_string(), None)
            })?;

        let response = SearchEmailsResponse { count: emails.len(), emails };
        Ok(CallToolResult::success(vec![Content::json(response)?]))
    }

    #[tool(description = "Get email content by ID", annotations(read_only_hint = true))]
    async fn get_email(&self, Parameters(req): Parameters<GetEmailRequest>) -> Result<CallToolResult, McpError> {
        validate_non_empty("email_id", &req.email_id)?;
        validate_non_empty("mailbox", &req.mailbox)?;
        self.ensure_connected().await?;
        let connection = self.connection.lock().await;

        let email = connection.get_email_content(&req.mailbox, &req.email_id).await
            .map_err(|e| {
                log::error!("Failed to fetch email {} from {}: {}", req.email_id, req.mailbox, e);
                McpError::internal_error(e.to_string(), None)
            })?;

        Ok(CallToolResult::success(vec![Content::json(email)?]))
    }

    #[tool(description = "Get current date and time", annotations(read_only_hint = true))]
    async fn get_current_date(&self) -> Result<CallToolResult, McpError> {
        let now = Utc::now();
        let response = CurrentDateResponse {
            timestamp: now.format("%Y-%m-%d %H:%M:%S %Z").to_string(),
            iso8601: now.to_rfc3339(),
        };
        Ok(CallToolResult::success(vec![Content::json(response)?]))
    }

    #[tool(description = "List available tags/flags for a mailbox", annotations(read_only_hint = true))]
    async fn list_tags(&self, Parameters(req): Parameters<ListTagsRequest>) -> Result<CallToolResult, McpError> {
        validate_non_empty("mailbox", &req.mailbox)?;
        self.ensure_connected().await?;
        let connection = self.connection.lock().await;

        let tags = connection.get_available_tags(&req.mailbox).await
            .map_err(|e| {
                log::error!("Failed to list tags for {}: {}", req.mailbox, e);
                McpError::internal_error(e.to_string(), None)
            })?;

        let response = ListTagsResponse { mailbox: req.mailbox, tags };
        Ok(CallToolResult::success(vec![Content::json(response)?]))
    }

    #[tool(description = "Get tags/flags currently set on an email", annotations(read_only_hint = true))]
    async fn get_email_tags(&self, Parameters(req): Parameters<GetEmailTagsRequest>) -> Result<CallToolResult, McpError> {
        validate_non_empty("email_id", &req.email_id)?;
        validate_non_empty("mailbox", &req.mailbox)?;
        self.ensure_connected().await?;
        let connection = self.connection.lock().await;

        let tags = connection.get_email_tags(&req.mailbox, &req.email_id).await
            .map_err(|e| {
                log::error!("Failed to get tags for email {} in {}: {}", req.email_id, req.mailbox, e);
                McpError::internal_error(e.to_string(), None)
            })?;

        let response = EmailTagsResponse { email_id: req.email_id, tags };
        Ok(CallToolResult::success(vec![Content::json(response)?]))
    }

    #[tool(description = "Apply a tag/flag to an email", annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true))]
    async fn apply_tag(&self, Parameters(req): Parameters<ModifyTagRequest>) -> Result<CallToolResult, McpError> {
        validate_non_empty("email_id", &req.email_id)?;
        validate_non_empty("mailbox", &req.mailbox)?;
        validate_non_empty("tag", &req.tag)?;
        self.ensure_connected().await?;
        let connection = self.connection.lock().await;

        connection.apply_tag(&req.mailbox, &req.email_id, &req.tag).await
            .map_err(|e| {
                log::error!("Failed to apply tag {} to email {} in {}: {}", req.tag, req.email_id, req.mailbox, e);
                McpError::internal_error(e.to_string(), None)
            })?;

        let response = TagOperationResponse { success: true, email_id: req.email_id, tag: req.tag };
        Ok(CallToolResult::success(vec![Content::json(response)?]))
    }

    #[tool(description = "Remove a tag/flag from an email", annotations(read_only_hint = false, destructive_hint = true))]
    async fn remove_tag(&self, Parameters(req): Parameters<ModifyTagRequest>) -> Result<CallToolResult, McpError> {
        validate_non_empty("email_id", &req.email_id)?;
        validate_non_empty("mailbox", &req.mailbox)?;
        validate_non_empty("tag", &req.tag)?;
        self.ensure_connected().await?;
        let connection = self.connection.lock().await;

        connection.remove_tag(&req.mailbox, &req.email_id, &req.tag).await
            .map_err(|e| {
                log::error!("Failed to remove tag {} from email {} in {}: {}", req.tag, req.email_id, req.mailbox, e);
                McpError::internal_error(e.to_string(), None)
            })?;

        let response = TagOperationResponse { success: true, email_id: req.email_id, tag: req.tag };
        Ok(CallToolResult::success(vec![Content::json(response)?]))
    }

    #[tool(description = "Move an email to another mailbox/folder", annotations(read_only_hint = false, destructive_hint = true))]
    async fn move_email(&self, Parameters(req): Parameters<MoveEmailRequest>) -> Result<CallToolResult, McpError> {
        validate_non_empty("email_id", &req.email_id)?;
        validate_non_empty("from_mailbox", &req.from_mailbox)?;
        validate_non_empty("to_mailbox", &req.to_mailbox)?;
        self.ensure_connected().await?;
        let connection = self.connection.lock().await;

        connection.move_email(&req.email_id, &req.from_mailbox, &req.to_mailbox).await
            .map_err(|e| {
                log::error!(
                    "Failed to move email {} from {} to {}: {}",
                    req.email_id,
                    req.from_mailbox,
                    req.to_mailbox,
                    e
                );
                McpError::internal_error(e.to_string(), None)
            })?;

        let response = MoveEmailResponse {
            success: true,
            email_id: req.email_id,
            from_mailbox: req.from_mailbox,
            to_mailbox: req.to_mailbox,
        };
        Ok(CallToolResult::success(vec![Content::json(response)?]))
    }

    #[tool(description = "Move multiple emails to another mailbox/folder", annotations(read_only_hint = false, destructive_hint = true))]
    async fn move_emails(&self, Parameters(req): Parameters<MoveEmailsRequest>) -> Result<CallToolResult, McpError> {
        validate_non_empty_list("email_ids", &req.email_ids)?;
        validate_non_empty("from_mailbox", &req.from_mailbox)?;
        validate_non_empty("to_mailbox", &req.to_mailbox)?;
        self.ensure_connected().await?;
        let connection = self.connection.lock().await;

        let results = connection
            .move_emails(&req.email_ids, &req.from_mailbox, &req.to_mailbox)
            .await
            .map_err(|e| {
                log::error!(
                    "Failed to move emails from {} to {}: {}",
                    req.from_mailbox,
                    req.to_mailbox,
                    e
                );
                McpError::internal_error(e.to_string(), None)
            })?;

        let moved = results.iter().filter(|status| status.success).count();
        let failed = results.len().saturating_sub(moved);
        let response = MoveEmailsResponse {
            success: failed == 0,
            moved,
            failed,
            from_mailbox: req.from_mailbox,
            to_mailbox: req.to_mailbox,
            results,
        };

        Ok(CallToolResult::success(vec![Content::json(response)?]))
    }

    #[tool(description = "Get an attachment from an email. Optionally save to a file path, otherwise returns base64-encoded content.", annotations(read_only_hint = false, destructive_hint = true, open_world_hint = true))]
    async fn get_attachment(&self, Parameters(req): Parameters<GetAttachmentRequest>) -> Result<CallToolResult, McpError> {
        validate_non_empty("email_id", &req.email_id)?;
        validate_non_empty("mailbox", &req.mailbox)?;
        validate_non_empty("attachment_name", &req.attachment_name)?;
        if let Some(path) = &req.save_path {
            validate_non_empty("save_path", path)?;
        }
        self.ensure_connected().await?;
        let connection = self.connection.lock().await;

        let attachment = connection.get_attachment(&req.mailbox, &req.email_id, &req.attachment_name).await
            .map_err(|e| {
                log::error!(
                    "Failed to fetch attachment {} for email {} in {}: {}",
                    req.attachment_name,
                    req.email_id,
                    req.mailbox,
                    e
                );
                McpError::internal_error(e.to_string(), None)
            })?;

        match attachment {
            Some(data) => {
                if let Some(path) = req.save_path {
                    std::fs::write(&path, &data.data)
                        .map_err(|e| {
                            log::error!("Failed to save attachment to {}: {}", path, e);
                            McpError::internal_error(format!("Failed to save attachment: {}", e), None)
                        })?;

                    let response = AttachmentResponse {
                        name: data.name,
                        content_type: data.content_type,
                        size: data.data.len(),
                        data: None,
                        saved_path: Some(path),
                    };
                    Ok(CallToolResult::success(vec![Content::json(response)?]))
                } else {
                    use base64::Engine;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&data.data);

                    let response = AttachmentResponse {
                        name: data.name,
                        content_type: data.content_type,
                        size: data.data.len(),
                        data: Some(encoded),
                        saved_path: None,
                    };
                    Ok(CallToolResult::success(vec![Content::json(response)?]))
                }
            }
            None => Err(McpError::internal_error(
                {
                    let message = format!("Attachment '{}' not found in email {}", req.attachment_name, req.email_id);
                    log::error!("{}", message);
                    message
                },
                None
            )),
        }
    }
}

#[tool_handler]
impl ServerHandler for ImapMailboxServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "This server provides access to an IMAP email account through MCP tools. \
                Start by connecting to your IMAP server using the 'connect' tool, then use \
                'list_mailboxes' to see available mailboxes, and 'search_emails' to find \
                emails. Use 'get_email' to view full email content."
                    .to_string(),
            ),
        }
    }
}
