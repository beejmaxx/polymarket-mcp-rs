# Changelog

All notable changes will be documented here. The project follows semantic versioning after the initial `0.x` development series.

## Unreleased

- Safe `core`, `research`, `trading`, and `all` server-side tool profiles; `research` is the default and hides all authenticated operations.
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
- Authenticated pagination, balances and allowances, cancel-by-market, geoblock checks, automatic heartbeats, and durable approval lifecycle auditing.
- Bounded transactional recorder writes, surfaced writer errors, asynchronous database access, and strict replay decoding.
- Graceful recording/watch shutdown on stdio EOF, SIGINT, and SIGTERM.
- Compiled-binary stdio MCP tests and a dated same-token comparison with the Python server.
- Security policy and contribution guide.
