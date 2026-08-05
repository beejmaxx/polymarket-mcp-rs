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

The server supports local stdio and stateless Streamable HTTP. Stdio keeps stdout exclusively for MCP frames and works with Codex, Claude Code/Desktop, VS Code, Cursor, and other generic MCP hosts. HTTP uses the official Rust SDK transport at `/mcp` and is intended for the credential-free `chatgpt` or `core` profile.

Automated tests exercise initialization, capability negotiation, `tools/list`, `resources/list`, UI resource reads, structured `tools/call`, typed errors, profile enforcement, shutdown, a real compiled child process, and a real HTTP client through the official Rust MCP SDK. The inline card uses the portable MCP Apps resource URI and bridge; all tools remain fully usable without UI.

## Polymarket

The implementation targets the production Gamma API, Data API, CLOB REST endpoint through the official Rust V2 SDK, and production market WebSocket. A daily read-only canary detects upstream drift. The application explicitly configures the current production CLOB hostname rather than relying on an SDK legacy default.
