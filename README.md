# Polymarket MCP

[![CI](https://github.com/beejmaxx/polymarket-mcp-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/beejmaxx/polymarket-mcp-rs/actions/workflows/ci.yml)
[![Production API canary](https://github.com/beejmaxx/polymarket-mcp-rs/actions/workflows/live-canary.yml/badge.svg)](https://github.com/beejmaxx/polymarket-mcp-rs/actions/workflows/live-canary.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A self-contained Rust MCP server for Polymarket: public discovery, exact CLOB V2 books, wallet analytics, realtime market state, SQLite recording/replay, fill simulation, and safely gated optional trading.

The default `research` profile needs no API key or wallet. It exposes 27 public/realtime tools and does not advertise or accept calls to authenticated trading tools.

## Install in a minute

macOS or Linux:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/beejmaxx/polymarket-mcp-rs/main/scripts/install.sh | sh
polymarket-mcp-rs doctor
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/beejmaxx/polymarket-mcp-rs/main/scripts/install.ps1 | iex
polymarket-mcp-rs doctor
```

Then add it to a client:

```bash
codex mcp add polymarket -- polymarket-mcp-rs
# or
claude mcp add polymarket -- polymarket-mcp-rs
```

Prebuilt binaries and a one-click `polymarket-mcp.mcpb` bundle are attached to every [GitHub release](https://github.com/beejmaxx/polymarket-mcp-rs/releases). See [the install guide](docs/INSTALL.md) for Claude Desktop, VS Code, source builds, custom database paths, and Windows details.

## Why this server

- Broad replacement surface: 39 tools across public data, live books, research workflows, and opt-in account operations.
- Correct numeric boundaries: decimal financial values and 256-bit IDs stay strings across JSON, preventing floating-point and integer loss.
- Stateful market research: Rust maintains reconstructed books and feed health internally instead of sending every tick to the model.
- Honest replay: SQLite stores observations captured by this process, tracks dropped updates, and never presents them as complete exchange history.
- Safe defaults: trading tools are absent by default; exposing them and enabling mutation are separate decisions.
- Operationally simple: one binary, stdio transport, structured errors, an offline/online doctor, graceful shutdown, daily production canaries, checksums, SBOMs, and release provenance.

## A useful first prompt

> Find the five highest-volume active Polymarket markets. For each, show the current outcome prices, liquidity, spread, order-book imbalance, and estimated slippage for 100 shares. Explain data limitations and do not recommend a trade.

Other demo prompts are in [docs/DEMO.md](docs/DEMO.md).

## Tool profiles

| Profile | Tools | Intended use |
|---|---:|---|
| `core` | 17 | Public REST discovery, books, wallets, analysis, and simulation |
| `research` (default) | 27 | Core plus realtime watching and local recording/replay |
| `trading` | 29 | Core plus authenticated account and trading operations |
| `all` | 39 | Research and trading together |

Select a profile with `--tool-profile research` or `POLYMARKET_TOOL_PROFILE=research`. Hidden tools are removed from both `tools/list` and dispatch. Choosing `trading` or `all` only exposes the tool definitions; order mutation still requires `POLYMARKET_ENABLE_TRADING=true`, a signer, local caps, and per-operation confirmation.

Run `polymarket-mcp-rs tools` to inspect the default catalog or `polymarket-mcp-rs --tool-profile all tools --json` for complete schemas. See [docs/TOOLS.md](docs/TOOLS.md) for the grouped reference.

## Architecture

```text
MCP client (stdio)
        |
        v
tool profile + typed MCP schemas
        |
        v
validation and stable output mapping
   |              |                |                 |
Gamma/Data/    CLOB REST      realtime books      SQLite + trading
public APIs    exact books    WebSocket health    replay / audit
```

The WebSocket engine seeds each watched token from CLOB REST, then applies full snapshots and incremental price changes. Connection state, reconnects, source-specific counters, local receipt time, feed age, errors, and recording gaps remain explicit. SQLite writes use a bounded channel and dedicated writer thread; active recordings flush before watches stop on MCP EOF, SIGINT, or SIGTERM.

## Configuration

| Variable | Default | Purpose |
|---|---|---|
| `POLYMARKET_TOOL_PROFILE` | `research` | `core`, `research`, `trading`, or `all` |
| `POLYMARKET_MCP_DB` | `polymarket-mcp.sqlite3` | SQLite recording and approval-audit database |
| `POLYMARKET_WS_URL` | Production market WebSocket | Override the full WebSocket URL |
| `POLYMARKET_WS_PROXY` | unset | Unauthenticated `socks5://host:port` WebSocket proxy |
| `HTTPS_PROXY` / `HTTP_PROXY` | environment dependent | Proxy used by HTTP API clients |
| `RUST_LOG` | `info` | stderr log filtering |

`ALL_PROXY=socks5://...` is also honored for WebSocket connections when `POLYMARKET_WS_PROXY` is unset. The server has no telemetry. It connects only to configured Polymarket endpoints and writes only to its configured SQLite path.

## Trading is a separate opt-in

Read [SECURITY.md](SECURITY.md) and [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) first. At minimum:

```bash
export POLYMARKET_TOOL_PROFILE=all
export POLYMARKET_ENABLE_TRADING=true
export POLYMARKET_PRIVATE_KEY=0x...
export POLYMARKET_MAX_ORDER_USDC=100
export POLYMARKET_SIGNATURE_TYPE=eoa
```

EOA, legacy Proxy, Gnosis Safe, and POLY_1271 configurations are supported. `preview_order` performs policy and current-market-rule checks and creates a single-use five-minute approval. `place_order` consumes that approval and requires `confirm=true`; batch placement requires `PLACE_BATCH`. Cancellations have their own confirmations. Approval transitions are persisted before submission, and ambiguous network outcomes are never reported as safe retries.

## Development and evidence

Rust 1.88 or newer is required:

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo test --locked --test live_api -- --ignored --nocapture
```

Normal tests are offline and launch the compiled executable as a real stdio MCP subprocess. The ignored suite exercises current production Gamma, Data, CLOB REST, wallet, scanning, WebSocket, recording, replay, and simulation paths; it never places an order. Tool catalogs have committed golden contracts, and release tags rebuild on five OS/architecture targets.

- [Python replacement matrix](docs/PARITY.md)
- [Same-token Python comparison](docs/PYTHON_COMPARISON.md)
- [Compatibility](docs/COMPATIBILITY.md)
- [Contract stability](docs/STABILITY.md)
- [Roadmap](docs/ROADMAP.md)
- [Contributing](CONTRIBUTING.md)

This software provides market infrastructure, not trading recommendations or financial advice.
