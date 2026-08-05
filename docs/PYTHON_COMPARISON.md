# Python server comparison

This is a dated engineering comparison against the local checkout of
[`caiovicentino/polymarket-mcp-server`](https://github.com/caiovicentino/polymarket-mcp-server),
not a claim that every Python implementation has the same behavior.

## Test method

On 2026-08-04, both compiled servers were launched as child processes and exercised through their real stdio MCP transports. Both negotiated MCP protocol `2025-11-25`. The Python server ran in demo mode with no credentials; the Rust server ran read-only with its complete tool catalog. The current Rust default is the narrower `research` profile. The comparison called discovery, market detail, and order-book tools, then repeated the book and spread checks against the same live market and outcome token.

On 2026-08-05, the repeatable `scripts/parity_harness.py` audit mapped all 25 Python demo tools, exercised 20 semantic capability groups through both real MCP transports, and finished with 19 passes, one Python implementation defect, and zero Rust failures. The defect was the Python server advertising realtime tools whose dispatcher calls a nonexistent `realtime.handle_tool`. Authenticated trading was excluded from both runs.

The Rust repository also contains automated tests for its compiled stdio binary. The ignored production suite calls both `search_markets` and `scan_market_microstructure` through MCP rather than bypassing the protocol layer. See [the live audit summary](PARITY_LIVE.md) and rerun the harness to generate full JSON/Markdown evidence locally.

## Observed results

| Area | Python server | Rust server |
|---|---|---|
| Tool listing | 25 tools in demo mode | 11 hosted, 30 default research, and 46 in `all`; mutation remains separately disabled unless configured |
| Successful result shape | JSON encoded inside text content | Typed `structuredContent` plus generated input/output schemas |
| Tool failures | JSON `{success: false}` returned as ordinary text content | MCP `isError=true` with stable `code`, message, and retryability |
| Financial values | JSON floating-point numbers | Exact decimal strings |
| Search response | Raw Gamma market objects, 81 fields in the sample | Stable 12-field market summaries with parsed outcome/token mappings |
| Market detail | Raw Gamma object, 82 fields in the sample | Stable 16-field domain response with current outcome books |
| Order-book depth | Raw arrays sliced before price normalization | Bids sorted high-to-low and asks low-to-high before truncation |
| Book metadata | Local response-construction timestamp | Upstream timestamp, hash, tick size, minimum order size, last trade, level counts, spread, and midpoint |
| Derived analysis | Separate price/spread and heuristic recommendation tools | Inspectable microprice, imbalance, depth, near-touch liquidity, and fill-impact calculations |
| Realtime persistence | In-memory subscriptions | Reconstructed local books plus SQLite book/event recording and deterministic replay |
| Answer-ready response | Requires several raw/convenience calls and model-side joining | Joined market/wallet research with source URLs, timestamps, and limitations |
| Hosted clients | Local stdio surface | Compact credential-blind profile, Streamable HTTP, operations endpoints, and optional portable MCP Apps card |

## Same-token correctness check

The shared sample was market `3158254`, “Will the price of Bitcoin be above $62,000 on August 4?”, using outcome token `110265694267936992469417700335614533467196469203686817149366068640963818137254`.

The Python `get_orderbook(depth=5)` response began bids at `0.001, 0.002, 0.003, ...` and asks at `0.999, 0.998, 0.997, ...`. Its implementation slices the upstream arrays before sorting, so a small depth returns levels far from the executable touch. Its `get_spread` response for the same token was:

```json
{
  "bid": 0.996,
  "ask": 0.994,
  "spread_value": -0.0020000000000000018
}
```

The negative spread is not a crossed market. The Python implementation assigns the CLOB `BUY` price to `ask` and `SELL` price to `bid`, reversing the endpoint semantics.

The Rust response, a few seconds later, returned best-price-first depth and exact values:

```json
{
  "best_bid": "0.994",
  "best_ask": "0.996",
  "spread": "0.002",
  "midpoint": "0.995"
}
```

The Rust `analyze_order_book` result tied its calculations to the same upstream book hash and timestamp and showed a 25-share buy at `0.996` and sell at `0.994`, with zero modeled slippage because both samples fit at the top level.

## Tradeoffs and limits

- The Python raw Gamma response exposes more upstream fields immediately. The Rust contract intentionally exposes fewer stable fields; adding a field requires an explicit schema decision.
- The servers chose different first search results because Rust sorts matches by trailing 24-hour volume while Python preserves the public-search response order. The same-token check avoids treating that ranking choice as a data discrepancy.
- Direct HTTP and stdio MCP production calls pass without a proxy. This machine's direct outbound market WebSocket remained in `Connecting`; the same test passes through the server's optional SOCKS5 setting. That routing limitation is environment-specific, and a proxy is not a normal deployment requirement.
- Live books can change between sequential calls. The comparison uses executable ordering and sign invariants, and records timestamps, rather than expecting sizes to match tick-for-tick.
- Authenticated trading was intentionally excluded. No `.env` credentials were loaded or used.

## Conclusion

The Rust server replaces the Python server's useful non-trading surface while improving the MCP contract, numeric fidelity, book correctness, source attribution, process packaging, hosted-client boundary, and realtime recording model. The strongest differentiator is not the language itself: it is that the Rust server turns volatile upstream payloads into explicit, testable market-data semantics.
