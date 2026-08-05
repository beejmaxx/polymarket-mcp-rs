ARG RUST_IMAGE=rust:1.90-bookworm
ARG RUNTIME_IMAGE=debian:bookworm-slim

FROM ${RUST_IMAGE} AS builder

WORKDIR /source
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/source/target,sharing=locked \
    cargo build --locked --release \
    && cp target/release/polymarket-mcp-rs /tmp/polymarket-mcp-rs

FROM ${RUNTIME_IMAGE} AS runtime

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /tmp/polymarket-mcp-rs /usr/local/bin/polymarket-mcp-rs

USER 10001:10001
EXPOSE 8080
ENV POLYMARKET_CACHE_TTL_MS=2000 \
    POLYMARKET_HTTP_BIND=0.0.0.0:8080 \
    POLYMARKET_HTTP_RATE_LIMIT_PER_MINUTE=120 \
    RUST_LOG=polymarket_mcp_rs=info,tower_http=info

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["polymarket-mcp-rs", "healthcheck"]

ENTRYPOINT ["polymarket-mcp-rs"]
CMD ["serve", "--transport", "http"]
