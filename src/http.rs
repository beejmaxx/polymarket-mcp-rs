use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, State},
    http::{HeaderValue, Request, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use serde::Serialize;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tower_http::{catch_panic::CatchPanicLayer, timeout::TimeoutLayer, trace::TraceLayer};

use crate::{App, PolymarketServer, ToolProfile, error::AppError};

#[derive(Clone, Debug)]
pub struct HttpConfig {
    pub bind: SocketAddr,
    pub allowed_hosts: Vec<String>,
    pub allowed_origins: Vec<String>,
    pub requests_per_minute: u32,
    pub request_timeout: Duration,
    pub max_request_body_bytes: usize,
}

impl HttpConfig {
    pub fn from_environment(bind: &str) -> Result<Self, AppError> {
        let bind = SocketAddr::from_str(bind).map_err(|error| {
            AppError::InvalidInput(format!("invalid HTTP bind address {bind:?}: {error}"))
        })?;
        let mut allowed_hosts =
            comma_values("POLYMARKET_HTTP_ALLOWED_HOSTS").unwrap_or_else(|| {
                let mut hosts = vec![
                    "localhost".to_owned(),
                    "127.0.0.1".to_owned(),
                    "::1".to_owned(),
                ];
                let address = bind.ip().to_string();
                if !hosts.contains(&address) {
                    hosts.push(address);
                }
                hosts
            });
        for name in ["RENDER_EXTERNAL_HOSTNAME", "RAILWAY_PUBLIC_DOMAIN"] {
            if let Ok(host) = std::env::var(name) {
                let host = host.trim().to_owned();
                if !host.is_empty() && !allowed_hosts.contains(&host) {
                    allowed_hosts.push(host);
                }
            }
        }
        if let Ok(app) = std::env::var("FLY_APP_NAME")
            && !app.trim().is_empty()
        {
            let host = format!("{}.fly.dev", app.trim());
            if !allowed_hosts.contains(&host) {
                allowed_hosts.push(host);
            }
        }
        let allowed_origins = comma_values("POLYMARKET_HTTP_ALLOWED_ORIGINS").unwrap_or_default();
        let requests_per_minute = environment_number("POLYMARKET_HTTP_RATE_LIMIT_PER_MINUTE", 120)?
            .clamp(1, 10_000) as u32;
        let timeout_seconds =
            environment_number("POLYMARKET_HTTP_TIMEOUT_SECONDS", 60)?.clamp(1, 300);
        let max_request_body_bytes =
            environment_number("POLYMARKET_HTTP_MAX_BODY_BYTES", 1024 * 1024)?
                .clamp(1024, 16 * 1024 * 1024);
        Ok(Self {
            bind,
            allowed_hosts,
            allowed_origins,
            requests_per_minute,
            request_timeout: Duration::from_secs(timeout_seconds as u64),
            max_request_body_bytes,
        })
    }
}

#[derive(Clone)]
struct HttpState {
    profile: ToolProfile,
    limiter: Arc<Mutex<HashMap<IpAddr, RateWindow>>>,
    rate_limit: u32,
    metrics: Arc<HttpMetrics>,
}

#[derive(Clone, Copy)]
struct RateWindow {
    started: Instant,
    count: u32,
}

#[derive(Default)]
struct HttpMetrics {
    started_at: std::sync::OnceLock<Instant>,
    requests: AtomicU64,
    rejected: AtomicU64,
    server_errors: AtomicU64,
    latency_micros: AtomicU64,
    in_flight: AtomicUsize,
}

impl HttpMetrics {
    fn start(&self) {
        let _ = self.started_at.set(Instant::now());
    }
}

#[derive(Serialize)]
struct Health<'a> {
    status: &'a str,
    service: &'a str,
    version: &'a str,
    profile: &'a str,
}

pub async fn serve_http(profile: ToolProfile, config: HttpConfig) -> Result<(), AppError> {
    if !matches!(profile, ToolProfile::Chatgpt | ToolProfile::Core) {
        return Err(AppError::InvalidInput(
            "HTTP transport permits only the stateless chatgpt or core profile; use stdio for research and trading profiles"
                .to_owned(),
        ));
    }
    let app = App::new_public()?;
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|error| AppError::Upstream {
            service: "HTTP listener",
            message: error.to_string(),
        })?;
    let address = listener.local_addr().map_err(|error| AppError::Upstream {
        service: "HTTP listener",
        message: error.to_string(),
    })?;
    tracing::info!(%address, %profile, path = "/mcp", "Polymarket MCP HTTP server listening");
    let cancellation = CancellationToken::new();
    let result = serve_on_listener(
        listener,
        app.clone(),
        profile,
        config,
        cancellation.clone(),
        shutdown_signal(cancellation),
    )
    .await;
    let shutdown = app.shutdown().await;
    result?;
    shutdown.map(|_| ())
}

pub async fn serve_on_listener<F>(
    listener: tokio::net::TcpListener,
    app: App,
    profile: ToolProfile,
    config: HttpConfig,
    cancellation: CancellationToken,
    shutdown: F,
) -> Result<(), AppError>
where
    F: Future<Output = ()> + Send + 'static,
{
    let router = build_router(app, profile, &config, cancellation);
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
    .map_err(|error| AppError::Upstream {
        service: "HTTP server",
        message: error.to_string(),
    })
}

