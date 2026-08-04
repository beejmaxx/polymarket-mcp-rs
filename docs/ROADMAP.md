# Roadmap

The project grows in small, reviewable phases. New infrastructure is added only when a working feature requires it.

## Phase 0: foundation — complete

- Standalone Rust MCP server over stdio
- Current `rmcp` and official Polymarket Rust V2 SDK
- Explicit production CLOB V2 endpoint
- Structured `server_status`, `search_markets`, and `get_market` tools
- Typed error responses
- Offline unit and MCP protocol tests
- Ignored production API smoke test

## Phase 1: read-only coverage

Add accurate public-data tools without credentials:

- Event details
- Full order books
- Price history
- Market holders
- Market comparison
- Public wallet positions and activity

Prefer composable filters and structured responses over many overlapping tools or generated trading advice.

## Phase 2: market recorder

Add one focused Rust-specific feature set:

- Current CLOB V2 websocket ingestion
- Locally maintained order books
- Feed freshness and reconnect health
- Recording selected markets to SQLite
- Replay of events observed by this server
- Fill and slippage simulation against current or recorded books

The recorder will report gaps and uncertainty explicitly. It will not claim complete exchange history when the upstream feed cannot prove continuity.

## Phase 3: optional trading

Trading remains postponed until the read-only and recording paths are reliable. Any future implementation must use CLOB V2 and include order preview, confirmation, idempotency, exposure limits, secret isolation, and audit records.

## Non-goals for the initial project

- GraphQL or Apollo Router
- Web dashboard
- Multiple Cargo crates or services
- Autonomous trading recommendations
- WASM strategies
- Parquet/DataFusion infrastructure

