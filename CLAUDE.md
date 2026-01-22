# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is an IMAP Mailbox MCP (Model Context Protocol) server written in Rust. It exposes email operations as MCP tools that AI assistants can use to interact with IMAP email servers (including ProtonMail Bridge).

## Build Commands

```bash
cargo build              # Build the project
cargo build --release    # Build optimized release binary
cargo run                # Run the MCP server
cargo test               # Run all tests
cargo test <test_name>   # Run a specific test
cargo clippy             # Run linter
cargo fmt                # Format code
```

## Architecture

### Core Components

- **`src/server/mod.rs`** - Main MCP server implementation (`ImapMailboxServer`). Implements the `ServerHandler` trait from `rmcp` and defines MCP tools using the `#[tool]` attribute macro.

- **`src/server/tools.rs`** - Additional tool implementations (referenced but not yet present)

- **`src/imap/`** - IMAP connection handling module (referenced). Expected types:
  - `ImapConnection` - Manages IMAP connection state
  - `ImapSettings` - Connection configuration (username, password, host, port, TLS)
  - `EmailContent` - Full email with body and attachments
  - `EmailMetadata` - Email headers for listing

- **`src/config/`** - Configuration module (referenced). Provides `Config` type that converts to `ImapSettings`

### MCP Tools Exposed

| Tool | Description |
|------|-------------|
| `connect` | Connect to an IMAP server with credentials |
| `list_mailboxes` | List available mailboxes |
| `search_emails` | Search for emails with date filtering |
| `get_email` | Fetch full email content by ID |
| `get_current_date` | Get current UTC timestamp |

### Key Dependencies

- `rmcp` - Rust MCP protocol implementation (provides `ServerHandler`, `#[tool]` macro)
- `tokio` - Async runtime
- `schemars` - JSON Schema generation for tool parameters
- `serde` - Serialization/deserialization
- `chrono` - Date/time handling
- `tracing` - Logging

### Design Patterns

- Connection state is held in `Arc<Mutex<ImapConnection>>` for safe concurrent access
- Auto-connect mode: when constructed via `with_config()`, connects automatically on initialization
- All tools return `Result<CallToolResult, McpError>` and require active connection (except `connect` and `get_current_date`)
