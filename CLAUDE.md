# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is an IMAP Mailbox MCP (Model Context Protocol) server written in Rust. It exposes email operations as MCP tools that AI assistants can use to interact with IMAP email servers (including ProtonMail Bridge).

## Build Commands

```bash
cargo build                        # Build (stdio transport only)
cargo build --features http        # Build with HTTP transport
cargo build --release --features http  # Release build with HTTP
cargo run                          # Run with stdio transport
cargo test --features http         # Run all tests (requires http feature)
cargo test <test_name> --features http  # Run a specific test
cargo clippy --features http       # Run linter
cargo fmt                          # Format code
```

## Architecture

### Transport Modes

The server supports two transport modes configured via `MCP_TRANSPORT` env var:
- **stdio** (default): Standard input/output for local MCP connections
- **http**: HTTP+SSE server with Bearer token authentication for remote access

### Core Components

- **`src/main.rs`** - Entry point. Handles CLI args, transport setup (stdio vs HTTP+SSE), and auto-connect logic.

- **`src/config.rs`** - Configuration loading from environment variables. Defines `ImapConfig` and `ServerConfig`.

- **`src/server/mod.rs`** - Main MCP server implementation (`ImapMailboxServer`). Implements the `ServerHandler` trait from `rmcp` and defines MCP tools using the `#[tool]` attribute macro.

- **`src/imap/mod.rs`** - IMAP connection state management (`ImapConnection`). Wraps the low-level client with connection state tracking.

- **`src/imap/imap_client.rs`** - Low-level IMAP protocol client. Handles TLS/STARTTLS, IMAP commands, and email parsing.

- **`src/imap/types.rs`** - Data structures: `ImapSettings`, `EmailContent`, `EmailMetadata`, `AttachmentData`, `ImapError`.

### MCP Tools Exposed

| Tool | Annotation | Description |
|------|------------|-------------|
| `list_mailboxes` | read-only | List available mailboxes |
| `search_emails` | read-only | Search emails with date filtering |
| `get_email` | read-only | Fetch full email content by ID |
| `get_current_date` | read-only | Get current UTC timestamp |
| `list_tags` | read-only | List available flags for a mailbox |
| `get_email_tags` | read-only | Get flags on a specific email |
| `apply_tag` | idempotent | Apply a flag to an email |
| `remove_tag` | destructive | Remove a flag from an email |
| `move_email` | destructive | Move single email to folder |
| `move_emails` | destructive | Move multiple emails to folder |
| `get_attachment` | open-world | Download attachment (file or base64) |

### Key Dependencies

- `rmcp` - Rust MCP protocol implementation (provides `ServerHandler`, `#[tool]` macro)
- `async-imap` - Async IMAP client library
- `mail-parser` - Email MIME parsing
- `tokio` - Async runtime
- `axum` - HTTP server (http feature)

### Design Patterns

- Connection state is held in `Arc<Mutex<ImapConnection>>` for safe concurrent access
- Auto-connect mode: connects automatically on initialization when env vars are set
- All tools return `Result<CallToolResult, McpError>` and require active connection (except `get_current_date`)
- Input validation via `validate_non_empty()` and `validate_non_empty_list()` functions

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `IMAP_HOST` | - | IMAP server hostname |
| `IMAP_PORT` | `993` | IMAP server port |
| `IMAP_USERNAME` | - | IMAP username |
| `IMAP_PASSWORD` | - | IMAP password |
| `IMAP_USE_TLS` | `true` | Use direct TLS connection |
| `IMAP_SKIP_TLS_VERIFY` | `false` | Skip TLS certificate verification |
| `MCP_TRANSPORT` | `stdio` | Transport mode: `stdio` or `http` |
| `MCP_HTTP_BIND` | `127.0.0.1:8080` | HTTP server bind address |
| `MCP_AUTH_TOKEN` | - | Bearer token (required for http) |
