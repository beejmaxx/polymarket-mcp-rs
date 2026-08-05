# Python replacement matrix

The Rust server targets capability parity, not a one-for-one port of every Python tool name. Overlapping convenience tools are represented by composable filters; opaque recommendations are replaced with inspectable metrics and simulations.

| Python capability | Rust replacement | Status |
|---|---|---|
| Search, trending, featured, category, sports, crypto, closing-soon filters | `search_markets`, `list_markets` filters/sorting | Complete; market-level ranking and pagination |
| Event markets and details | `get_event` | Complete |
| Answer-ready market explanation | `get_market_brief` | Rust extension; joins metadata, books, history, fill impact, sources, timestamps, and limitations |
| Market details, price, spread, liquidity, volume | `get_market`, `analyze_order_book`, `scan_market_microstructure` | Complete, with inspectable single- and multi-market L2 metrics |
| Full current order book | `get_order_book` | Complete |
| Price history | `get_price_history` | Complete, public CLOB endpoint |
| Top holders | `get_market_holders` | Complete, public Data API |
| Compare markets | `compare_markets` | Complete |
| “Opportunity” recommendation | `get_market`, `compare_markets`, `simulate_order` | Replaced with transparent data; no opaque advice |
| Positions and position details | `get_wallet_positions` | Complete |
| Portfolio value and P/L | `get_wallet_value`, `analyze_wallet_risk` | Complete |
| Public trade and activity history | `get_wallet_trades`, `get_wallet_activity` | Complete |
| Joined public wallet research | `get_wallet_summary` | Rust extension; positions, value, risk, recent trades/activity, source, and limitations |
| Suggested portfolio actions | `analyze_wallet_risk` | Descriptive metrics only; no generated advice |
| Price/order-book subscriptions | `watch_markets`, `get_live_snapshot`, `get_realtime_events` | Complete snapshot-plus-delta local state model |
| Realtime status and unsubscribe | `get_realtime_status`, `stop_watching` | Complete |
| User-order/user-trade websocket subscriptions | `watch_user_events`, `get_user_events`, `get_user_realtime_status`, `stop_user_watch` | Complete buffered authenticated WebSocket workflow; production credentials still require operator verification |
| Resolution subscription | `get_realtime_events` | Sequenced pollable lifecycle events; MCP push is a follow-up |
| Limit and market orders | `preview_order`, `place_order` | Complete, approval-gated |
| Batch orders | `place_batch_orders` | Complete, atomic SDK endpoint and batch cap |
| Price suggestion / smart trade | `simulate_order`, `preview_order` | Replaced with deterministic fill math and policy checks |
| Order status and open orders | `get_order`, `list_open_orders` | Complete |
| Authenticated trade/order history | `list_account_trades`, public wallet history | Complete with cursor pagination |
| Cancel one / cancel all | `cancel_order`, `cancel_all_orders` | Complete, confirmation-gated |
| Cancel all orders in one market | `cancel_market_orders` | Complete, confirmation-gated |
| Rebalance position | Public positions + previews | Deliberately not autonomous |
| Explicit negative-risk event basket check | `analyze_event_consistency` | Rust extension; refuses to infer relationships from titles alone |

Current Polymarket wallet modes are supported through configurable EOA, Proxy, Gnosis Safe, or POLY_1271 signatures and an optional/required funder address. Authenticated balance and allowance inspection is exposed through `get_balance_allowance`.

Rust-only differentiation is concentrated in persistent concurrent book state, bounded multi-market scans, explicit feed health, dedicated SQLite book/event writing, deterministic replay, exact decimal arithmetic, generated MCP schemas, answer-ready source-linked outputs, a credential-blind hosted profile, and a single distributable binary.

The executable audit in `scripts/parity_harness.py` covers every Python demo tool through both real stdio MCP transports. It treats response-shape differences as intentional and checks semantic invariants such as executable-first book ordering, nonnegative spreads, bounded output, lifecycle cleanup, and replacement availability.
