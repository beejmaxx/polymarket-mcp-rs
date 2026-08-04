# Contributing

Thanks for helping improve `polymarket-mcp-rs`.

## Development

Rust 1.90 or newer is required. Before opening a pull request, run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

The ignored production suite is read-only and never places orders:

```bash
cargo test --test live_api -- --ignored --nocapture
```

Keep stdout reserved for MCP messages; diagnostics belong on stderr. Use exact decimal and identifier strings at the MCP boundary. New trading operations must remain disabled by default, require explicit confirmation, and include tests that cannot place live orders.

Tool catalog changes must update `tests/contracts/`, [docs/TOOLS.md](docs/TOOLS.md), and the changelog. Treat a hidden profile route as a security boundary: it must be absent from listing and rejected by dispatch.

Protocol behavior should be justified against current Polymarket documentation. Please describe API assumptions and include a fixture or focused test for protocol changes.

## Pull requests

- Keep changes focused and explain user-visible behavior.
- Update tool schemas and documentation together.
- Do not include private keys, API credentials, wallet exports, or production recording databases.
- Report breaking tool-contract changes clearly.
