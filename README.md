# polymarket-mcp-rs

An early-release Polymarket MCP server in Rust, built for accurate public market intelligence and carefully gated optional trading. It covers discovery, current and historical market data, public wallet analytics, reconstructed CLOB books, SQLite recording/replay, fill simulation, and authenticated account operations.

The server uses Polymarket's official Rust V2 SDK and explicitly targets the current production endpoints rather than the SDK's legacy default hostname.

## What it exposes

- Discovery: `search_markets`, `list_markets`, `get_event`, `get_market`, `compare_markets`
- Market data: `get_order_book`, `get_price_history`, `get_market_holders`
- Public wallets: `get_wallet_positions`, `get_wallet_value`, `get_wallet_trades`, `get_wallet_activity`, `analyze_wallet_risk`
- Realtime: `watch_markets`, `get_live_snapshot`, `get_realtime_events`, `get_realtime_status`, `stop_watching`
- Market lab: `simulate_order`, `start_recording`, `stop_recording`, `list_recordings`, `replay_market`
- Trading: `trading_status`, `preview_order`, `get_order_approval`, `place_order`, `place_batch_orders`, `get_order`, `list_open_orders`, `list_account_trades`, `get_balance_allowance`, `cancel_order`, `cancel_market_orders`, `cancel_all_orders`

All successful results are typed structured JSON. Errors include a stable code, readable message, and retryability flag. Exact financial values and 256-bit identifiers cross the MCP boundary as strings, avoiding JSON floating-point loss.

See [docs/PARITY.md](docs/PARITY.md) for the Python-server capability mapping.

## Architecture

```text
MCP client (stdio)
        |
        v
server.rs          schemas + thin handlers
        |
        v
app.rs             validation + use cases + stable output mapping
   |          |             |                 |
   v          v             v                 v
API clients  realtime.rs  recorder.rs       trading.rs
Gamma/Data/  WS books     SQLite writer     opt-in signer,
CLOB REST    + health     + replay          approvals + caps
```

The websocket service keeps full books inside the process; MCP receives compact snapshots rather than raw tick streams. A watch is immediately seeded from CLOB REST as `rest_seed`. Full websocket snapshots and `price_change` placement/cancellation deltas then maintain the book as `websocket_snapshot` or `websocket_delta`. Connection state, reconnects, source-specific counters, local receipt time, feed age, errors, and dropped recording updates remain explicit.

SQLite writes run through a bounded channel on a dedicated writer thread. Replay is ordered by upstream timestamp and insertion ID, and only claims to replay observations captured by this server.

## Build and verify

Rust 1.88 or newer is required.

```bash
cargo build --release
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --test live_api -- --ignored --nocapture
```

Normal tests are offline. The ignored suite exercises current production Gamma, Data, CLOB REST, wallet, simulation, genuine websocket delivery, watch lifecycle, SQLite recording, and replay paths. It never places an order.

## Install

Until the first crates.io release, build from source:

```bash
git clone https://github.com/beejmaxx/polymarket-mcp-rs.git
cd polymarket-mcp-rs
cargo install --path . --locked
```

## Run over stdio

```json
{
  "mcpServers": {
    "polymarket": {
      "command": "/absolute/path/to/polymarket-mcp-rs/target/release/polymarket-mcp-rs",
      "env": {
        "RUST_LOG": "info",
        "POLYMARKET_MCP_DB": "/absolute/path/to/polymarket-mcp.sqlite3"
      }
    }
  }
}
```

Logs go only to stderr; stdout is reserved for MCP. `POLYMARKET_MCP_DB` defaults to `polymarket-mcp.sqlite3` in the process working directory.

## Configuration

| Variable | Default | Purpose |
|---|---|---|
| `POLYMARKET_MCP_DB` | `polymarket-mcp.sqlite3` | SQLite recording database |
| `POLYMARKET_WS_URL` | Official production market websocket | Override the full market websocket URL |
| `POLYMARKET_WS_PROXY` | unset | Unauthenticated `socks5://host:port` websocket proxy |
| `HTTPS_PROXY` / `HTTP_PROXY` | environment dependent | Proxy used by HTTP API clients |
| `RUST_LOG` | `info` | stderr log filtering |

`ALL_PROXY=socks5://...` is also honored for websocket connections when `POLYMARKET_WS_PROXY` is unset.

## Trading safety

Trading is disabled by default. Merely providing a private key does not enable it. To opt in:

```bash
export POLYMARKET_ENABLE_TRADING=true
export POLYMARKET_PRIVATE_KEY=0x...
export POLYMARKET_MAX_ORDER_USDC=100
# eoa, proxy, gnosis_safe, or poly1271
export POLYMARKET_SIGNATURE_TYPE=eoa
# Required for poly1271; optional for proxy/safe when automatic derivation is correct
export POLYMARKET_FUNDER_ADDRESS=0x...
```

The private key is parsed into a signer and never returned by a tool or logged. Authentication is lazy. EOA, legacy Proxy, Gnosis Safe, and deposit-wallet POLY_1271 configurations are supported. Authenticated clients send automatic heartbeats, and placement checks Polymarket's geographic eligibility response first. Order placement uses a two-step flow:

1. `preview_order` applies local type, side, unit, decimal, and notional policy checks and, by default, fetches the current tick size and minimum order size before returning a single-use approval that expires after five minutes. It does not guarantee exchange acceptance or execution.
2. `place_order` consumes that approval and also requires `confirm=true`. Batch placement requires `PLACE_BATCH` and applies the cap to the entire batch.

Approval transitions are written to SQLite before submission. `get_order_approval` distinguishes approved, validating, submitting, submitted, expired, and ambiguous `unknown` outcomes so a lost network response is not silently treated as a safe retry.

`cancel_order` requires `confirm=true`; cancel-by-market and cancel-all require `CANCEL_MARKET` and `CANCEL_ALL`. Preview, public tools, and authenticated account reads work while order mutation remains disabled.

See [CONTRIBUTING.md](CONTRIBUTING.md) for development expectations and [SECURITY.md](SECURITY.md) before configuring a signer.

This software provides market infrastructure, not trading recommendations or financial advice.
