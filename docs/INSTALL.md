# Installation and client setup

## Recommended: release binary

The installer selects the current OS and CPU architecture, downloads the matching GitHub release archive, verifies its SHA-256 checksum, and installs one executable.

macOS and Linux:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/beejmaxx/polymarket-mcp-rs/main/scripts/install.sh | sh
```

The default destination is `~/.local/bin`. Override it with `POLYMARKET_MCP_INSTALL_DIR`, or pin a release with `POLYMARKET_MCP_VERSION=0.1.1`.

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/beejmaxx/polymarket-mcp-rs/main/scripts/install.ps1 | iex
```

The PowerShell installer verifies the archive and adds `%USERPROFILE%\.local\bin` to the user PATH. You can instead download and inspect either installer before running it.

## Package managers

Homebrew on macOS or Linux:

```bash
brew install beejmaxx/tap/polymarket-mcp-rs
```

Scoop on Windows:

```powershell
scoop bucket add beejmaxx https://github.com/beejmaxx/scoop-bucket
scoop install beejmaxx/polymarket-mcp-rs
```

Both repositories use the same versioned release archives and SHA-256 hashes as
the installers. The release also includes validated WinGet manifests for users
who want to submit or install them locally; an official WinGet Community listing
requires a separate upstream review.

Validate the installation:

```bash
polymarket-mcp-rs --version
polymarket-mcp-rs doctor
```

`doctor --offline` checks configuration and SQLite without network access. `doctor --json` is suitable for support logs and automation; it never prints credentials.

## One-click MCP Bundle

Download `polymarket-mcp.mcpb` from the latest release and open it in an MCPB-compatible client. The bundle contains all supported platform binaries and defaults to the credential-free `research` profile.

The bundle is large because it is portable across macOS Intel/Apple Silicon, Linux x86-64/ARM64, and Windows x86-64. Standalone archives remain the smallest install.

## Build from source

```bash
git clone https://github.com/beejmaxx/polymarket-mcp-rs.git
cd polymarket-mcp-rs
cargo install --path . --locked
```

Or install the current main branch directly:

```bash
cargo install --git https://github.com/beejmaxx/polymarket-mcp-rs --locked
```

## Codex

The current Codex CLI accepts a local stdio command after `--`:

```bash
codex mcp add polymarket -- polymarket-mcp-rs
codex mcp list
```

To use a profile explicitly:

```bash
codex mcp add polymarket --env POLYMARKET_TOOL_PROFILE=core -- polymarket-mcp-rs
```

## Claude Code

```bash
claude mcp add polymarket -- polymarket-mcp-rs
```

For a project-scoped configuration, use Claude Code's `--scope project` option before the `--` separator.

## Claude Desktop and generic stdio clients

Open the client's MCP JSON configuration and add:

```json
{
  "mcpServers": {
    "polymarket": {
      "command": "/absolute/path/to/polymarket-mcp-rs",
      "env": {
        "POLYMARKET_TOOL_PROFILE": "research",
        "POLYMARKET_MCP_DB": "/absolute/path/to/polymarket-mcp.sqlite3"
      }
    }
  }
}
```

Claude Desktop's configuration is normally `~/Library/Application Support/Claude/claude_desktop_config.json` on macOS and `%APPDATA%\Claude\claude_desktop_config.json` on Windows. Fully quit and restart the desktop client after editing it.

## VS Code

Create `.vscode/mcp.json` for a workspace or use **MCP: Open User Configuration**:

```json
{
  "servers": {
    "polymarket": {
      "type": "stdio",
      "command": "/absolute/path/to/polymarket-mcp-rs",
      "env": {
        "POLYMARKET_TOOL_PROFILE": "research"
      }
    }
  }
}
```

If VS Code MCP sandboxing is enabled, allow network access to `gamma-api.polymarket.com`, `clob.polymarket.com`, `data-api.polymarket.com`, and `ws-subscriptions-clob.polymarket.com`, plus write access to the selected database path.

## Troubleshooting

- Run `polymarket-mcp-rs doctor --json` outside the client first.
- Use an absolute executable and database path when GUI applications have a different PATH or working directory.
- Confirm stdout is not being redirected; MCP uses stdout, while logs use stderr.
- A failed WebSocket check with healthy REST checks usually means a proxy, firewall, or regional network path issue. Configure `POLYMARKET_WS_PROXY` independently from HTTP proxy variables.
- On macOS, a manually downloaded unsigned binary may need explicit approval in system security settings. The shell installer download normally avoids Finder quarantine; release checksums and GitHub provenance should still be verified.
- Run `polymarket-mcp-rs tools` to confirm the selected profile. Trading tools are intentionally absent from the default catalog.
