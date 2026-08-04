# Compatibility

## Release targets

| Operating system | Architecture | Release artifact |
|---|---|---|
| Linux (glibc) | x86-64 | `polymarket-mcp-rs-linux-x86_64.tar.gz` |
| Linux (glibc) | ARM64 | `polymarket-mcp-rs-linux-aarch64.tar.gz` |
| macOS | Intel | `polymarket-mcp-rs-macos-x86_64.tar.gz` |
| macOS | Apple Silicon | `polymarket-mcp-rs-macos-aarch64.tar.gz` |
| Windows | x86-64 | `polymarket-mcp-rs-windows-x86_64.zip` |

The MCP Bundle contains all five targets. Native Linux builds currently require glibc; Alpine/musl users should build from source until a musl artifact is added and exercised in the live suite.

## MCP clients

The server uses stdio and keeps stdout exclusively for MCP frames. It is suitable for clients that can launch a local stdio server, including Codex, Claude Code/Desktop, VS Code, Cursor, and other generic MCP hosts.

Automated tests exercise initialization, capability negotiation, `tools/list`, structured `tools/call`, typed errors, profile enforcement, shutdown, and a real compiled child process through the official Rust MCP SDK. The upstream MCP conformance runner currently tests servers by HTTP URL; it cannot directly drive this stdio-only deployment. Adding an HTTP transport solely for a CI badge is intentionally out of scope.

## Polymarket

The implementation targets the production Gamma API, Data API, CLOB REST endpoint through the official Rust V2 SDK, and production market WebSocket. A daily read-only canary detects upstream drift. The application explicitly configures the current production CLOB hostname rather than relying on an SDK legacy default.
