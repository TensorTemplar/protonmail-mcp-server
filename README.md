# ProtonMail MCP Server

IMAP Mailbox MCP (Model Context Protocol) server for ProtonMail Bridge. Exposes email operations as MCP tools for AI assistants.

## Installation

### From crates.io

```bash
# With HTTP transport (recommended)
cargo install protonmail-mcp-server --features http

# Stdio only (default)
cargo install protonmail-mcp-server
```

### From source

```bash
git clone https://github.com/tensor-templar/protonmail-mcp-server
cd protonmail-mcp-server
cargo install --path . --features http
```

## Synopsis

ProtonMail MCP Server is an IMAP-backed MCP server for ProtonMail Bridge. It exposes mailbox operations as MCP tools so AI assistants can list, search, and retrieve mail through a local Bridge connection.

## Quickstart

```bash
cp .env.example .env
# Edit .env with your IMAP and MCP settings
cargo run
```

## HTTP Transport Deployment

### Architecture

```
┌─────────────┐     HTTPS/SSE      ┌──────────────────┐     IMAP
│   Agent 1   │◄──────────────────►│                  │◄──────────►┌────────────────┐
├─────────────┤                    │   MCP Server     │            │ ProtonMail     │
│   Agent 2   │◄──────────────────►│   (HTTP+SSE)     │◄──────────►│ Bridge         │
├─────────────┤                    │                  │            │                │
│   Agent N   │◄──────────────────►│ Per-session IMAP │◄──────────►│ localhost:1143 │
└─────────────┘                    └──────────────────┘            └────────────────┘
```

- Each MCP session gets its own IMAP connection
- Bearer token authentication required
- SSE (Server-Sent Events) for streaming responses

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `MCP_TRANSPORT` | `stdio` | Transport mode: `stdio` or `http` |
| `MCP_HTTP_BIND` | `127.0.0.1:8080` | HTTP server bind address |
| `MCP_AUTH_TOKEN` | (required for http) | Bearer token for authentication |
| `MCP_SSE_KEEPALIVE` | `true` | Enable SSE keep-alive pings (see note below) |

> **Note:** If using Python MCP SDK < 1.25.0, set `MCP_SSE_KEEPALIVE=false` to avoid JSON parsing errors.
> The older SDK can't handle empty SSE data fields sent as keep-alive pings.
> See: [python-sdk#1672](https://github.com/modelcontextprotocol/python-sdk/issues/1672)

### Running HTTP Server

```bash
# Via environment
MCP_TRANSPORT=http MCP_AUTH_TOKEN=secret123 ./target/release/protonmail-mcp-server

# Via CLI override
./target/release/protonmail-mcp-server --transport http --bind 0.0.0.0:8080
```

### Client Usage

```bash
# Initialize session
curl -X POST http://localhost:8080/mcp \
  -H "Authorization: Bearer secret123" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"my-agent","version":"1.0"}}}'
```

### Security Considerations

1. **Network binding**: Default `127.0.0.1` (localhost only). Use `0.0.0.0` for remote access.
2. **Authentication**: `MCP_AUTH_TOKEN` is mandatory for HTTP mode.
3. **TLS**: For production, deploy behind a reverse proxy (nginx, Caddy) with HTTPS.

## Available MCP Tools

| Tool | Description | Annotations |
|------|-------------|-------------|
| `list_mailboxes` | List available mailboxes | read-only |
| `search_emails` | Search emails with date filtering | read-only |
| `get_email` | Fetch full email content by ID | read-only |
| `get_current_date` | Get current UTC timestamp | read-only |
| `list_tags` | List available flags for a mailbox | read-only |
| `get_email_tags` | Get flags on a specific email | read-only |
| `apply_tag` | Apply a flag to an email | idempotent |
| `remove_tag` | Remove a flag from an email | destructive |
| `move_email` | Move email to another folder | destructive |
| `get_attachment` | Download attachment (file or base64) | destructive, open-world |

## Development

```bash
# Run tests
cargo test --features http

# Run clippy
cargo clippy --features http

# Format
cargo fmt
```
