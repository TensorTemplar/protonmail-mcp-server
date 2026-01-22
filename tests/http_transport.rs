//! Integration tests for HTTP transport
//!
//! These tests verify the HTTP+SSE transport functionality including:
//! - Bearer token authentication
//! - MCP protocol over HTTP
//! - Session management

#![cfg(feature = "http")]

use std::sync::Arc;
use std::time::Duration;

use axum::{Router, middleware};
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, StreamableHttpServerConfig,
    session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

/// Mock server that implements basic MCP tools for testing
mod mock_server {
    use rmcp::{
        ErrorData as McpError,
        ServerHandler,
        handler::server::router::tool::ToolRouter,
        model::*,
        schemars::JsonSchema,
        tool, tool_handler, tool_router,
    };
    use serde::Deserialize;

    #[derive(Clone)]
    pub struct MockMailServer {
        tool_router: ToolRouter<MockMailServer>,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    pub struct EchoRequest {
        pub message: String,
    }

    #[tool_router]
    impl MockMailServer {
        pub fn new() -> Self {
            Self {
                tool_router: Self::tool_router(),
            }
        }

        #[tool(description = "Echo a message back")]
        async fn echo(
            &self,
            rmcp::handler::server::wrapper::Parameters(req): rmcp::handler::server::wrapper::Parameters<EchoRequest>,
        ) -> Result<CallToolResult, McpError> {
            Ok(CallToolResult::success(vec![Content::text(format!("Echo: {}", req.message))]))
        }

        #[tool(description = "Get server status")]
        async fn status(&self) -> Result<CallToolResult, McpError> {
            Ok(CallToolResult::success(vec![Content::text("Server is running")]))
        }
    }

    #[tool_handler]
    impl ServerHandler for MockMailServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo {
                protocol_version: ProtocolVersion::V_2024_11_05,
                capabilities: ServerCapabilities::builder()
                    .enable_tools()
                    .build(),
                server_info: Implementation::from_build_env(),
                instructions: Some("Mock server for testing".to_string()),
            }
        }
    }
}

use mock_server::MockMailServer;

/// Bearer token authentication middleware for tests
async fn auth_middleware(
    axum::extract::State(expected_token): axum::extract::State<Arc<String>>,
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
        _ => Err(axum::http::StatusCode::UNAUTHORIZED),
    }
}

/// Start a test server with auth on a random port
async fn start_server_with_auth(auth_token: &str) -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
    let ct = CancellationToken::new();

    let mcp_service = StreamableHttpService::new(
        || Ok(MockMailServer::new()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig {
            cancellation_token: ct.child_token(),
            stateful_mode: true,
            sse_keep_alive: None, // Disable keep-alive for tests
            ..Default::default()
        },
    );

    let auth_state = Arc::new(auth_token.to_string());
    let router = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(middleware::from_fn_with_state(auth_state, auth_middleware));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let ct_for_server = ct.clone();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { ct_for_server.cancelled_owned().await })
            .await
            .ok();
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    (format!("http://{}", addr), ct, handle)
}

/// Start a test server without auth
async fn start_server_no_auth() -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
    let ct = CancellationToken::new();

    let mcp_service = StreamableHttpService::new(
        || Ok(MockMailServer::new()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig {
            cancellation_token: ct.child_token(),
            stateful_mode: true,
            sse_keep_alive: None,
            ..Default::default()
        },
    );

    let router = Router::new().nest_service("/mcp", mcp_service);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let ct_for_server = ct.clone();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { ct_for_server.cancelled_owned().await })
            .await
            .ok();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    (format!("http://{}", addr), ct, handle)
}

// ============ Authentication Tests ============

#[tokio::test]
async fn test_missing_auth_returns_401() {
    let (addr, ct, handle) = start_server_with_auth("secret123").await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/mcp", addr))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401, "Missing auth should return 401");

    ct.cancel();
    handle.await.ok();
}

#[tokio::test]
async fn test_invalid_auth_returns_401() {
    let (addr, ct, handle) = start_server_with_auth("secret123").await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/mcp", addr))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", "Bearer wrongtoken")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401, "Invalid auth should return 401");

    ct.cancel();
    handle.await.ok();
}

#[tokio::test]
async fn test_valid_auth_allows_request() {
    let (addr, ct, handle) = start_server_with_auth("secret123").await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/mcp", addr))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", "Bearer secret123")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200, "Valid auth should return 200");

    ct.cancel();
    handle.await.ok();
}

// ============ MCP Protocol Tests (without auth) ============

#[tokio::test]
async fn test_initialize_returns_server_info() {
    let (addr, ct, handle) = start_server_no_auth().await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/mcp", addr))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();

    // Split SSE events by double newline
    let events: Vec<&str> = body.split("\n\n").filter(|e| !e.is_empty()).collect();
    assert!(events.len() >= 2, "Should have priming event and response. Got {} events: {:?}", events.len(), events);

    // Verify priming event
    let priming_event = events[0];
    assert!(priming_event.contains("id:"), "Priming should have id");
    assert!(priming_event.contains("retry:"), "Priming should have retry");

    // Verify initialize response contains server info
    let response_event = events[1];
    assert!(response_event.contains(r#""jsonrpc":"2.0""#), "Response should be JSON-RPC");
    assert!(response_event.contains(r#""id":1"#), "Response should have same ID");
    assert!(response_event.contains("protocolVersion"), "Response should have protocol version");

    ct.cancel();
    handle.await.ok();
}

// Note: Session-based subsequent requests (tools/list, tools/call) require
// the MCP client to properly consume the SSE stream. These are better tested
// with a full MCP client rather than raw HTTP requests. The initialize test
// above verifies the core HTTP+SSE transport is working.

#[tokio::test]
async fn test_missing_accept_header_returns_406() {
    let (addr, ct, handle) = start_server_no_auth().await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/mcp", addr))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json") // Missing text/event-stream
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 406, "Should reject without SSE accept");

    ct.cancel();
    handle.await.ok();
}

#[tokio::test]
async fn test_concurrent_sessions_have_different_ids() {
    let (addr, ct, handle) = start_server_no_auth().await;

    let client = reqwest::Client::new();

    // Create two sessions concurrently
    let (session1, session2) = tokio::join!(
        async {
            let response = client
                .post(format!("{}/mcp", addr))
                .header("Content-Type", "application/json")
                .header("Accept", "application/json, text/event-stream")
                .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"client1","version":"1.0"}}}"#)
                .send()
                .await
                .unwrap();
            response.headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        },
        async {
            let response = client
                .post(format!("{}/mcp", addr))
                .header("Content-Type", "application/json")
                .header("Accept", "application/json, text/event-stream")
                .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"client2","version":"1.0"}}}"#)
                .send()
                .await
                .unwrap();
            response.headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        }
    );

    assert!(session1.is_some(), "Session 1 should be created");
    assert!(session2.is_some(), "Session 2 should be created");
    assert_ne!(session1, session2, "Sessions should have different IDs");

    ct.cancel();
    handle.await.ok();
}
