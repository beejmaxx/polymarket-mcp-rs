# polymarket-mcp-rs

A production-oriented Polymarket MCP server in Rust. It covers public discovery, current and historical market data, public wallet analytics, background CLOB books, SQLite recording/replay, fill simulation, and opt-in policy-gated trading.

The server uses Polymarket's official Rust V2 SDK and explicitly targets the current production endpoints rather than the SDK's legacy default hostname.

## What it exposes

- Discovery: `search_markets`, `list_markets`, `get_event`, `get_market`, `compare_markets`
- Market data: `get_order_book`, `get_price_history`, `get_market_holders`
- Public wallets: `get_wallet_positions`, `get_wallet_value`, `get_wallet_trades`, `get_wallet_activity`, `analyze_wallet_risk`
- Realtime: `watch_markets`, `get_live_snapshot`, `get_realtime_status`, `stop_watching`
- Market lab: `simulate_order`, `start_recording`, `stop_recording`, `list_recordings`, `replay_market`
- Trading: `trading_status`, `preview_order`, `place_order`, `place_batch_orders`, `get_order`, `list_open_orders`, `list_account_trades`, `cancel_order`, `cancel_all_orders`

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

The websocket service keeps full books inside the process; MCP receives snapshots rather than raw tick streams. A new watch is immediately seeded from CLOB REST and labels that data `rest_seed`. Websocket updates replace it with `websocket` data when connected. Connection state, local receipt time, feed age, errors, and dropped recording updates remain explicit.

SQLite writes run on a dedicated writer thread. Replay is ordered by upstream timestamp and insertion ID, and only claims to replay observations captured by this server.

## Build and verify

Rust 1.88 or newer is required.

```bash
cargo build --release
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --test live_api -- --ignored --nocapture
```

Normal tests are offline. The ignored suite exercises current production Gamma, Data, CLOB REST, wallet, simulation, watch lifecycle, SQLite recording, and replay paths. It never places an order.

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

## Trading safety

Trading is disabled by default. Merely providing a private key does not enable it. To opt in:

```bash
export POLYMARKET_ENABLE_TRADING=true
export POLYMARKET_PRIVATE_KEY=0x...
export POLYMARKET_MAX_ORDER_USDC=100
```

The private key is parsed into a signer and never returned by a tool or logged. Authentication is lazy. Order placement uses a two-step flow:

1. `preview_order` validates type, side, units, exact decimals, and notional policy, then returns a single-use approval that expires after five minutes.
2. `place_order` consumes that approval and also requires `confirm=true`. Batch placement requires `PLACE_BATCH` and applies the cap to the entire batch.

`cancel_order` requires `confirm=true`; `cancel_all_orders` requires the exact phrase `CANCEL_ALL`. Preview and all read-only tools work while trading remains disabled.

This software provides market infrastructure, not trading recommendations or financial advice.
