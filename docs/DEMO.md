# Interview demo

This walkthrough demonstrates the server as a live market-intelligence engine, not merely an API wrapper. It is entirely read-only.

## Setup

Install the release binary, run `polymarket-mcp-rs doctor`, and configure it in an MCP client using the example in the project README. Keep the default `research` profile and use a fresh SQLite path so the recording portion is easy to explain.

## Ninety-second version

1. Ask: “Find the five highest-volume active markets and analyze 100-share execution impact without recommending a trade.”
2. Open one result with `analyze_order_book`; point to exact-decimal spread, microprice, imbalance, and both sample fills.
3. Start `watch_markets`, then show `get_live_snapshot`: Rust holds the full live book while the model sees one compact, timestamped response.
4. End on `server_status`: the default research profile is credential-free and does not expose trading tools.

## Five-minute flow

1. Find an active market.

   Ask the MCP client: “Find the five highest-volume active Bitcoin markets. Show their outcome-token IDs.” This exercises `search_markets` or `list_markets` and gives you a current token ID for the remaining steps.

2. Inspect the market rather than asking for a recommendation.

   Call `analyze_order_book` with the selected token, a depth of 50, a price band of `0.02`, and `100` sample shares. Explain:

   - midpoint is the unweighted center of the best bid and ask;
   - microprice shifts that center using displayed top-level sizes;
   - positive imbalance means more displayed bid size than ask size;
   - near-touch totals show liquidity close to the executable prices;
   - sample impact walks the displayed book and is a simulation, not a fill promise.

   Optionally call `scan_market_microstructure` with a limit of five to show the same exact-decimal analysis applied concurrently across a bounded market set. If a binary complement edge appears, describe it as a gross mechanical check that excludes fees and execution risk, not an arbitrage recommendation.

3. Start the Rust realtime engine.

   Call `watch_markets` for both outcome-token IDs, retain the returned watch ID, then call `get_live_snapshot`. Point out the `rest_seed` to `websocket_snapshot` or `websocket_delta` source transition, upstream versus local timestamps, feed age, reconnect count, and genuine websocket update count.

4. Show compact events.

   Call `get_realtime_events` with the watch ID. This demonstrates why the LLM does not consume every raw tick: Rust maintains the high-frequency state and exposes bounded, sequenced events and compact book snapshots.

5. Record and replay observations.

   Start a recording for the watch, leave it running while discussing the architecture, then stop it and call `replay_market` and `replay_events`. Be precise: replay contains observations captured by this process; it is not claimed to be complete exchange history or an execution backtest.

6. Close cleanly.

   Call `stop_watching`. Finish by showing `server_status`: the default `research` profile does not expose trading tools, and data exploration never requires a wallet. If asked about execution, show `polymarket-mcp-rs --tool-profile all tools` in a terminal and explain that exposure and enablement are separate gates.

## What this demonstrates

- Current CLOB V2 integration rather than legacy request shapes
- Exact decimal and 256-bit identifier handling
- Concurrent websocket state reconstruction with explicit health
- Bounded async-to-thread persistence and deterministic replay
- Typed MCP schemas, structured errors, and one distributable Rust binary
- Server-side tool profiles that remove trading routes by default
- Honest separation between observable market data, simulation, and execution
