# polymarket-mcp-rs

A small, read-only MCP server for current Polymarket market discovery and CLOB V2 data, written in Rust.

This is the reviewed Phase 0 foundation for a larger market-recording and replay project. It intentionally does not contain trading, websocket recording, a database, GraphQL, or a web server yet.

## Current tools

- `server_status` — reports the server version, mode, and configured endpoints without making a network request.
- `search_markets` — searches active events and returns matching markets with stable IDs and outcome-token mappings.
- `get_market` — fetches one market and summarizes its current CLOB V2 order books.

All successful tool results are typed JSON. Tool failures contain a machine-readable error code, a readable message, and whether retrying may help.

## Architecture

```text
MCP client
    |
    v
server.rs       MCP schemas and thin handlers
    |
    v
app.rs          input validation and use cases
    |
    v
polymarket.rs   official Gamma and CLOB V2 SDK clients
    |
    +--> https://gamma-api.polymarket.com
    `--> https://clob.polymarket.com
```

The important request path is:

1. `main.rs` configures stderr-only logging and starts the stdio transport.
2. `server.rs` registers tools and converts MCP requests into application calls.
3. `app.rs` validates inputs and builds stable response types from upstream data.
4. `polymarket.rs` is the only module that directly uses the external SDK clients.
5. `types.rs` defines the MCP-facing contract; SDK response structs do not leak through it.

`lib.rs` keeps the application usable from tests or a future transport without turning the project into a multi-crate workspace.

## CLOB V2

The server uses `polymarket_client_sdk_v2` and explicitly configures the current production endpoint:

```text
https://clob.polymarket.com
```

It does not rely on the SDK's default hostname because SDK releases and examples may retain pre-cutover values. This project is read-only; it does not load wallet credentials or private keys.

## Build and test

The minimum supported Rust version is 1.88.

```bash
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

The normal tests do not require network access. Run the ignored production smoke test explicitly:

```bash
cargo test --test live_api -- --ignored --nocapture
```

## Run over stdio

Build a release binary:

```bash
cargo build --release
```

Example MCP client configuration:

```json
{
  "mcpServers": {
    "polymarket": {
      "command": "/absolute/path/to/polymarket-mcp-rs/target/release/polymarket-mcp-rs",
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

Logs go to stderr. Stdout is reserved for the MCP protocol.

## Planned direction

After this foundation is reviewed and understood:

1. Add accurate read-only tools for history, holders, events, and public wallet activity.
2. Add a current websocket order-book engine.
3. Record selected markets and feed-health information in SQLite.
4. Add deterministic replay of locally observed events and fill/slippage simulation.

Live trading remains explicitly out of scope until the read-only and recording paths are reliable.

See [docs/ROADMAP.md](docs/ROADMAP.md) for the phased scope and explicit non-goals.
