# Python replacement matrix

The Rust server targets capability parity, not a one-for-one port of every Python tool name. Overlapping convenience tools are represented by composable filters; opaque recommendations are replaced with inspectable metrics and simulations.

| Python capability | Rust replacement | Status |
|---|---|---|
| Search, trending, featured, category, sports, crypto, closing-soon filters | `search_markets`, `list_markets` filters/sorting | Complete |
| Event markets and details | `get_event` | Complete |
| Market details, price, spread, liquidity, volume | `get_market` | Complete |
| Full current order book | `get_order_book` | Complete |
| Price history | `get_price_history` | Complete, public CLOB endpoint |
| Top holders | `get_market_holders` | Complete, public Data API |
| Compare markets | `compare_markets` | Complete |
| “Opportunity” recommendation | `get_market`, `compare_markets`, `simulate_order` | Replaced with transparent data; no opaque advice |
| Positions and position details | `get_wallet_positions` | Complete |
| Portfolio value and P/L | `get_wallet_value`, `analyze_wallet_risk` | Complete |
| Public trade and activity history | `get_wallet_trades`, `get_wallet_activity` | Complete |
| Suggested portfolio actions | `analyze_wallet_risk` | Descriptive metrics only; no generated advice |
| Price/order-book subscriptions | `watch_markets`, `get_live_snapshot` | Complete local state model |
| Realtime status and unsubscribe | `get_realtime_status`, `stop_watching` | Complete |
| User-order/user-trade websocket subscriptions | `list_open_orders`, `list_account_trades` | Pollable authenticated replacement; websocket push is a follow-up |
| Resolution subscription | `get_market` plus current market websocket infrastructure | Current state available; dedicated push event is a follow-up |
| Limit and market orders | `preview_order`, `place_order` | Complete, approval-gated |
| Batch orders | `place_batch_orders` | Complete, atomic SDK endpoint and batch cap |
| Price suggestion / smart trade | `simulate_order`, `preview_order` | Replaced with deterministic fill math and policy checks |
| Order status and open orders | `get_order`, `list_open_orders` | Complete |
| Authenticated trade/order history | `list_account_trades`, public wallet history | Complete |
| Cancel one / cancel all | `cancel_order`, `cancel_all_orders` | Complete, confirmation-gated |
| Cancel all orders in one market | Use listed order IDs with `cancel_order` | Core capability covered; convenience endpoint is a follow-up |
| Rebalance position | Public positions + previews | Deliberately not autonomous |

Rust-only differentiation is concentrated in persistent concurrent book state, bounded internal fan-out, explicit feed health, dedicated SQLite writing, deterministic replay, exact decimal arithmetic, and a single distributable binary.
