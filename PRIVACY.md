# Privacy

Polymarket MCP is open-source software that queries public Polymarket endpoints. The project does not include analytics, advertising, user accounts, or project-operated telemetry.

## Local stdio use

Local research recordings and approval audit records stay in the SQLite database on the machine running the server. If an operator enables authenticated tools, credentials remain in that process environment and requests are sent to Polymarket. Protect the machine, environment, and database as sensitive data.

## Public read-only service

The provided HTTP configuration exposes public market and public proxy-wallet data without an account. Its application layer uses in-memory state and does not persist MCP prompts or results. Health counters contain aggregate request counts and timing, not prompt bodies.

A person or organization deploying this repository controls its infrastructure and logs. Their reverse proxy or hosting provider may retain IP addresses, headers, timestamps, and other operational metadata under their own policy. Consult that operator's privacy notice for a specific hosted endpoint.

## External services

Queries are sent to Polymarket's Gamma, Data, and CLOB services. Public wallet queries include the requested public address. Review Polymarket's terms and privacy policy before using those services.

Do not send secrets in MCP prompts. Report a project privacy or security concern through the private process described in [SECURITY.md](SECURITY.md).
