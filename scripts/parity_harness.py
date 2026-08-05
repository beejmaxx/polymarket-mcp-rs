#!/usr/bin/env python3
"""Live, semantic parity audit for the Python and Rust Polymarket MCP servers.

The servers intentionally have different tool names and response contracts. This
harness therefore checks replacement capabilities and market-data invariants, not
byte-for-byte JSON equality. It never enables credentials or places an order.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import re
import shlex
import signal
import sys
import time
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable


PROTOCOL_VERSION = "2025-11-25"


@dataclass
class Check:
    capability: str
    python_tool: str | None
    rust_tools: list[str]
    status: str
    evidence: str
    elapsed_ms: int


class McpFailure(RuntimeError):
    pass


class StdioMcpClient:
    def __init__(
        self,
        name: str,
        command: list[str],
        cwd: Path,
        environment: dict[str, str],
        timeout: float,
    ) -> None:
        self.name = name
        self.command = command
        self.cwd = cwd
        self.environment = environment
        self.timeout = timeout
        self.process: asyncio.subprocess.Process | None = None
        self.next_id = 1
        self.stderr_tail: list[str] = []
        self.stderr_task: asyncio.Task[None] | None = None

    async def start(self) -> None:
        self.process = await asyncio.create_subprocess_exec(
            *self.command,
            cwd=self.cwd,
            env=self.environment,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            start_new_session=True,
            limit=4 * 1024 * 1024,
        )
        self.stderr_task = asyncio.create_task(self._drain_stderr())
        result = await self.request(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "polymarket-parity-harness", "version": "1.0.0"},
            },
        )
        if not result.get("protocolVersion"):
            raise McpFailure(f"{self.name} returned no negotiated protocol version")
        await self.notify("notifications/initialized", {})

    async def _drain_stderr(self) -> None:
        assert self.process and self.process.stderr
        while line := await self.process.stderr.readline():
            message = line.decode(errors="replace").rstrip()
            self.stderr_tail.append(message)
            if len(self.stderr_tail) > 40:
                self.stderr_tail.pop(0)

    async def request(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        request_id = self.next_id
        self.next_id += 1
        await self._write(
            {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
        )
        deadline = asyncio.get_running_loop().time() + self.timeout
        while True:
            remaining = deadline - asyncio.get_running_loop().time()
            if remaining <= 0:
                raise McpFailure(f"{self.name} timed out waiting for {method}")
            message = await asyncio.wait_for(self._read(), timeout=remaining)
            if message.get("id") != request_id:
                continue
            if "error" in message:
                raise McpFailure(f"{self.name} {method}: {message['error']}")
            result = message.get("result")
            if not isinstance(result, dict):
                raise McpFailure(f"{self.name} {method}: invalid result {result!r}")
            return result

    async def notify(self, method: str, params: dict[str, Any]) -> None:
        await self._write({"jsonrpc": "2.0", "method": method, "params": params})

    async def _write(self, message: dict[str, Any]) -> None:
        if not self.process or not self.process.stdin:
            raise McpFailure(f"{self.name} is not running")
        self.process.stdin.write(json.dumps(message, separators=(",", ":")).encode() + b"\n")
        await self.process.stdin.drain()

    async def _read(self) -> dict[str, Any]:
        if not self.process or not self.process.stdout:
            raise McpFailure(f"{self.name} is not running")
        line = await self.process.stdout.readline()
        if not line:
            code = await self.process.wait()
            stderr = "\n".join(self.stderr_tail[-8:])
            raise McpFailure(f"{self.name} exited with {code}; stderr:\n{stderr}")
        try:
            value = json.loads(line)
        except json.JSONDecodeError as error:
            raise McpFailure(f"{self.name} emitted non-JSON stdout: {line!r}") from error
        if not isinstance(value, dict):
            raise McpFailure(f"{self.name} emitted a non-object MCP frame")
        return value

    async def list_tools(self) -> dict[str, dict[str, Any]]:
        result = await self.request("tools/list", {})
        return {tool["name"]: tool for tool in result.get("tools", [])}

    async def call(self, tool: str, arguments: dict[str, Any]) -> tuple[Any, bool, str]:
        result = await self.request("tools/call", {"name": tool, "arguments": arguments})
        is_error = bool(result.get("isError", False))
        if "structuredContent" in result:
            value = result["structuredContent"]
        else:
            texts = [
                item.get("text", "")
                for item in result.get("content", [])
                if item.get("type") == "text"
            ]
            text = "\n".join(texts)
            try:
                value = json.loads(text)
            except (json.JSONDecodeError, TypeError):
                value = text
        detail = compact(value)
        if isinstance(value, dict) and value.get("success") is False:
            is_error = True
        if isinstance(value, str) and re.search(
            r"(^|\b)(error|failed|authentication required|unavailable)(\b|:)",
            value,
            re.IGNORECASE,
        ):
            is_error = True
        return value, is_error, detail

    async def close(self) -> None:
        if not self.process:
            return
        if self.process.stdin:
            self.process.stdin.close()
        try:
            await asyncio.wait_for(self.process.wait(), timeout=4)
        except asyncio.TimeoutError:
            try:
                os.killpg(self.process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                await asyncio.wait_for(self.process.wait(), timeout=4)
            except asyncio.TimeoutError:
                try:
                    os.killpg(self.process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                await self.process.wait()
        if self.stderr_task:
            await self.stderr_task


def compact(value: Any, limit: int = 240) -> str:
    text = json.dumps(value, sort_keys=True, ensure_ascii=False) if not isinstance(value, str) else value
    text = " ".join(text.split())
    return text if len(text) <= limit else f"{text[: limit - 1]}…"


def count_items(value: Any) -> int:
    if isinstance(value, list):
        return len(value)
    if isinstance(value, dict):
        for key in ("markets", "events", "positions", "holders", "points", "data"):
            if isinstance(value.get(key), list):
                return len(value[key])
        if isinstance(value.get("count"), int):
            return value["count"]
    return 0


def require(condition: bool, message: str) -> None:
    if not condition:
        raise McpFailure(message)


class Audit:
    def __init__(self, python: StdioMcpClient, rust: StdioMcpClient) -> None:
        self.python = python
        self.rust = rust
        self.checks: list[Check] = []
        self.python_tools: dict[str, dict[str, Any]] = {}
        self.rust_tools: dict[str, dict[str, Any]] = {}
        self.seed: dict[str, Any] = {}

    async def catalog(self) -> None:
        self.python_tools, self.rust_tools = await asyncio.gather(
            self.python.list_tools(), self.rust.list_tools()
        )
        mappings = {
            "search_markets": ["search_markets"],
            "get_trending_markets": ["list_markets"],
            "filter_markets_by_category": ["list_markets"],
            "get_event_markets": ["get_event"],
            "get_featured_markets": ["list_markets"],
            "get_closing_soon_markets": ["list_markets"],
            "get_sports_markets": ["list_markets"],
            "get_crypto_markets": ["list_markets"],
            "get_market_details": ["get_market"],
            "get_current_price": ["get_order_book"],
            "get_orderbook": ["get_order_book"],
            "get_spread": ["get_order_book"],
            "get_market_volume": ["get_market"],
            "get_liquidity": ["get_market"],
            "get_price_history": ["get_price_history"],
            "get_market_holders": ["get_market_holders"],
            "analyze_market_opportunity": ["analyze_order_book", "simulate_order"],
            "compare_markets": ["compare_markets"],
            "subscribe_market_prices": ["watch_markets", "get_realtime_events"],
            "subscribe_orderbook_updates": ["watch_markets", "get_live_snapshot"],
            "subscribe_user_orders": ["watch_user_events", "get_user_events"],
            "subscribe_user_trades": ["watch_user_events", "get_user_events"],
            "subscribe_market_resolution": ["watch_markets", "get_realtime_events"],
            "get_realtime_status": ["get_realtime_status"],
            "unsubscribe_realtime": ["stop_watching", "stop_user_watch"],
        }
        started = time.monotonic()
        missing = {
            name: tools
            for name, tools in mappings.items()
            if name in self.python_tools and not all(tool in self.rust_tools for tool in tools)
        }
        unexpected = sorted(set(self.python_tools) - set(mappings))
        status = "PASS" if not missing and not unexpected else "FAIL"
        self.checks.append(
            Check(
                "Complete Python demo-tool catalog mapping",
                None,
                sorted({tool for tools in mappings.values() for tool in tools}),
                status,
                f"Python={len(self.python_tools)} Rust={len(self.rust_tools)} "
                f"missing={missing or 'none'} unmapped={unexpected or 'none'}",
                round((time.monotonic() - started) * 1000),
            )
        )
        if status == "FAIL":
            raise McpFailure(self.checks[-1].evidence)

    async def bootstrap(self) -> None:
        search, is_error, detail = await self.rust.call("search_markets", {"query": "bitcoin", "limit": 10})
        require(not is_error and count_items(search) > 0, f"Rust search bootstrap failed: {detail}")
        markets = search.get("markets", [])
        selected = None
        selected_detail = None
        for candidate in markets:
            value, failed, _ = await self.rust.call("get_market", {"market_id": candidate["market_id"]})
            if not failed and any(outcome.get("token_id") for outcome in value.get("outcomes", [])):
                selected = candidate
                selected_detail = value
                break
        require(selected is not None and selected_detail is not None, "no active market with tokens found")
        token = next(
            outcome["token_id"]
            for outcome in selected_detail["outcomes"]
            if outcome.get("token_id")
        )
        market_ids = [market["market_id"] for market in markets[:3]]
        require(len(market_ids) >= 2, "need two markets for comparison")
        self.seed = {
            "market_id": selected["market_id"],
            "market_ids": market_ids,
            "event_id": selected["event_id"],
            "event_slug": selected.get("event_slug"),
            "market_slug": selected.get("slug"),
            "condition_id": selected_detail.get("condition_id"),
            "token_id": token,
        }

    async def pair(
        self,
        capability: str,
        python_tool: str,
        python_args: dict[str, Any],
        rust_calls: list[tuple[str, dict[str, Any]]],
        validate_rust: Callable[[list[Any]], None] | None = None,
        allow_empty_python: bool = True,
    ) -> None:
        started = time.monotonic()
        python_value: Any = None
        python_error = False
        python_detail = "not called"
        rust_values: list[Any] = []
        rust_details: list[str] = []
        rust_error = False
        try:
            python_value, python_error, python_detail = await self.python.call(python_tool, python_args)
        except Exception as error:  # audit must preserve the other server's result
            python_error = True
            python_detail = str(error)
        for rust_tool, arguments in rust_calls:
            try:
                value, failed, detail = await self.rust.call(rust_tool, arguments)
                rust_values.append(value)
                rust_details.append(f"{rust_tool}: {detail}")
                rust_error = rust_error or failed
            except Exception as error:
                rust_error = True
                rust_details.append(f"{rust_tool}: {error}")
        try:
            if validate_rust and not rust_error:
                validate_rust(rust_values)
        except Exception as error:
            rust_error = True
            rust_details.append(f"invariant: {error}")
        if not allow_empty_python and not python_error and count_items(python_value) == 0:
            python_error = True
            python_detail = f"empty result: {python_detail}"
        if rust_error:
            status = "FAIL"
        elif python_error:
            status = "PYTHON_DEFECT"
        else:
            status = "PASS"
        evidence = f"Python: {python_detail} | Rust: {'; '.join(rust_details)}"
        self.checks.append(
            Check(
                capability,
                python_tool,
                [name for name, _ in rust_calls],
                status,
                evidence,
                round((time.monotonic() - started) * 1000),
            )
        )

    async def discovery(self) -> None:
        closing_before = datetime.fromtimestamp(
            time.time() + 168 * 3600, timezone.utc
        ).isoformat()
        await self.pair(
            "Text market search",
            "search_markets",
            {"query": "bitcoin", "limit": 5},
            [("search_markets", {"query": "bitcoin", "limit": 5})],
            lambda values: require(count_items(values[0]) > 0, "empty search"),
            allow_empty_python=False,
        )
        await self.pair(
            "Highest-volume active markets",
            "get_trending_markets",
            {"timeframe": "24h", "limit": 5},
            [("list_markets", {"limit": 5, "sort_by": "volume_24h", "ascending": False})],
            lambda values: require(count_items(values[0]) > 0, "empty ranking"),
            allow_empty_python=False,
        )
        await self.pair(
            "Category filtering",
            "filter_markets_by_category",
            {"category": "Crypto", "limit": 5},
            [("list_markets", {"tag_slug": "crypto", "limit": 5})],
        )
        await self.pair(
            "Event market expansion",
            "get_event_markets",
            {"event_id": self.seed["event_id"]},
            [("get_event", {"event_id": self.seed["event_id"]})],
            lambda values: require(count_items(values[0]) > 0, "event has no markets"),
        )
        await self.pair(
            "Featured markets",
            "get_featured_markets",
            {"limit": 5},
            [("list_markets", {"featured": True, "limit": 5})],
        )
        await self.pair(
            "Closing-soon markets",
            "get_closing_soon_markets",
            {"hours": 168, "limit": 5},
            [("list_markets", {"end_before": closing_before, "limit": 5})],
        )
        await self.pair(
            "Sports markets",
            "get_sports_markets",
            {"limit": 5},
            [("list_markets", {"tag_slug": "sports", "limit": 5})],
        )
        await self.pair(
            "Crypto markets",
            "get_crypto_markets",
            {"symbol": "BTC", "limit": 5},
            [("search_markets", {"query": "bitcoin", "limit": 5})],
            lambda values: require(count_items(values[0]) > 0, "empty crypto search"),
        )

    async def analysis(self) -> None:
        market_id = self.seed["market_id"]
        token_id = self.seed["token_id"]
        condition_id = self.seed["condition_id"]
        await self.pair(
            "Market detail",
            "get_market_details",
            {"market_id": market_id},
            [("get_market", {"market_id": market_id})],
            lambda values: require(values[0].get("market_id") == market_id, "market ID changed"),
        )
        for capability, python_tool in (
            ("Current executable prices", "get_current_price"),
            ("Executable order-book depth", "get_orderbook"),
            ("Spread and midpoint", "get_spread"),
        ):
            python_args = {"token_id": token_id}
            if python_tool == "get_orderbook":
                python_args["depth"] = 5
            await self.pair(
                capability,
                python_tool,
                python_args,
                [("get_order_book", {"token_id": token_id, "depth": 5})],
                validate_book,
            )
        await self.pair(
            "Volume statistics",
            "get_market_volume",
            {"market_id": market_id},
            [("get_market", {"market_id": market_id})],
        )
        await self.pair(
            "Liquidity",
            "get_liquidity",
            {"market_id": market_id},
            [("get_market", {"market_id": market_id})],
        )
        await self.pair(
            "Bounded price history",
            "get_price_history",
            {"token_id": token_id, "resolution": "1h"},
            [("get_price_history", {"token_id": token_id, "interval": "1d", "limit": 100})],
            lambda values: require(count_items(values[0]) > 0, "empty history"),
        )
        if condition_id:
            await self.pair(
                "Top public holders",
                "get_market_holders",
                {"market_id": market_id, "limit": 5},
                [("get_market_holders", {"condition_id": condition_id, "limit": 5})],
            )
        await self.pair(
            "Transparent opportunity analysis replacement",
            "analyze_market_opportunity",
            {"market_id": market_id},
            [
                ("analyze_order_book", {"token_id": token_id, "sample_shares": "25"}),
                ("simulate_order", {"token_id": token_id, "side": "buy", "shares": "25"}),
            ],
            lambda values: require(
                values[0].get("sample_shares") == "25" and values[1].get("requested_shares") == "25",
                "sample execution contract mismatch",
            ),
        )
        await self.pair(
            "Multi-market comparison",
            "compare_markets",
            {"market_ids": self.seed["market_ids"][:2]},
            [("compare_markets", {"market_ids": self.seed["market_ids"][:2]})],
            lambda values: require(count_items(values[0]) == 2, "comparison did not return two markets"),
        )

    async def realtime(self) -> None:
        token_id = self.seed["token_id"]
        condition_id = self.seed["condition_id"] or self.seed["market_id"]
        started = time.monotonic()
        watch, watch_error, watch_detail = await self.rust.call(
            "watch_markets", {"token_ids": [token_id]}
        )
        if watch_error:
            self.checks.append(Check("Realtime lifecycle", None, ["watch_markets"], "FAIL", watch_detail, 0))
            return
        watch_id = watch["watch_id"]
        try:
            python_calls = [
                ("subscribe_market_prices", {"market_ids": [condition_id]}),
                ("subscribe_orderbook_updates", {"token_ids": [token_id], "depth": 5}),
                ("subscribe_market_resolution", {"market_ids": [condition_id]}),
                ("get_realtime_status", {}),
                ("subscribe_user_orders", {}),
                ("subscribe_user_trades", {}),
                ("unsubscribe_realtime", {"subscription_id": "parity-nonexistent"}),
            ]
            python_results = []
            python_defects = []
            for tool, arguments in python_calls:
                value, failed, detail = await self.python.call(tool, arguments)
                python_results.append(f"{tool}: {detail}")
                if failed and tool not in {"subscribe_user_orders", "subscribe_user_trades"}:
                    python_defects.append(tool)
            snapshot, snapshot_error, snapshot_detail = await self.rust.call(
                "get_live_snapshot", {"watch_id": watch_id, "depth": 5}
            )
            status, status_error, status_detail = await self.rust.call("get_realtime_status", {})
            events, events_error, events_detail = await self.rust.call(
                "get_realtime_events", {"watch_id": watch_id, "after_sequence": 0, "limit": 20}
            )
            require(not snapshot_error and snapshot.get("books"), "Rust realtime snapshot is empty")
            require(not status_error and status.get("active_watch_count") == 1, "Rust watch status missing")
            require(not events_error, "Rust realtime events failed")
            recording, recording_error, _ = await self.rust.call(
                "start_recording", {"watch_id": watch_id, "label": "parity audit"}
            )
            require(not recording_error, "Rust start_recording failed")
            stopped, stop_error, _ = await self.rust.call(
                "stop_recording", {"recording_id": recording["recording_id"]}
            )
            require(not stop_error and stopped.get("snapshot_count", 0) >= 1, "recording has no snapshot")
            replay, replay_error, _ = await self.rust.call(
                "replay_market", {"recording_id": recording["recording_id"], "limit": 10}
            )
            require(not replay_error and replay.get("books"), "Rust replay is empty")
            simulation, simulation_error, _ = await self.rust.call(
                "simulate_order", {"token_id": token_id, "side": "buy", "shares": "1"}
            )
            require(not simulation_error and simulation.get("requested_shares") == "1", "simulation failed")
            status_name = "PYTHON_DEFECT" if python_defects else "PASS"
            evidence = (
                f"Python defects={python_defects or 'none'}; "
                f"Rust snapshot={snapshot_detail}; status={status_detail}; events={events_detail}; "
                f"Python calls={' | '.join(python_results)}"
            )
            self.checks.append(
                Check(
                    "Public realtime watch, health, recording, replay, and simulation lifecycle",
                    "subscribe_* / get_realtime_status / unsubscribe_realtime",
                    [
                        "watch_markets",
                        "get_live_snapshot",
                        "get_realtime_events",
                        "get_realtime_status",
                        "start_recording",
                        "stop_recording",
                        "replay_market",
                        "simulate_order",
                    ],
                    status_name,
                    evidence,
                    round((time.monotonic() - started) * 1000),
                )
            )
        except Exception as error:
            self.checks.append(
                Check(
                    "Public realtime watch, health, recording, replay, and simulation lifecycle",
                    "subscribe_* / get_realtime_status / unsubscribe_realtime",
                    ["watch_markets", "get_live_snapshot", "start_recording", "replay_market"],
                    "FAIL",
                    str(error),
                    round((time.monotonic() - started) * 1000),
                )
            )
        finally:
            await self.rust.call("stop_watching", {"watch_id": watch_id})


def validate_book(values: list[Any]) -> None:
    book = values[0]
    summary = book.get("summary", {})
    bids = book.get("bids", [])
    asks = book.get("asks", [])
    bid_prices = [float(level["price"]) for level in bids]
    ask_prices = [float(level["price"]) for level in asks]
    require(bid_prices == sorted(bid_prices, reverse=True), "bids are not executable-first")
    require(ask_prices == sorted(ask_prices), "asks are not executable-first")
    if summary.get("best_bid") and summary.get("best_ask"):
        require(float(summary["best_bid"]) <= float(summary["best_ask"]), "negative/crossed spread")


def markdown_report(payload: dict[str, Any]) -> str:
    lines = [
        "# Live Python replacement audit",
        "",
        f"Generated: `{payload['generated_at']}`",
        "",
        "This audit launches both implementations through their real stdio MCP transports. "
        "It compares semantic capabilities and invariants because the Rust server intentionally "
        "uses typed, stable contracts instead of copying the Python response shapes.",
        "",
        f"Overall: **{payload['summary']['overall']}** — "
        f"{payload['summary']['pass']} pass, {payload['summary']['python_defect']} Python defects, "
        f"{payload['summary']['fail']} Rust failures.",
        "",
        "| Capability | Python tool | Rust replacement | Result | Time |",
        "|---|---|---|---:|---:|",
    ]
    for check in payload["checks"]:
        lines.append(
            f"| {check['capability']} | {check['python_tool'] or 'catalog'} | "
            f"{', '.join(check['rust_tools'])} | **{check['status']}** | {check['elapsed_ms']} ms |"
        )
    lines.extend(["", "## Evidence", ""])
    for check in payload["checks"]:
        lines.extend([f"### {check['capability']} — {check['status']}", "", check["evidence"], ""])
    lines.extend(
        [
            "## Interpretation",
            "",
            "- `PASS`: both servers completed the mapped capability and Rust invariants passed.",
            "- `PYTHON_DEFECT`: the Rust replacement passed while the Python call failed or returned an error.",
            "- `FAIL`: a Rust replacement or invariant failed; the audit exits non-zero.",
            "- Authenticated account and trading operations are intentionally outside this report.",
            "",
        ]
    )
    return "\n".join(lines)


def parse_args() -> argparse.Namespace:
    root = Path(__file__).resolve().parents[1]
    python_root = root.parent / "polymarket-mcp-server"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust-root", type=Path, default=root)
    parser.add_argument("--python-root", type=Path, default=python_root)
    parser.add_argument(
        "--rust-command",
        default=str(root / "target" / "debug" / "polymarket-mcp-rs") + " --tool-profile all",
    )
    parser.add_argument(
        "--python-command",
        default=str(python_root / ".venv" / "bin" / "polymarket-mcp"),
    )
    parser.add_argument("--timeout", type=float, default=45.0)
    parser.add_argument("--output", type=Path, default=root / "artifacts" / "parity-report.json")
    parser.add_argument("--markdown", type=Path, default=root / "artifacts" / "parity-report.md")
    return parser.parse_args()


async def run(args: argparse.Namespace) -> int:
    environment = os.environ.copy()
    for key in (
        "POLYMARKET_PRIVATE_KEY",
        "POLYMARKET_API_KEY",
        "POLYMARKET_SECRET",
        "POLYMARKET_PASSPHRASE",
        "POLYMARKET_ENABLE_TRADING",
    ):
        environment.pop(key, None)
    environment["DEMO_MODE"] = "true"
    # `all` makes authenticated replacement schemas visible for catalog parity,
    # while the absent mutation gate and scrubbed credentials keep the run inert.
    environment["POLYMARKET_TOOL_PROFILE"] = "all"
    rust = StdioMcpClient(
        "Rust",
        shlex.split(args.rust_command),
        args.rust_root,
        environment,
        args.timeout,
    )
    python = StdioMcpClient(
        "Python",
        shlex.split(args.python_command),
        args.python_root,
        environment,
        args.timeout,
    )
    try:
        await asyncio.gather(rust.start(), python.start())
        audit = Audit(python, rust)
        await audit.catalog()
        await audit.bootstrap()
        await audit.discovery()
        await audit.analysis()
        await audit.realtime()
        failures = sum(check.status == "FAIL" for check in audit.checks)
        payload = {
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "protocol_version": PROTOCOL_VERSION,
            "commands": {"python": args.python_command, "rust": args.rust_command},
            "seed": audit.seed,
            "summary": {
                "overall": "PASS" if failures == 0 else "FAIL",
                "pass": sum(check.status == "PASS" for check in audit.checks),
                "python_defect": sum(check.status == "PYTHON_DEFECT" for check in audit.checks),
                "fail": failures,
            },
            "checks": [asdict(check) for check in audit.checks],
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.markdown.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(payload, indent=2) + "\n")
        args.markdown.write_text(markdown_report(payload))
        print(json.dumps(payload["summary"], sort_keys=True))
        print(f"JSON: {args.output}")
        print(f"Markdown: {args.markdown}")
        return 1 if failures else 0
    finally:
        await asyncio.gather(python.close(), rust.close())


def main() -> None:
    try:
        raise SystemExit(asyncio.run(run(parse_args())))
    except KeyboardInterrupt:
        raise SystemExit(130) from None


if __name__ == "__main__":
    main()
