# Threat model

## Assets

- private signing key and derived authenticated API credentials;
- ability to place or cancel orders;
- local SQLite recordings and approval audit data;
- integrity of market data presented to an MCP model;
- integrity of downloaded executables and bundles.

## Trust boundaries

The MCP client can call every tool exposed by the selected profile. Polymarket's Gamma, Data, CLOB REST, and CLOB WebSocket services are external systems. The local process environment and database path are controlled by the user. A model's prose is not a trusted authorization mechanism.

## Controls

- The default profile removes every authenticated and trading route from discovery and dispatch.
- Exposing trading tools does not enable order mutation. Mutation separately requires `POLYMARKET_ENABLE_TRADING=true`.
- The private key is loaded from the environment, parsed into a signer, and never returned by a tool or intentionally logged.
- Placement uses a fresh single-use approval, a five-minute expiry, notional caps, live market-rule checks by default, and a separate confirmation call.
- Batch and cancellation operations require exact confirmation values. Approval transitions are written before submission.
- Ambiguous submission failures remain `unknown`; they are not silently retried.
- Authenticated clients use heartbeats, and placement checks geographic eligibility.
- Exact decimals and identifiers avoid JSON precision corruption.
- Recorder backpressure and decode failures are surfaced; replay does not claim completeness.
- Release artifacts have checksums, SBOMs, and provenance attestations.

## Known limitations

- Any local process running as the same OS user may be able to read environment variables, process memory, or the SQLite database. Use a dedicated low-value wallet and a hardened machine for live trading.
- An MCP client can call exposed tools according to its own approval policy. Do not expose the `trading` or `all` profile to a client you do not trust.
- Upstream APIs, DNS, TLS roots, proxies, and the network can fail or return stale/inconsistent data. Feed age, source, and errors are exposed, but the server cannot prove real-world event truth.
- A market order or aggressive limit order can fill immediately. Preview is policy validation, not a profit or execution guarantee.
- GitHub-hosted binaries are not currently platform code-signed or Apple-notarized. Verify SHA-256 checksums and GitHub attestations.
- The server is designed for local stdio. It does not add authentication suitable for an Internet-facing deployment.

## Recommended deployment

Use the `research` profile for normal use. If trading is required, create a dedicated wallet, keep the order cap low, protect the database, avoid shell history for secrets, and keep confirmations enabled in the MCP host. Stop the server and rotate the key if credential exposure is suspected.
