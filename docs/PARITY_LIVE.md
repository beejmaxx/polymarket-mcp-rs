# Live Python replacement audit

The latest local audit was run on 2026-08-05 against the sibling checkout of `caiovicentino/polymarket-mcp-server`. It launched both compiled applications through real stdio MCP transports with credentials removed and mutation disabled.

Result: **PASS — 19 capability groups passed, one Python defect was identified, and zero Rust failures occurred.**

The audit covered all 25 tools advertised by the Python demo profile:

- text search, trending, featured, category, sports, crypto, and closing-soon discovery;
- event expansion, market detail, prices, executable depth, spread, volume, and liquidity;
- bounded public history and holders;
- transparent order-book/fill analysis and multi-market comparison;
- public realtime watch, health, recording, replay, and simulation lifecycle.

The Python realtime group was classified `PYTHON_DEFECT`: every advertised realtime call was routed to a nonexistent `polymarket_mcp.tools.realtime.handle_tool`. The Rust lifecycle completed and cleaned up correctly. The audit also observed the already documented Python order-book truncation and reversed bid/ask semantics; Rust invariants required executable-first sorting and a nonnegative spread.

Run the audit yourself from this repository, with the Python checkout and its `.venv` in the sibling default location:

```bash
cargo build --locked
python3 scripts/parity_harness.py
```

It writes detailed, timestamped JSON and Markdown under `artifacts/`. A `FAIL` means a Rust replacement or invariant failed and produces a nonzero exit code. Authenticated account operations and live trading are intentionally outside this audit.
