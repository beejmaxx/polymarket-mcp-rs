# Migrating from the Python server

The Rust server is a capability replacement, not a tool-name-compatible fork. Start with the default `research` profile; it needs no credentials and includes public data, live books, recording, replay, and simulation.

## Common mappings

| Python workflow | Rust workflow |
|---|---|
| Search or browse markets | `search_markets` or `list_markets` |
| Trending, featured, category, or closing-soon lists | `list_markets` with sort, tag, featured, liquidity/volume, and end-time filters |
| Market price and spread | `get_market`, `get_order_book`, or `analyze_order_book` |
| Compare candidates | `compare_markets` or `scan_market_microstructure` |
| Subscribe to prices/books | `watch_markets`, then poll `get_live_snapshot` or `get_realtime_events` |
| Portfolio, trades, and activity | `get_wallet_positions`, `get_wallet_value`, `get_wallet_trades`, `get_wallet_activity` |
| User order/fill stream | `watch_user_events`, then `get_user_events` |
| Place an order | `preview_order`, then `place_order` with explicit confirmation |

## Contract differences

- Successful results use MCP `structuredContent`, not JSON embedded in a text string.
- Decimal values and 256-bit identifiers are JSON strings to preserve precision.
- Parameterized tools reject unknown fields instead of silently ignoring typos.
- Order-book depth is sorted into executable order before truncation.
- History and event responses are bounded; use sequence cursors for incremental reads.
- Recorded replay contains observations captured by this process, not complete exchange history.
- Current collateral terminology is pUSD. `POLYMARKET_MAX_ORDER_PUSD` is preferred; the old `POLYMARKET_MAX_ORDER_USDC` name remains a temporary environment-variable alias.

## Enabling account tools

Set `POLYMARKET_TOOL_PROFILE=trading` or `all` and configure `POLYMARKET_PRIVATE_KEY`. This exposes authenticated reads and user-event watches. It does not enable order mutation. Mutation additionally requires `POLYMARKET_ENABLE_TRADING=true`, local caps, a preview approval, and operation-specific confirmation.

Use a separate low-value wallet for testing. Authenticated placement has not been exercised by the public test suite because the repository never ships or loads maintainer credentials.

## Database location

The default database now lives in the operating system's application-data directory instead of the process working directory. If an earlier Rust checkout created `polymarket-mcp.sqlite3` beside the executable, keep using it by setting `POLYMARKET_MCP_DB` to that file's absolute path, or move it to the new default location while the server is stopped. The server never moves or deletes an existing database automatically.
