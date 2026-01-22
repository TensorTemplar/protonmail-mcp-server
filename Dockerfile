FROM rust:1.88-slim-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY src ./src

RUN cargo build --release --features http

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/protonmail-mcp-server /app/protonmail-mcp-server

RUN useradd -r -u 1000 -s /bin/false mcpuser
USER mcpuser

ENV RUST_LOG=info
ENV MCP_TRANSPORT=http
ENV MCP_HTTP_BIND=0.0.0.0:3000

EXPOSE 3000

ENTRYPOINT ["/app/protonmail-mcp-server"]
