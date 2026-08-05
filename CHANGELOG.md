# Changelog

All notable changes will be documented here. The project follows semantic versioning after the initial `0.x` development series.

## Unreleased

- Safe `chatgpt`, `core`, `research`, `trading`, and `all` server-side tool profiles; `research` is the local default and hides all authenticated operations.
- Source-linked `get_market_brief`, `get_wallet_summary`, and explicit negative-risk `analyze_event_consistency` research workflows.
- Stateless Streamable HTTP for credential-blind public profiles, including host/origin validation, body and time limits, per-peer rate limiting, health/readiness, Prometheus metrics, and short brief caching.
- Portable MCP Apps market-brief card with standard resource metadata and bridge notifications; all data remains available as ordinary structured tool output.
- Container, Compose, Render, and Fly deployment assets plus current ChatGPT tunnel and HTTPS setup documentation.
- Live dual-server parity harness covering all 25 Python demo tools and versioned ChatGPT routing eval fixtures.
- CLI help, version, profile-aware tool inspection, and offline/online structured diagnostics.
- Golden tool-catalog and structured-schema contract tests.
- Cross-platform installers, five native release targets, and a portable MCP Bundle.
- Release checksums, SPDX SBOM generation, GitHub provenance attestations, and automated official MCP Registry publishing.
- Client-specific installation, compatibility, stability, and threat-model documentation.
- Correct full-book reconstruction using both snapshots and `price_change` deltas.
- Configurable WebSocket endpoint and SOCKS5 proxy support.
- Truthful production WebSocket canary and detailed connection health.
- Sequenced market lifecycle, trade, tick-size, and best-price events.
- SQLite persistence and deterministic replay for sequenced realtime events.
- Exact-decimal order-book microstructure analysis and two-sided sample execution impact.
- Bounded concurrent multi-market microstructure scans and binary complement checks.
- Proxy, Safe, and POLY_1271 trading account configuration.
- Authenticated pagination, balances and allowances, cancel-by-market, geoblock checks, and durable approval lifecycle auditing.
- Bounded transactional recorder writes, surfaced writer errors, asynchronous database access, and strict replay decoding.
- Graceful recording/watch shutdown on stdio EOF, SIGINT, and SIGTERM.
- Compiled-binary stdio MCP tests and a dated same-token comparison with the Python server.
- Security policy and contribution guide.
- Correct market-level ranking and non-overlapping filtered pagination, including 7-day, 30-day, and end-time filters.
- Bounded price-history output with upstream counts and deterministic endpoint-preserving downsampling.
- Strict rejection of unknown tool parameters and corrected MCP read/destructive annotations.
- Authenticated buffered user order/trade WebSocket watches using the official SDK.
- Current pUSD terminology, with the legacy `POLYMARKET_MAX_ORDER_USDC` variable accepted only as a compatibility alias.
- Account reads no longer implicitly enable cancel-on-disconnect heartbeats.
- OS application-data database defaults and side-effect-free tool catalog inspection.
