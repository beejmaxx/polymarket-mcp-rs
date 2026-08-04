# Roadmap and scope

## Completed

- Rust MCP server over stdio with typed JSON contracts and structured errors
- Explicit production Gamma, Data, CLOB REST V2, and CLOB websocket endpoints
- Discovery, events, market details, full books, history, holders, and comparisons
- Public wallet positions, valuation, trades, activity, and descriptive risk aggregation
- Background websocket watches with REST seeding, snapshot-plus-delta reconstruction, lifecycle events, source labels, proxy support, timestamps, health, reconnects, and cancellation
- SQLite full-book recording with transactional writes, gap/error counts, strict decoding, and deterministic local replay
- Current-book fill and slippage simulation
- Disabled-by-default authenticated order preview, single/batch placement, wallet-mode configuration, paginated account reads, balances/allowances, heartbeats, geoblock checks, and scoped cancellation
- Offline unit/MCP integration tests and a genuine websocket production suite

## Sensible follow-ups

- Authenticated user websocket events for immediate order/fill updates and MCP notifications
- Reconciliation helpers that search account orders after an ambiguous submission response
- Recorded-book execution backtests across strategies
- Explicit cancel-on-disconnect policy controls beyond the SDK's automatic authenticated heartbeats
- GitHub Actions CI, a scheduled production canary, and tagged binary releases
- HTTP/SSE transport and OAuth only if a real deployment needs remote multi-user access

## Non-goals

- GraphQL or Apollo Router
- Autonomous recommendations, “smart trades,” or portfolio-action advice
- Claiming locally recorded observations are complete historical exchange data
- A web dashboard, multi-service workspace, WASM strategies, or DataFusion before a demonstrated need
