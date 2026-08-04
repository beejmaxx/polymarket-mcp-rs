# Roadmap and scope

## Completed

- Rust MCP server over stdio with typed JSON contracts and structured errors
- Explicit production Gamma, Data, CLOB REST V2, and CLOB websocket endpoints
- Discovery, events, market details, full books, history, holders, and comparisons
- Public wallet positions, valuation, trades, activity, and descriptive risk aggregation
- Background websocket watches with REST seeding, source labels, timestamps, health, and cancellation
- SQLite full-book recording with gap counts and deterministic local replay
- Current-book fill and slippage simulation
- Disabled-by-default authenticated order preview, single/batch placement, account order/trade reads, and cancellation
- Offline unit/MCP integration tests and an ignored production smoke suite

## Sensible follow-ups

- Authenticated user websocket events for immediate order/fill updates
- Cancel-by-condition/token convenience tool
- Recorded-book execution backtests across strategies
- Heartbeat-driven cancel-on-disconnect when the SDK's heartbeat feature is adopted
- HTTP/SSE transport and OAuth only if a real deployment needs remote multi-user access

## Non-goals

- GraphQL or Apollo Router
- Autonomous recommendations, “smart trades,” or portfolio-action advice
- Claiming locally recorded observations are complete historical exchange data
- A web dashboard, multi-service workspace, WASM strategies, or DataFusion before a demonstrated need
