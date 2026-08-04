# Tool profiles and catalog

Profiles are server-side allowlists. A hidden tool is neither advertised by `tools/list` nor accepted by dispatch.

## Core — 17 tools

- Discovery: `search_markets`, `list_markets`, `get_event`, `get_market`, `compare_markets`
- Market data: `get_order_book`, `analyze_order_book`, `scan_market_microstructure`, `get_price_history`, `get_market_holders`
- Public wallets: `get_wallet_positions`, `get_wallet_value`, `get_wallet_trades`, `get_wallet_activity`, `analyze_wallet_risk`
- Analysis: `simulate_order`
- Operations: `server_status`

## Research — 27 tools, default

Everything in Core, plus:

- Realtime: `watch_markets`, `get_live_snapshot`, `get_realtime_events`, `get_realtime_status`, `stop_watching`
- Local market lab: `start_recording`, `stop_recording`, `list_recordings`, `replay_market`, `replay_events`

## Trading — 29 tools

Everything in Core, plus:

- Safety and approvals: `trading_status`, `preview_order`, `get_order_approval`
- Placement: `place_order`, `place_batch_orders`
- Account reads: `get_order`, `list_open_orders`, `list_account_trades`, `get_balance_allowance`
- Cancellation: `cancel_order`, `cancel_market_orders`, `cancel_all_orders`

This profile excludes realtime recording/replay tools. Use `all` if both groups are needed.

## All — 39 tools

The union of Research and Trading.

## Inspect exact schemas

```bash
polymarket-mcp-rs tools
polymarket-mcp-rs --tool-profile all tools --json
```

All successful tools advertise structured output schemas. Errors use:

```json
{
  "code": "invalid_input",
  "message": "invalid input: ...",
  "retryable": false
}
```

Exact decimals and token/condition identifiers are strings by contract. Timestamps identify their units in field names or tool descriptions.
