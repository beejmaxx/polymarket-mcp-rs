# Security policy

## Reporting a vulnerability

Please do not open a public issue for vulnerabilities involving private-key exposure, authentication bypass, unintended order placement, confirmation bypass, or credential leakage. Use GitHub's private vulnerability reporting for this repository.

Include the affected version, reproduction steps, impact, and any suggested mitigation. Avoid testing with another person's wallet or placing live orders without authorization.

## Operating model

- Trading is disabled unless `POLYMARKET_ENABLE_TRADING=true`.
- Private keys are loaded from process environment and must never be committed or logged.
- Order placement and cancellation require explicit confirmation fields.
- The server is designed for local stdio use. Do not expose it as a remote unauthenticated service.
- Use a dedicated low-value wallet and a conservative `POLYMARKET_MAX_ORDER_USDC` while evaluating trading features.

Market data is public, but locally recorded books and wallet activity may still be operationally sensitive. Protect the SQLite database accordingly.

