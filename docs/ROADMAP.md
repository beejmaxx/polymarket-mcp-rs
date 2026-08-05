# Roadmap and scope

## Completed

- Rust MCP server over stdio and stateless Streamable HTTP with typed JSON contracts and structured errors
- Explicit production Gamma, Data, CLOB REST V2, and CLOB websocket endpoints
- Discovery, events, market details, full books, history, holders, and comparisons
- Public wallet positions, valuation, trades, activity, and descriptive risk aggregation
- Background websocket watches with REST seeding, snapshot-plus-delta reconstruction, lifecycle events, source labels, proxy support, timestamps, health, reconnects, and cancellation
- SQLite full-book and sequenced-event recording with transactional writes, separate gap/error counts, strict decoding, and deterministic local replay
- Current-book fill and slippage simulation
- Exact-decimal order-book microstructure analysis with depth, imbalance, microprice, near-touch liquidity, and sample impact
- Bounded concurrent scans across high-volume active markets, including explicitly caveated binary complement checks
- Disabled-by-default authenticated order preview, single/batch placement, wallet-mode configuration, paginated account reads, balances/allowances, user order/trade WebSocket watches, geoblock checks, and scoped cancellation
- Graceful recording/watch shutdown on MCP EOF, SIGINT, and SIGTERM
- Offline unit/in-process/compiled-stdio MCP tests, a genuine websocket production suite, GitHub Actions CI, a scheduled canary, and tagged binary-release automation
- Default-safe tool profiles, an online/offline doctor, golden catalog contracts, cross-platform installers, five native release targets, MCPB packaging, checksums, SBOMs, provenance, and official-registry automation
- Compact credential-blind ChatGPT profile, source-linked market and wallet briefs, explicit negative-risk event consistency analysis, and a portable inline MCP Apps market card
- Hardened public HTTP boundary with host/origin checks, request limits, timeouts, health/readiness, Prometheus metrics, containers, and Render/Fly deployment examples
- Live dual-server semantic parity harness covering every Python demo tool, plus versioned ChatGPT routing eval fixtures

## Sensible follow-ups

- Reconciliation helpers that search account orders after an ambiguous submission response
- Recorded-book execution research only after a defensible fill and queue-position model exists
- Explicit opt-in cancel-on-disconnect controls; account reads intentionally do not arm this policy
- OAuth only if a future deployment introduces private per-user data or actions; the current public endpoint intentionally needs no account

## Non-goals

- GraphQL or Apollo Router
- Autonomous recommendations, “smart trades,” or portfolio-action advice
- Claiming locally recorded observations are complete historical exchange data
- A standalone web dashboard, multi-service workspace, WASM strategies, or DataFusion before a demonstrated need
