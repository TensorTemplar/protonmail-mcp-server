use crate::imap::ImapSettings;
use secrecy::Secret;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Environment variable not set: {0}")]
    EnvVar(#[from] std::env::VarError),
    #[error("Could not parse port number: {0}")]
    ParsePort(#[from] std::num::ParseIntError),
    #[error("Could not parse boolean value: {0}")]
    ParseBool(#[from] std::str::ParseBoolError),
}

pub type Result<T> = std::result::Result<T, ConfigError>;

/// IMAP connection configuration
#[derive(Debug, Clone)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Secret<String>,
    pub use_tls: bool,
    pub skip_tls_verify: bool,
}

/// MCP server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Transport mode: "stdio" or "http"
    pub transport: String,
    /// HTTP bind address (only used when transport = "http")
    pub http_bind: String,
    /// Authentication token for HTTP mode (required when transport = "http")
    pub auth_token: Option<String>,
    /// Enable SSE keep-alive pings (default: true)
    /// Disable if using Python MCP SDK < 1.25.0 which can't parse empty SSE data.
    /// See: https://github.com/modelcontextprotocol/python-sdk/issues/1672
    pub sse_keepalive: bool,
}

/// Complete configuration loaded from environment
#[derive(Debug, Clone)]
pub struct Config {
    pub imap: ImapConfig,
    pub server: ServerConfig,
}

fn parse_bool(s: &str) -> bool {
    matches!(s.to_lowercase().as_str(), "true" | "1" | "yes")
}

pub fn load_config() -> Result<Config> {
    dotenv::dotenv().ok();

    // IMAP configuration
    let host = std::env::var("IMAP_HOST")?;

    let use_tls_str = std::env::var("IMAP_USE_TLS").unwrap_or_else(|_| "false".to_string());
    let use_tls = parse_bool(&use_tls_str);

    let default_port = if use_tls { "993" } else { "1143" };
    let port_str = std::env::var("IMAP_PORT").unwrap_or_else(|_| default_port.to_string());
    let port: u16 = port_str.parse()?;

    let user = std::env::var("IMAP_USERNAME").or_else(|_| std::env::var("IMAP_USER"))?;
    let password = Secret::new(std::env::var("IMAP_PASSWORD")?);

    let skip_tls_verify_str = std::env::var("IMAP_SKIP_TLS_VERIFY")
        .unwrap_or_else(|_| "true".to_string());
    let skip_tls_verify = parse_bool(&skip_tls_verify_str);

    let imap = ImapConfig {
        host,
        port,
        user,
        password,
        use_tls,
        skip_tls_verify,
    };

    // Server configuration
    let transport = std::env::var("MCP_TRANSPORT").unwrap_or_else(|_| "stdio".to_string());
    let http_bind = std::env::var("MCP_HTTP_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let auth_token = std::env::var("MCP_AUTH_TOKEN").ok();
    let sse_keepalive = std::env::var("MCP_SSE_KEEPALIVE")
        .map(|s| parse_bool(&s))
        .unwrap_or(true); // Default enabled

    let server = ServerConfig {
        transport,
        http_bind,
        auth_token,
        sse_keepalive,
    };

    Ok(Config { imap, server })
}

impl ImapConfig {
    pub fn to_imap_settings(&self) -> ImapSettings {
        ImapSettings {
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            password: self.password.clone(),
            use_tls: self.use_tls,
            skip_tls_verify: self.skip_tls_verify,
        }
    }
}

// Keep backward compatibility
impl Config {
    pub fn to_imap_settings(&self) -> ImapSettings {
        self.imap.to_imap_settings()
    }
}