fn build_router(
    app: App,
    profile: ToolProfile,
    config: &HttpConfig,
    cancellation: CancellationToken,
) -> Router {
    let mut transport_config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_max_request_body_bytes(config.max_request_body_bytes)
        .with_cancellation_token(cancellation);
    if config.allowed_hosts == ["*"] {
        tracing::warn!("HTTP Host validation explicitly disabled");
        transport_config = transport_config.disable_allowed_hosts();
    } else {
        transport_config = transport_config.with_allowed_hosts(config.allowed_hosts.clone());
    }
    if !config.allowed_origins.is_empty() {
        transport_config = transport_config.with_allowed_origins(config.allowed_origins.clone());
    }
    let factory_app = app;
    let mcp_service: StreamableHttpService<PolymarketServer, NeverSessionManager> =
        StreamableHttpService::new(
            move || Ok(PolymarketServer::with_profile(factory_app.clone(), profile)),
            Arc::new(NeverSessionManager::default()),
            transport_config,
        );
    let metrics = Arc::new(HttpMetrics::default());
    metrics.start();
    let state = HttpState {
        profile,
        limiter: Arc::new(Mutex::new(HashMap::new())),
        rate_limit: config.requests_per_minute,
        metrics,
    };
    Router::new()
        .route("/", get(landing))
        .route("/healthz", get(health))
        .route("/readyz", get(health))
        .route("/metrics", get(prometheus_metrics))
        .nest_service("/mcp", mcp_service)
        .layer(middleware::from_fn_with_state(state.clone(), observe))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            config.request_timeout,
        ))
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn observe(State(state): State<HttpState>, request: Request<Body>, next: Next) -> Response {
    if request.uri().path() != "/mcp" {
        return next.run(request).await;
    }
    state.metrics.requests.fetch_add(1, Ordering::Relaxed);
    let ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]));
    if !allow_request(&state, ip).await {
        state.metrics.rejected.fetch_add(1, Ordering::Relaxed);
        let mut response = (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        return response;
    }
    state.metrics.in_flight.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    let mut response = next.run(request).await;
    state.metrics.in_flight.fetch_sub(1, Ordering::Relaxed);
    state.metrics.latency_micros.fetch_add(
        started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
        Ordering::Relaxed,
    );
    if response.status().is_server_error() {
        state.metrics.server_errors.fetch_add(1, Ordering::Relaxed);
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn allow_request(state: &HttpState, ip: IpAddr) -> bool {
    let now = Instant::now();
    let mut clients = state.limiter.lock().await;
    if clients.len() > 4096 {
        clients.retain(|_, window| now.duration_since(window.started) < Duration::from_secs(120));
    }
    let window = clients.entry(ip).or_insert(RateWindow {
        started: now,
        count: 0,
    });
    if now.duration_since(window.started) >= Duration::from_secs(60) {
        *window = RateWindow {
            started: now,
            count: 0,
        };
    }
    if window.count >= state.rate_limit {
        return false;
    }
    window.count += 1;
    true
}

async fn health(State(state): State<HttpState>) -> Json<Health<'static>> {
    Json(Health {
        status: "ok",
        service: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        profile: state.profile.as_str(),
    })
}

async fn prometheus_metrics(State(state): State<HttpState>) -> impl IntoResponse {
    let started = state
        .metrics
        .started_at
        .get()
        .map_or(0.0, |started| started.elapsed().as_secs_f64());
    let body = format!(
        concat!(
            "# TYPE polymarket_mcp_http_requests_total counter\n",
            "polymarket_mcp_http_requests_total {}\n",
            "# TYPE polymarket_mcp_http_rate_limited_total counter\n",
            "polymarket_mcp_http_rate_limited_total {}\n",
            "# TYPE polymarket_mcp_http_server_errors_total counter\n",
            "polymarket_mcp_http_server_errors_total {}\n",
            "# TYPE polymarket_mcp_http_in_flight gauge\n",
            "polymarket_mcp_http_in_flight {}\n",
            "# TYPE polymarket_mcp_http_latency_seconds_total counter\n",
            "polymarket_mcp_http_latency_seconds_total {}\n",
            "# TYPE polymarket_mcp_process_uptime_seconds gauge\n",
            "polymarket_mcp_process_uptime_seconds {}\n"
        ),
        state.metrics.requests.load(Ordering::Relaxed),
        state.metrics.rejected.load(Ordering::Relaxed),
        state.metrics.server_errors.load(Ordering::Relaxed),
        state.metrics.in_flight.load(Ordering::Relaxed),
        state.metrics.latency_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0,
        started,
    );
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], body)
}

async fn landing() -> Html<&'static str> {
    Html(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Polymarket MCP</title><style>body{max-width:720px;margin:10vh auto;padding:24px;font:17px system-ui;line-height:1.5;color:#172033}code{background:#eef2f7;padding:2px 6px;border-radius:5px}</style></head><body><h1>Polymarket MCP</h1><p>Credential-free, source-linked Polymarket research over MCP.</p><p>Streamable HTTP endpoint: <code>/mcp</code></p><p>Operations: <a href="/healthz">health</a> · <a href="/metrics">metrics</a></p></body></html>"#,
    )
}

fn comma_values(name: &str) -> Option<Vec<String>> {
    std::env::var(name).ok().map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

fn environment_number(name: &str, default: usize) -> Result<usize, AppError> {
    std::env::var(name)
        .ok()
        .map(|value| {
            value.parse::<usize>().map_err(|error| {
                AppError::InvalidInput(format!("{name} must be a positive integer: {error}"))
            })
        })
        .transpose()
        .map(|value| value.unwrap_or(default))
}

async fn shutdown_signal(cancellation: CancellationToken) {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    cancellation.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_http_config_defaults_are_bounded() {
        let config = HttpConfig::from_environment("127.0.0.1:8080").unwrap();
        assert_eq!(config.requests_per_minute, 120);
        assert!(config.allowed_hosts.contains(&"127.0.0.1".to_owned()));
        assert!(config.max_request_body_bytes <= 16 * 1024 * 1024);
    }
}
