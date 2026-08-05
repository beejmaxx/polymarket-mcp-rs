# ChatGPT setup

The server supports two ChatGPT paths. Both use the compact, credential-free `chatgpt` profile. Never expose the `research`, `trading`, or `all` profiles through a public endpoint.

## Private local use through Secure MCP Tunnel

This keeps the Rust process and all requests on your machine except for the encrypted outbound tunnel connection. Install a release binary, verify it, and locate its absolute path:

```bash
polymarket-mcp-rs doctor
command -v polymarket-mcp-rs
```

Create a tunnel in the OpenAI Platform and initialize the tunnel client with the binary as a local stdio MCP command:

```bash
export CONTROL_PLANE_API_KEY="sk-..."

tunnel-client init \
  --sample sample_mcp_stdio_local \
  --profile polymarket-local \
  --tunnel-id tunnel_... \
  --mcp-command "/absolute/path/to/polymarket-mcp-rs --tool-profile chatgpt"

tunnel-client doctor --profile polymarket-local --explain
tunnel-client run --profile polymarket-local
```

In ChatGPT, enable developer mode if your account or workspace permits it, add a plugin, choose **Tunnel**, and select the configured tunnel. UI names can change; the current official instructions are at [Connect and test your plugin](https://developers.openai.com/plugins/deploy/connect-chatgpt) and [Secure MCP Tunnels](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels).

The tunnel is for private development and is not accepted as the endpoint for a public plugin submission.

## Public HTTPS use

Run Streamable HTTP locally:

```bash
polymarket-mcp-rs serve --transport http --bind 127.0.0.1:8080
curl http://127.0.0.1:8080/healthz
```

After deploying behind HTTPS, add `https://your-domain.example/mcp` as a custom MCP/plugin endpoint in ChatGPT developer mode. Set the exact public hostname:

```bash
POLYMARKET_HTTP_ALLOWED_HOSTS=your-domain.example
POLYMARKET_HTTP_ALLOWED_ORIGINS=https://chatgpt.com
```

Render, Railway, and Fly hostnames are recognized automatically from their standard environment variables. Explicit configuration is still preferable for a custom domain.

The public process ignores trading credentials even if they exist in its environment. It also rejects the stateful `research`, `trading`, and `all` profiles. `get_market_brief` can render a small inline card through the open MCP Apps standard; its complete structured result remains available when a client does not support UI.

## Test prompts

1. `Find the five highest-volume active Polymarket markets.`
2. `Give me a complete source-linked brief for the most liquid result.`
3. `Estimate the displayed fill impact of buying 100 shares; do not recommend a trade.`
4. `Compare the first two markets and explain which data came from Gamma versus the CLOB.`
5. `What can you not determine from this server?`

See `evals/chatgpt-cases.json` for direct, indirect, follow-up, and unsupported test cases.
