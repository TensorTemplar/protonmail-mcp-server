# ProtonMail MCP Server

Unofficial IMAP-backed MCP (Model Context Protocol) server for ProtonMail Bridge. Exposes mailbox operations as MCP tools so AI assistants can list, search, and retrieve mail through a local Bridge connection.

## Quickstart

### Install

```bash
# From crates.io (recommended)
cargo install protonmail-mcp-server --features http

# Or from source
git clone https://github.com/tensor-templar/protonmail-mcp-server
cd protonmail-mcp-server
cargo install --path . --features http
```

### Configure and Run

```bash
cp .env.example .env
# Edit .env with your IMAP and MCP settings
protonmail-mcp-server
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

## Sponsors

If you find this project useful, consider supporting us:

[![Sponsor $10](https://img.shields.io/badge/Sponsor-%2410-green?style=for-the-badge&logo=stripe)](https://buy.stripe.com/28o4gL5Qu9URaFaeUU)
[![Sponsor $6/month](https://img.shields.io/badge/Sponsor-%246%2Fmonth-orange?style=for-the-badge&logo=stripe)](https://buy.stripe.com/aEUaF992G5EBaFa8wx)

Please include your name and project in the payment notes to be listed in SPONSORS.md.
