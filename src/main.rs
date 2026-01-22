use clap::Parser;
use log::LevelFilter;
use protonmail_mcp_server::config::{load_config, Config};
use protonmail_mcp_server::server::ImapMailboxServer;

#[derive(Parser)]
#[command(name = "protonmail-mcp-server")]
#[command(about = "IMAP Mailbox MCP Server for ProtonMail Bridge")]
struct Args {
    /// Transport mode: stdio or http (overrides MCP_TRANSPORT env var)
    #[arg(long)]
    transport: Option<String>,

    /// HTTP bind address (overrides MCP_HTTP_BIND env var)
    #[arg(long)]
    bind: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger (logs go to stderr, leaving stdout for MCP protocol)
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .format_timestamp_secs()
        .format_target(false)
        .init();

    // Load configuration from .env file
    let config = match load_config() {
        Ok(config) => config,
        Err(e) => {
            log::error!("Could not load config from environment: {}", e);
            log::error!("Required: IMAP_HOST, IMAP_USERNAME, IMAP_PASSWORD");
            return Err(e.into());
        }
    };

    // CLI args override env vars
    let args = Args::parse();
    let transport = args.transport.unwrap_or(config.server.transport.clone());
    let bind = args.bind.unwrap_or(config.server.http_bind.clone());

    log::info!("Starting ProtonMail MCP server (transport: {})...", transport);
    log::info!(
        "IMAP: {}:{} (TLS: {}, skip_verify: {})",
        config.imap.host,
        config.imap.port,
        config.imap.use_tls,
        config.imap.skip_tls_verify
    );

    match transport.as_str() {
        "stdio" => run_stdio_server(config).await,
        #[cfg(feature = "http")]
        "http" => run_http_server(config, &bind).await,
        #[cfg(not(feature = "http"))]
        "http" => {
            log::error!("HTTP transport not available. Rebuild with --features http");
            Err("HTTP transport not compiled in".into())
        }
        other => {
            log::error!("Unknown transport: {}. Use 'stdio' or 'http'.", other);
            Err(format!("Unknown transport: {}", other).into())
        }
    }
}

/// Create server from config
fn create_server(config: &Config) -> ImapMailboxServer {
    ImapMailboxServer::with_config(config.clone())
}

/// Run the MCP server over stdio transport
#[cfg(feature = "stdio")]
async fn run_stdio_server(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    use rmcp::ServiceExt;
    use tokio::io::{stdin, stdout};

    let server = create_server(&config);

    // Auto-connect for stdio mode (eager connection)
    if server.is_auto_connect() {
        log::info!("Attempting auto-connect to IMAP server...");
        if let Err(e) = server.auto_connect().await {
            log::warn!("Auto-connect failed: {}. Will retry on first tool call.", e);
        } else {
            log::info!("Auto-connected to IMAP server successfully");
        }
    }

    // Set up stdio transport
    let transport = (stdin(), stdout());

    log::info!("MCP server ready, accepting connections over stdio");

    // Start the MCP server
    let mcp_server = server.serve(transport).await?;

    // Wait for completion
    mcp_server.waiting().await?;

    log::info!("MCP server shut down");
    Ok(())
}

#[cfg(not(feature = "stdio"))]
async fn run_stdio_server(_config: Config) -> Result<(), Box<dyn std::error::Error>> {
    log::error!("Stdio transport not available. Rebuild with default features or --features stdio");
    Err("Stdio transport not compiled in".into())
}

/// Run the MCP server over HTTP+SSE transport
#[cfg(feature = "http")]
async fn run_http_server(config: Config, bind: &str) -> Result<(), Box<dyn std::error::Error>> {
    use axum::{Router, middleware};
    use rmcp::transport::streamable_http_server::{
        StreamableHttpService, StreamableHttpServerConfig,
        session::local::LocalSessionManager,
    };
    use std::sync::Arc;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    // Get auth token (required for HTTP mode)
    let auth_token = config.server.auth_token.clone().ok_or_else(|| {
        log::error!("MCP_AUTH_TOKEN is required for HTTP transport");
        "MCP_AUTH_TOKEN not set"
    })?;

    let ct = CancellationToken::new();

    // SSE keep-alive: enabled by default (15s interval), can be disabled for
    // compatibility with Python MCP SDK < 1.25.0 which can't parse empty SSE data.
    // See: https://github.com/modelcontextprotocol/python-sdk/issues/1672
    let sse_keep_alive = if config.server.sse_keepalive {
        Some(Duration::from_secs(15))
    } else {
        log::info!("SSE keep-alive disabled (MCP_SSE_KEEPALIVE=false)");
        None
    };

    // Factory creates fresh server per session with its own IMAP connection
    let config_for_factory = config.clone();
    let mcp_service = StreamableHttpService::new(
        move || {
            let srv = create_server(&config_for_factory);
            // Connection happens lazily on first tool call via ensure_connected()
            Ok(srv)
        },
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig {
            cancellation_token: ct.child_token(),
            sse_keep_alive,
            ..Default::default()
        },
    );

    // Build router with auth middleware
    let auth_state = Arc::new(auth_token);
    let router = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(middleware::from_fn_with_state(auth_state, auth_middleware));

    let listener = tokio::net::TcpListener::bind(bind).await?;
    log::info!("MCP HTTP server listening on http://{}", bind);
    log::info!("Endpoint: POST http://{}/mcp", bind);

    // Handle graceful shutdown on Ctrl+C
    let ct_for_shutdown = ct.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c().await.ok();
            log::info!("Received shutdown signal");
            ct_for_shutdown.cancel();
        })
        .await?;

    log::info!("MCP HTTP server shut down");
    Ok(())
}

/// Bearer token authentication middleware
#[cfg(feature = "http")]
async fn auth_middleware(
    axum::extract::State(expected_token): axum::extract::State<std::sync::Arc<String>>,
    headers: axum::http::HeaderMap,
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match auth_header {
        Some(token) if token == expected_token.as_str() => Ok(next.run(request).await),
        Some(_) => {
            log::warn!("Invalid bearer token provided");
            Err(axum::http::StatusCode::UNAUTHORIZED)
        }
        None => {
            log::warn!("Missing Authorization header");
            Err(axum::http::StatusCode::UNAUTHORIZED)
        }
    }
}
