# Polymarket MCP

[![CI](https://github.com/beejmaxx/polymarket-mcp-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/beejmaxx/polymarket-mcp-rs/actions/workflows/ci.yml)
[![Production API canary](https://github.com/beejmaxx/polymarket-mcp-rs/actions/workflows/live-canary.yml/badge.svg)](https://github.com/beejmaxx/polymarket-mcp-rs/actions/workflows/live-canary.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A self-contained Rust MCP server for Polymarket: source-linked market briefs, public discovery, exact CLOB V2 books, wallet analytics, realtime market state, SQLite recording/replay, fill simulation, and safely gated optional trading.

The default `research` profile needs no API key or wallet. It exposes 30 public/realtime tools and does not advertise or accept calls to authenticated trading tools. A separate 11-tool `chatgpt` profile is compact, stateless, credential-blind, and available over Streamable HTTP with an optional inline market card.

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

Or use a package manager:

```bash
brew install beejmaxx/tap/polymarket-mcp-rs
```

```powershell
scoop bucket add beejmaxx https://github.com/beejmaxx/scoop-bucket
scoop install beejmaxx/polymarket-mcp-rs
```

Then add it to a client:

```bash
codex mcp add polymarket -- polymarket-mcp-rs
# or
claude mcp add polymarket -- polymarket-mcp-rs
```

Prebuilt binaries and a one-click `polymarket-mcp.mcpb` bundle are attached to every [GitHub release](https://github.com/beejmaxx/polymarket-mcp-rs/releases). See [the install guide](docs/INSTALL.md) for Claude Desktop, VS Code, source builds, custom database paths, and Windows details.

## Why this server

- Verified replacement surface: 46 tools across public data, live books, research workflows, and opt-in account operations, with an automated live audit against all 25 Python demo tools.
- Answer-ready workflows: one market brief joins metadata, executable books, history, fill impact, source URLs, timestamps, and limitations; wallet and explicit negative-risk event summaries avoid unnecessary tool chains.
- Correct numeric boundaries: decimal financial values and 256-bit IDs stay strings across JSON, preventing floating-point and integer loss.
- Stateful market research: Rust maintains reconstructed books and feed health internally instead of sending every tick to the model.
- Honest replay: SQLite stores observations captured by this process, tracks dropped updates, and never presents them as complete exchange history.
- Safe defaults: trading tools are absent by default; exposing them and enabling mutation are separate decisions.
- Operationally simple: one binary, stdio or stateless Streamable HTTP, structured errors, health and Prometheus endpoints, an offline/online doctor, graceful shutdown, daily production canaries, containers, checksums, SBOMs, and release provenance.

## A useful first prompt

> Find the five highest-volume active Polymarket markets. For each, show the current outcome prices, liquidity, spread, order-book imbalance, and estimated slippage for 100 shares. Explain data limitations and do not recommend a trade.

Other demo prompts are in [docs/DEMO.md](docs/DEMO.md).

## Tool profiles

| Profile | Tools | Intended use |
|---|---:|---|
| `chatgpt` | 11 | Compact stateless public research surface for hosted clients |
| `core` | 20 | Public REST discovery, books, wallets, answer-ready analysis, and simulation |
| `research` (default) | 30 | Core plus realtime watching and local recording/replay |
| `trading` | 36 | Core plus authenticated account, user-event streaming, and trading operations |
| `all` | 46 | Research and trading together |

Select a profile with `--tool-profile research` or `POLYMARKET_TOOL_PROFILE=research`. Hidden tools are removed from both `tools/list` and dispatch. Choosing `trading` or `all` only exposes the tool definitions; order mutation still requires `POLYMARKET_ENABLE_TRADING=true`, a signer, local caps, and per-operation confirmation. Public HTTP accepts only `chatgpt` or `core` and constructs an application instance that ignores credentials by design.

Run `polymarket-mcp-rs tools` to inspect the default catalog or `polymarket-mcp-rs --tool-profile all tools --json` for complete schemas. See [docs/TOOLS.md](docs/TOOLS.md) for the grouped reference.

## Architecture

```text
MCP client (stdio or Streamable HTTP)
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

The market WebSocket engine seeds each watched token from CLOB REST, then applies full snapshots and incremental price changes. Authenticated profiles can separately watch account order and trade events. Connection state, reconnects, source-specific counters, local receipt time, feed age, errors, and recording gaps remain explicit. SQLite writes use a bounded channel and dedicated writer thread; active recordings flush before watches stop on MCP EOF, SIGINT, or SIGTERM.

## Configuration

| Variable | Default | Purpose |
|---|---|---|
| `POLYMARKET_TOOL_PROFILE` | `research` | `chatgpt`, `core`, `research`, `trading`, or `all` |
| `POLYMARKET_MCP_DB` | OS application-data directory | SQLite recording and approval-audit database |
| `POLYMARKET_WS_URL` | Production market WebSocket | Override the full WebSocket URL |
| `POLYMARKET_WS_PROXY` | unset | Unauthenticated `socks5://host:port` WebSocket proxy |
| `HTTPS_PROXY` / `HTTP_PROXY` | environment dependent | Proxy used by HTTP API clients |
| `RUST_LOG` | `info` | stderr log filtering |

The default database is in `~/Library/Application Support/polymarket-mcp/` on macOS, `$XDG_DATA_HOME/polymarket-mcp/` (or `~/.local/share/polymarket-mcp/`) on Linux, and `%LOCALAPPDATA%\polymarket-mcp\` on Windows. `ALL_PROXY=socks5://...` is also honored for the public market WebSocket when `POLYMARKET_WS_PROXY` is unset; neither setting is needed on a normal unrestricted network. The server has no telemetry.

## Trading is a separate opt-in

Read [SECURITY.md](SECURITY.md) and [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) first. At minimum:

```bash
export POLYMARKET_TOOL_PROFILE=all
export POLYMARKET_ENABLE_TRADING=true
export POLYMARKET_PRIVATE_KEY=0x...
export POLYMARKET_MAX_ORDER_PUSD=100
export POLYMARKET_SIGNATURE_TYPE=eoa
```

EOA, legacy Proxy, Gnosis Safe, and POLY_1271 configurations are supported. `preview_order` performs policy and current-market-rule checks and creates a single-use five-minute approval. `place_order` consumes that approval and requires `confirm=true`; batch placement requires `PLACE_BATCH`. Cancellations have their own confirmations. Approval transitions are persisted before submission, and ambiguous network outcomes are never reported as safe retries. Account reads and user-event watches do not silently enable cancel-on-disconnect heartbeats.

## Development and evidence

Rust 1.90 or newer is required:

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo test --locked --test live_api -- --ignored --nocapture
```

Normal tests are offline and launch the compiled executable as a real stdio MCP subprocess. The ignored suite exercises current production Gamma, Data, CLOB REST, wallet, scanning, WebSocket, recording, replay, and simulation paths; it never places an order. Tool catalogs have committed golden contracts, and release tags rebuild on five OS/architecture targets.

The live semantic replacement audit launches both this server and the reference Python server through MCP, maps every Python demo tool, and fails on a Rust error or invariant violation:

```bash
python3 scripts/parity_harness.py
```

- [Python replacement matrix](docs/PARITY.md)
- [Migration from the Python server](docs/MIGRATING_FROM_PYTHON.md)
- [Same-token Python comparison](docs/PYTHON_COMPARISON.md)
- [Latest live parity audit summary](docs/PARITY_LIVE.md)
- [ChatGPT setup](docs/CHATGPT.md)
- [Read-only HTTP deployment](docs/DEPLOYMENT.md)
- [Compatibility](docs/COMPATIBILITY.md)
- [Contract stability](docs/STABILITY.md)
- [Roadmap](docs/ROADMAP.md)
- [Contributing](CONTRIBUTING.md)
- [Privacy](PRIVACY.md)
- [Support](SUPPORT.md)
- [Launch and demo kit](docs/LAUNCH.md)

This software provides market infrastructure, not trading recommendations or financial advice.
