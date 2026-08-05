# Read-only HTTP deployment

The hosted deployment is intentionally a different security boundary from local stdio:

| Surface | Profile | Persistence | Credentials |
|---|---|---|---|
| Local stdio | `research` by default | SQLite recording/replay | Optional local trading opt-in |
| Public HTTP | `chatgpt` by default | None | Ignored by construction |

## Run the container

```bash
docker build -t polymarket-mcp .
docker run --read-only --tmpfs /tmp \
  -p 8080:8080 \
  -e POLYMARKET_HTTP_ALLOWED_HOSTS=localhost,127.0.0.1 \
  polymarket-mcp
```

Or:

```bash
docker compose up --build
```

After the container workflow has published the repository package, operators can also pull `ghcr.io/beejmaxx/polymarket-mcp-rs:latest` instead of building locally. Pin a release tag or digest in production.

The service exposes:

- `POST /mcp` — stateless Streamable HTTP MCP
- `GET /healthz` and `GET /readyz` — process readiness
- `GET /metrics` — Prometheus text metrics
- `GET /` — small human-readable endpoint page

## Production configuration

| Variable | Default | Purpose |
|---|---:|---|
| `POLYMARKET_HTTP_BIND` | `127.0.0.1:8080` outside the image | Listener address |
| `POLYMARKET_HTTP_ALLOWED_HOSTS` | loopback/bind address | Comma-separated DNS-rebinding allowlist; `*` explicitly disables this protection |
| `POLYMARKET_HTTP_ALLOWED_ORIGINS` | unset | Optional comma-separated browser Origin allowlist |
| `POLYMARKET_HTTP_RATE_LIMIT_PER_MINUTE` | `120` | Per-client-IP request window |
| `POLYMARKET_HTTP_TIMEOUT_SECONDS` | `60` | End-to-end request timeout |
| `POLYMARKET_HTTP_MAX_BODY_BYTES` | `1048576` | MCP POST body limit |
| `POLYMARKET_CACHE_TTL_MS` | `2000` for public HTTP | Short market-brief cache; `0` disables, maximum `60000` |

Terminate TLS at a managed load balancer or reverse proxy. The built-in limiter intentionally trusts only the immediate TCP peer, not spoofable forwarding headers; behind a shared proxy it becomes a per-proxy backstop. Configure real client-aware rate limiting at the trusted edge. Do not set private keys, API secrets, wallet cookies, or shared user credentials in a public deployment.

`render.yaml` and `deploy/fly.toml.example` provide starting configurations. Hosting account creation, billing, final region/domain choice, and DNS changes remain operator-owned actions.

## Public ChatGPT submission

A public plugin needs a stable HTTPS `/mcp` endpoint, verified domain/identity, accurate tool metadata and test cases, privacy and support URLs, and workspace permission to submit. This repository includes [privacy](../PRIVACY.md), [support](../SUPPORT.md), versioned eval fixtures, and an optional portable inline card; a deployer must publish the policy pages at stable public URLs and identify the actual operator. Follow the current [OpenAI submission requirements](https://developers.openai.com/plugins/deploy/submission) rather than copying old marketplace instructions.

## Release smoke checks

Before pointing ChatGPT or another hosted client at a deployment:

```bash
curl --fail https://your-domain.example/healthz
curl --fail https://your-domain.example/readyz
curl --fail https://your-domain.example/metrics
```

Then connect MCP Inspector or another MCP client to `https://your-domain.example/mcp`, verify that exactly 11 tools are exposed under the default hosted profile, read `ui://polymarket/market-brief-v1.html`, and run the prompts in `evals/chatgpt-cases.json`.
