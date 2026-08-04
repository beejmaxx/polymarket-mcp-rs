# Launch kit

Use these drafts only after the release links, checksums, MCP Registry entry, and install commands have been verified on clean machines.

## GitHub release summary

Polymarket MCP is a self-contained Rust server with 39 tools for public market discovery, exact CLOB V2 books, wallet analytics, realtime local book reconstruction, SQLite recording/replay, fill simulation, and carefully gated optional trading.

The default credential-free research profile exposes 27 tools and removes all authenticated routes from MCP discovery and dispatch. Release assets cover macOS Intel/Apple Silicon, Linux x86-64/ARM64, and Windows x86-64, with a portable MCP Bundle, SHA-256 checksums, an SPDX SBOM, and GitHub provenance.

## Short announcement

I released Polymarket MCP, an open-source Rust MCP server that goes beyond wrapping REST endpoints: it reconstructs live CLOB books, exposes feed health and exact-decimal microstructure, records/replays local observations, and simulates execution. The default profile needs no key or wallet and hides every trading tool. One binary, 39 tools total, five release targets, and a one-click MCPB.

Repository: https://github.com/beejmaxx/polymarket-mcp-rs

## Technical community post

I wanted a Polymarket MCP server that was useful as infrastructure, not just a long list of thin API wrappers. The Rust process maintains concurrent WebSocket books internally and gives the model compact snapshots, exact-decimal analysis, explicit feed age/errors, deterministic local replay, and current-book fill simulation.

The main design boundary is safety: `research` is the default 27-tool allowlist. Trading routes are absent from `tools/list` and rejected by dispatch unless a user chooses `trading` or `all`; mutation still has a second enablement gate, single-use approvals, caps, and confirmations.

I also published the boring-but-important parts: real compiled-stdio MCP tests, live read-only canaries, golden catalogs, checksum-verified installers, five native binaries, MCPB, SBOM, and provenance. Feedback on tool contracts and real research workflows is welcome.

## Demo script

Record a terminal plus MCP client at readable size:

1. Run `polymarket-mcp-rs doctor` and show every endpoint green.
2. Ask for five high-volume active markets and 100-share execution impact without advice.
3. Open one exact-decimal microstructure result.
4. Start a watch and show WebSocket source, feed age, and update counters.
5. Record briefly, stop, and replay the captured states.
6. Run `polymarket-mcp-rs tools | wc -l`, then show that `place_order` is absent by default.

Keep the finished clip under 90 seconds. Never configure or display a private key.

## Discovery checklist

- GitHub release and repository topics
- Official MCP Registry entry
- crates.io package when a publisher token is available
- Homebrew tap and Scoop bucket after release hashes are final
- Relevant MCP awesome lists through their normal contribution process
- A concise post in communities where the maintainer already participates; do not cross-post identical promotional copy or imply Polymarket endorsement
- A Polymarket developer-community note framed as an open-source integration and request for technical feedback

Track install failures, doctor failures by endpoint, issue count, repeat contributors, stars, and downstream mentions manually. The server contains no telemetry.
