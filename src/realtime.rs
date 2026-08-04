use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::{SinkExt as _, StreamExt as _};
use polymarket_client_sdk_v2::{
    clob::{
        types::{Side, response::OrderBookSummaryResponse},
        ws::{BookUpdate, PriceChange, WsMessage},
    },
    types::{Decimal, U256},
};
use serde_json::json;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{RwLock, broadcast, watch},
    time::{MissedTickBehavior, interval},
};
use tokio_socks::tcp::Socks5Stream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, client_async_tls, connect_async, tungstenite::Message,
};
use url::Url;

use crate::{
    error::AppError,
    types::{
        LiveOrderBook, LiveSnapshotOutput, PriceLevel, RealtimeEvent, RealtimeEventsOutput,
        RealtimeStatusOutput, StopWatchOutput, WatchInfo,
    },
};

pub const CLOB_WS_ENDPOINT: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";

#[derive(Clone, Debug)]
pub struct RealtimeUpdate {
    pub watch_id: String,
    pub book: LiveOrderBook,
}

#[derive(Clone)]
pub struct RealtimeService {
    endpoint: Arc<str>,
    socks_proxy: Option<Arc<str>>,
    watches: Arc<RwLock<HashMap<String, WatchState>>>,
    next_id: Arc<AtomicU64>,
    updates: broadcast::Sender<RealtimeUpdate>,
}

impl fmt::Debug for RealtimeService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RealtimeService")
            .field("endpoint", &self.endpoint)
            .field("socks_proxy_configured", &self.socks_proxy.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct WatchState {
    token_ids: Vec<String>,
    started_at_ms: u64,
    last_update_at_ms: Option<u64>,
    connection_state: String,
    update_count: u64,
    rest_seed_count: u64,
    websocket_update_count: u64,
    price_change_count: u64,
    reconnect_count: u64,
    error_count: u64,
    last_error: Option<String>,
    books: HashMap<String, LiveOrderBook>,
    recent_events: VecDeque<RealtimeEvent>,
    next_event_sequence: u64,
    cancel: watch::Sender<bool>,
}

impl RealtimeService {
    pub fn new() -> Result<Self, AppError> {
        let endpoint =
            std::env::var("POLYMARKET_WS_URL").unwrap_or_else(|_| CLOB_WS_ENDPOINT.to_owned());
        validate_endpoint(&endpoint)?;
        let socks_proxy = configured_socks_proxy()?;
        Ok(Self {
            endpoint: Arc::from(endpoint),
            socks_proxy: socks_proxy.map(Arc::from),
            watches: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
            updates: broadcast::channel(4_096).0,
        })
    }

    pub async fn watch_markets(
        &self,
        token_ids: Vec<U256>,
        initial_books: Vec<OrderBookSummaryResponse>,
    ) -> Result<WatchInfo, AppError> {
        let watch_id = format!("watch-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let token_id_strings = token_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let (cancel, cancel_rx) = watch::channel(false);
        let seeded_at = now_ms();
        let books = initial_books
            .into_iter()
            .map(|book| {
                let book = rest_live_book(book, seeded_at);
                (book.token_id.clone(), book)
            })
            .collect::<HashMap<_, _>>();
        let seed_count = books.len() as u64;
        let state = WatchState {
            token_ids: token_id_strings.clone(),
            started_at_ms: now_ms(),
            last_update_at_ms: (!books.is_empty()).then_some(seeded_at),
            connection_state: "Connecting".to_owned(),
            update_count: seed_count,
            rest_seed_count: seed_count,
            websocket_update_count: 0,
            price_change_count: 0,
            reconnect_count: 0,
            error_count: 0,
            last_error: None,
            books,
            recent_events: VecDeque::with_capacity(1_024),
            next_event_sequence: 1,
            cancel,
        };
        let info = watch_info(&watch_id, &state);
        self.watches.write().await.insert(watch_id.clone(), state);

        let task = WatchTask {
            watch_id,
            token_ids: token_id_strings,
            endpoint: Arc::clone(&self.endpoint),
            socks_proxy: self.socks_proxy.clone(),
            watches: Arc::clone(&self.watches),
            updates: self.updates.clone(),
            cancel: cancel_rx,
        };
        tokio::spawn(task.run());

        Ok(info)
    }

    pub async fn snapshot(
        &self,
        watch_id: &str,
        depth: usize,
    ) -> Result<LiveSnapshotOutput, AppError> {
        let states = self.watches.read().await;
        let state = states
            .get(watch_id)
            .ok_or_else(|| AppError::InvalidInput(format!("unknown watch_id {watch_id}")))?;
        let now = now_ms();
        let mut books = state.books.values().cloned().collect::<Vec<_>>();
        books.sort_by(|left, right| left.token_id.cmp(&right.token_id));
        for book in &mut books {
            book.feed_age_ms = Some(now.saturating_sub(book.received_at_ms));
            book.bids.truncate(depth);
            book.asks.truncate(depth);
        }
        Ok(LiveSnapshotOutput {
            watch: watch_info(watch_id, state),
            books,
        })
    }

    pub async fn status(&self) -> RealtimeStatusOutput {
        let states = self.watches.read().await;
        let mut watches = states
            .iter()
            .map(|(id, state)| watch_info(id, state))
            .collect::<Vec<_>>();
        watches.sort_by(|left, right| left.watch_id.cmp(&right.watch_id));
        let connection_state = aggregate_connection_state(&states);
        RealtimeStatusOutput {
            connection_state,
            active_watch_count: states.len(),
            subscription_count: states.values().map(|state| state.token_ids.len()).sum(),
            watches,
        }
    }

    pub async fn events(
        &self,
        watch_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<RealtimeEventsOutput, AppError> {
        let states = self.watches.read().await;
        let state = states
            .get(watch_id)
            .ok_or_else(|| AppError::InvalidInput(format!("unknown watch_id {watch_id}")))?;
        let events = state
            .recent_events
            .iter()
            .filter(|event| event.sequence > after_sequence)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        Ok(RealtimeEventsOutput {
            watch_id: watch_id.to_owned(),
            count: events.len(),
            latest_sequence: state.next_event_sequence.saturating_sub(1),
            events,
        })
    }

    pub async fn stop(&self, watch_id: &str) -> StopWatchOutput {
        let state = self.watches.write().await.remove(watch_id);
        if let Some(state) = &state {
            let _ = state.cancel.send(true);
        }
        StopWatchOutput {
            watch_id: watch_id.to_owned(),
            stopped: state.is_some(),
        }
    }

    #[must_use]
    pub fn subscribe_updates(&self) -> broadcast::Receiver<RealtimeUpdate> {
        self.updates.subscribe()
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

struct WatchTask {
    watch_id: String,
    token_ids: Vec<String>,
    endpoint: Arc<str>,
    socks_proxy: Option<Arc<str>>,
    watches: Arc<RwLock<HashMap<String, WatchState>>>,
    updates: broadcast::Sender<RealtimeUpdate>,
    cancel: watch::Receiver<bool>,
}

impl WatchTask {
    async fn run(mut self) {
        let mut backoff = Duration::from_secs(1);
        loop {
            if *self.cancel.borrow() {
                break;
            }
            self.set_connection_state(if backoff == Duration::from_secs(1) {
                "Connecting"
            } else {
                "Reconnecting"
            })
            .await;

            let result = self.connect_once().await;
            if *self.cancel.borrow() {
                break;
            }
            if let Err(error) = result {
                self.record_connection_error(error).await;
            }
            {
                let mut states = self.watches.write().await;
                let Some(state) = states.get_mut(&self.watch_id) else {
                    break;
                };
                state.connection_state = "Reconnecting".to_owned();
                state.reconnect_count += 1;
            }
            tokio::select! {
                changed = self.cancel.changed() => {
                    if changed.is_err() || *self.cancel.borrow() { break; }
                }
                () = tokio::time::sleep(backoff) => {}
            }
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }
    }

    async fn connect_once(&mut self) -> Result<(), String> {
        if let Some(proxy) = &self.socks_proxy {
            let endpoint = Url::parse(&self.endpoint).map_err(|error| error.to_string())?;
            let host = endpoint
                .host_str()
                .ok_or_else(|| "websocket endpoint has no host".to_owned())?;
            let port = endpoint
                .port_or_known_default()
                .ok_or_else(|| "websocket endpoint has no port".to_owned())?;
            let stream = Socks5Stream::connect(proxy.as_ref(), (host, port))
                .await
                .map_err(|error| format!("SOCKS connection failed: {error}"))?;
            let (socket, _) = client_async_tls(self.endpoint.as_ref(), stream)
                .await
                .map_err(|error| format!("websocket handshake failed: {error}"))?;
            self.consume_socket(socket).await
        } else {
            let (socket, _) = connect_async(self.endpoint.as_ref())
                .await
                .map_err(|error| format!("websocket connection failed: {error}"))?;
            self.consume_socket(socket).await
        }
    }

    async fn consume_socket<S>(
        &mut self,
        mut socket: WebSocketStream<MaybeTlsStream<S>>,
    ) -> Result<(), String>
    where
        S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let request = json!({
            "assets_ids": self.token_ids,
            "type": "market",
            "custom_feature_enabled": true
        });
        socket
            .send(Message::Text(request.to_string().into()))
            .await
            .map_err(|error| format!("failed to subscribe: {error}"))?;
        self.set_connection_state("Connected").await;

        let mut heartbeat = interval(Duration::from_secs(10));
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
        heartbeat.tick().await;
        let mut last_received = Instant::now();
        loop {
            tokio::select! {
                changed = self.cancel.changed() => {
                    if changed.is_err() || *self.cancel.borrow() { return Ok(()); }
                }
                _ = heartbeat.tick() => {
                    if last_received.elapsed() > Duration::from_secs(30) {
                        return Err("websocket heartbeat timed out after 30 seconds".to_owned());
                    }
                    socket.send(Message::Text("PING".into())).await
                        .map_err(|error| format!("failed to send websocket heartbeat: {error}"))?;
                }
                message = socket.next() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            last_received = Instant::now();
                            if text == "PONG" { continue; }
                            for message in decode_messages(&text)? {
                                self.apply_message(message).await;
                            }
                        }
                        Some(Ok(Message::Binary(bytes))) => {
                            last_received = Instant::now();
                            let text = std::str::from_utf8(&bytes)
                                .map_err(|error| format!("websocket sent non-UTF-8 data: {error}"))?;
                            for message in decode_messages(text)? {
                                self.apply_message(message).await;
                            }
                        }
                        Some(Ok(Message::Ping(payload))) => {
                            last_received = Instant::now();
                            socket.send(Message::Pong(payload)).await
                                .map_err(|error| format!("failed to send websocket pong: {error}"))?;
                        }
                        Some(Ok(Message::Pong(_))) => last_received = Instant::now(),
                        Some(Ok(Message::Close(frame))) => {
                            return Err(format!("websocket closed: {frame:?}"));
                        }
                        Some(Ok(_)) => {}
                        Some(Err(error)) => return Err(format!("websocket receive failed: {error}")),
                        None => return Err("websocket stream ended".to_owned()),
                    }
                }
            }
        }
    }

    async fn apply_message(&self, message: WsMessage) {
        let received_at_ms = now_ms();
        match message {
            WsMessage::Book(book) => {
                let book = live_book(book, received_at_ms);
                self.publish_book(book, received_at_ms, false).await;
            }
            WsMessage::PriceChange(change) => {
                self.apply_price_change(change, received_at_ms).await;
            }
            WsMessage::LastTradePrice(trade) => {
                let mut event = realtime_event("last_trade_price", trade.timestamp);
                event.condition_id = Some(trade.market.to_string());
                event.token_id = Some(trade.asset_id.to_string());
                event.price = Some(trade.price.to_string());
                event.size = trade.size.map(|value| value.to_string());
                event.side = trade.side.map(|value| value.to_string());
                self.push_event(event).await;
            }
            WsMessage::BestBidAsk(update) => {
                let mut event = realtime_event("best_bid_ask", update.timestamp);
                event.condition_id = Some(update.market.to_string());
                event.token_id = Some(update.asset_id.to_string());
                event.best_bid = Some(update.best_bid.to_string());
                event.best_ask = Some(update.best_ask.to_string());
                self.push_event(event).await;
            }
            WsMessage::TickSizeChange(update) => {
                let mut event = realtime_event("tick_size_change", update.timestamp);
                event.condition_id = Some(update.market.to_string());
                event.token_id = Some(update.asset_id.to_string());
                event.old_tick_size = Some(update.old_tick_size.to_string());
                event.new_tick_size = Some(update.new_tick_size.to_string());
                self.push_event(event).await;
            }
            WsMessage::MarketResolved(resolution) => {
                let mut event = realtime_event("market_resolved", resolution.timestamp);
                event.condition_id = Some(resolution.market.to_string());
                event.winning_token_id = Some(resolution.winning_asset_id.to_string());
                event.winning_outcome = Some(resolution.winning_outcome);
                event.question = resolution.question;
                event.slug = resolution.slug;
                self.push_event(event).await;
            }
            WsMessage::NewMarket(market) => {
                let mut event = realtime_event("new_market", market.timestamp);
                event.condition_id = Some(market.market.to_string());
                event.question = Some(market.question);
                event.slug = Some(market.slug);
                self.push_event(event).await;
            }
            _ => {}
        }
    }

    async fn push_event(&self, mut event: RealtimeEvent) {
        let mut states = self.watches.write().await;
        let Some(state) = states.get_mut(&self.watch_id) else {
            return;
        };
        event.sequence = state.next_event_sequence;
        state.next_event_sequence += 1;
        if state.recent_events.len() == 1_024 {
            state.recent_events.pop_front();
        }
        state.recent_events.push_back(event);
    }

    async fn apply_price_change(&self, change: PriceChange, received_at_ms: u64) {
        let mut changed_books = Vec::new();
        {
            let mut states = self.watches.write().await;
            let Some(state) = states.get_mut(&self.watch_id) else {
                return;
            };
            for delta in change.price_changes {
                let token_id = delta.asset_id.to_string();
                let Some(book) = state.books.get_mut(&token_id) else {
                    continue;
                };
                let levels = if delta.side == Side::Buy {
                    &mut book.bids
                } else {
                    &mut book.asks
                };
                // A missing size is not a deletion. Some protocol variants carry only
                // best-price metadata, so only an explicit zero removes a level.
                if !apply_optional_level(levels, delta.price, delta.size) {
                    continue;
                }
                normalize_book(book);
                book.source = "websocket_delta".to_owned();
                book.condition_id = change.market.to_string();
                book.upstream_timestamp_ms = change.timestamp;
                book.received_at_ms = received_at_ms;
                book.feed_age_ms =
                    Some(received_at_ms.saturating_sub(change.timestamp.max(0) as u64));
                book.hash = delta.hash;
                changed_books.push(book.clone());
            }
            if !changed_books.is_empty() {
                state.last_update_at_ms = Some(received_at_ms);
                state.update_count += changed_books.len() as u64;
                state.websocket_update_count += changed_books.len() as u64;
                state.price_change_count += changed_books.len() as u64;
                state.last_error = None;
            }
        }
        for book in changed_books {
            let _ = self.updates.send(RealtimeUpdate {
                watch_id: self.watch_id.clone(),
                book,
            });
        }
    }

    async fn publish_book(&self, book: LiveOrderBook, received_at_ms: u64, price_change: bool) {
        {
            let mut states = self.watches.write().await;
            let Some(state) = states.get_mut(&self.watch_id) else {
                return;
            };
            state.last_update_at_ms = Some(received_at_ms);
            state.update_count += 1;
            state.websocket_update_count += 1;
            state.price_change_count += u64::from(price_change);
            state.last_error = None;
            state.books.insert(book.token_id.clone(), book.clone());
        }
        let _ = self.updates.send(RealtimeUpdate {
            watch_id: self.watch_id.clone(),
            book,
        });
    }

    async fn set_connection_state(&self, value: &str) {
        let mut states = self.watches.write().await;
        if let Some(state) = states.get_mut(&self.watch_id) {
            state.connection_state = value.to_owned();
        }
    }

    async fn record_connection_error(&self, error: String) {
        let mut states = self.watches.write().await;
        if let Some(state) = states.get_mut(&self.watch_id) {
            state.error_count += 1;
            state.last_error = Some(error);
        }
    }
}

fn decode_messages(text: &str) -> Result<Vec<WsMessage>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("invalid websocket JSON: {error}"))?;
    let values = match value {
        serde_json::Value::Array(values) => values,
        value => vec![value],
    };
    Ok(values
        .into_iter()
        .filter(|value| value.get("event_type").is_some())
        .filter_map(|value| match serde_json::from_value(value) {
            Ok(message) => Some(message),
            Err(error) => {
                tracing::warn!(%error, "ignored unsupported websocket message");
                None
            }
        })
        .collect())
}

fn realtime_event(event_type: &str, timestamp_ms: i64) -> RealtimeEvent {
    RealtimeEvent {
        sequence: 0,
        event_type: event_type.to_owned(),
        timestamp_ms,
        condition_id: None,
        token_id: None,
        price: None,
        size: None,
        side: None,
        best_bid: None,
        best_ask: None,
        old_tick_size: None,
        new_tick_size: None,
        winning_token_id: None,
        winning_outcome: None,
        question: None,
        slug: None,
    }
}

fn apply_level(levels: &mut Vec<PriceLevel>, price: Decimal, size: Decimal) {
    let price = price.to_string();
    if size <= Decimal::ZERO {
        levels.retain(|level| level.price != price);
    } else if let Some(level) = levels.iter_mut().find(|level| level.price == price) {
        level.size = size.to_string();
    } else {
        levels.push(PriceLevel {
            price,
            size: size.to_string(),
        });
    }
}

fn apply_optional_level(
    levels: &mut Vec<PriceLevel>,
    price: Decimal,
    size: Option<Decimal>,
) -> bool {
    let Some(size) = size else {
        return false;
    };
    apply_level(levels, price, size);
    true
}

fn normalize_book(book: &mut LiveOrderBook) {
    book.bids
        .sort_by(|left, right| decimal_cmp(&right.price, &left.price));
    book.asks
        .sort_by(|left, right| decimal_cmp(&left.price, &right.price));
    let best_bid = book
        .bids
        .first()
        .and_then(|level| level.price.parse::<Decimal>().ok());
    let best_ask = book
        .asks
        .first()
        .and_then(|level| level.price.parse::<Decimal>().ok());
    book.best_bid = best_bid.map(|value| value.to_string());
    book.best_ask = best_ask.map(|value| value.to_string());
    book.spread = best_bid
        .zip(best_ask)
        .map(|(bid, ask)| (ask - bid).to_string());
    book.midpoint = best_bid
        .zip(best_ask)
        .map(|(bid, ask)| ((bid + ask) / Decimal::TWO).to_string());
}

fn decimal_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    left.parse::<Decimal>()
        .ok()
        .cmp(&right.parse::<Decimal>().ok())
}

fn watch_info(watch_id: &str, state: &WatchState) -> WatchInfo {
    WatchInfo {
        watch_id: watch_id.to_owned(),
        token_ids: state.token_ids.clone(),
        started_at_ms: state.started_at_ms,
        last_update_at_ms: state.last_update_at_ms,
        connection_state: state.connection_state.clone(),
        update_count: state.update_count,
        rest_seed_count: state.rest_seed_count,
        websocket_update_count: state.websocket_update_count,
        price_change_count: state.price_change_count,
        reconnect_count: state.reconnect_count,
        error_count: state.error_count,
        last_error: state.last_error.clone(),
        snapshot_count: state.books.len(),
    }
}

fn aggregate_connection_state(states: &HashMap<String, WatchState>) -> String {
    if states.is_empty() {
        return "Idle".to_owned();
    }
    let connected = states
        .values()
        .filter(|state| state.connection_state == "Connected")
        .count();
    if connected == states.len() {
        "Connected".to_owned()
    } else if connected > 0 {
        "Degraded".to_owned()
    } else if states
        .values()
        .any(|state| state.connection_state == "Reconnecting")
    {
        "Reconnecting".to_owned()
    } else {
        "Connecting".to_owned()
    }
}

fn live_book(book: BookUpdate, received_at_ms: u64) -> LiveOrderBook {
    let mut book = LiveOrderBook {
        source: "websocket_snapshot".to_owned(),
        token_id: book.asset_id.to_string(),
        condition_id: book.market.to_string(),
        upstream_timestamp_ms: book.timestamp,
        received_at_ms,
        feed_age_ms: Some(received_at_ms.saturating_sub(book.timestamp.max(0) as u64)),
        hash: book.hash,
        best_bid: None,
        best_ask: None,
        spread: None,
        midpoint: None,
        bids: book
            .bids
            .into_iter()
            .map(|level| PriceLevel {
                price: level.price.to_string(),
                size: level.size.to_string(),
            })
            .collect(),
        asks: book
            .asks
            .into_iter()
            .map(|level| PriceLevel {
                price: level.price.to_string(),
                size: level.size.to_string(),
            })
            .collect(),
    };
    normalize_book(&mut book);
    book
}

fn rest_live_book(book: OrderBookSummaryResponse, received_at_ms: u64) -> LiveOrderBook {
    let mut book = LiveOrderBook {
        source: "rest_seed".to_owned(),
        token_id: book.asset_id.to_string(),
        condition_id: book.market.to_string(),
        upstream_timestamp_ms: book.timestamp.timestamp_millis(),
        received_at_ms,
        feed_age_ms: Some(
            received_at_ms.saturating_sub(book.timestamp.timestamp_millis().max(0) as u64),
        ),
        hash: book.hash,
        best_bid: None,
        best_ask: None,
        spread: None,
        midpoint: None,
        bids: book
            .bids
            .into_iter()
            .map(|level| PriceLevel {
                price: level.price.to_string(),
                size: level.size.to_string(),
            })
            .collect(),
        asks: book
            .asks
            .into_iter()
            .map(|level| PriceLevel {
                price: level.price.to_string(),
                size: level.size.to_string(),
            })
            .collect(),
    };
    normalize_book(&mut book);
    book
}

fn validate_endpoint(endpoint: &str) -> Result<(), AppError> {
    let url = Url::parse(endpoint).map_err(|error| AppError::ClientInitialization {
        service: "CLOB WebSocket",
        message: error.to_string(),
    })?;
    if !matches!(url.scheme(), "ws" | "wss") || url.host_str().is_none() {
        return Err(AppError::ClientInitialization {
            service: "CLOB WebSocket",
            message: "POLYMARKET_WS_URL must be an absolute ws:// or wss:// URL".to_owned(),
        });
    }
    Ok(())
}

fn configured_socks_proxy() -> Result<Option<String>, AppError> {
    if let Ok(value) = std::env::var("POLYMARKET_WS_PROXY") {
        return parse_socks_proxy(&value).map(Some);
    }
    for name in ["ALL_PROXY", "all_proxy"] {
        if let Ok(value) = std::env::var(name)
            && (value.starts_with("socks5://") || value.starts_with("socks5h://"))
        {
            return parse_socks_proxy(&value).map(Some);
        }
    }
    Ok(None)
}

fn parse_socks_proxy(value: &str) -> Result<String, AppError> {
    let authority = value
        .strip_prefix("socks5://")
        .or_else(|| value.strip_prefix("socks5h://"))
        .unwrap_or(value)
        .trim_end_matches('/');
    if authority.is_empty() || authority.contains('@') || authority.contains('/') {
        return Err(AppError::ClientInitialization {
            service: "CLOB WebSocket",
            message: "POLYMARKET_WS_PROXY must be an unauthenticated socks5://host:port URL"
                .to_owned(),
        });
    }
    Ok(authority.to_owned())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_delta_updates_and_removes_levels() {
        let mut levels = vec![PriceLevel {
            price: "0.5".to_owned(),
            size: "10".to_owned(),
        }];
        apply_level(&mut levels, "0.5".parse().unwrap(), "7".parse().unwrap());
        assert_eq!(levels[0].size, "7");
        apply_level(&mut levels, "0.5".parse().unwrap(), Decimal::ZERO);
        assert!(levels.is_empty());
    }

    #[test]
    fn missing_delta_size_does_not_delete_a_level() {
        let mut levels = vec![PriceLevel {
            price: "0.5".to_owned(),
            size: "10".to_owned(),
        }];
        assert!(!apply_optional_level(
            &mut levels,
            "0.5".parse().unwrap(),
            None
        ));
        assert_eq!(levels[0].size, "10");
    }

    #[test]
    fn websocket_decoder_accepts_initial_array() {
        let messages = decode_messages(
            r#"[{"event_type":"book","asset_id":"1","market":"0x0000000000000000000000000000000000000000000000000000000000000000","timestamp":"1","bids":[],"asks":[],"hash":null}]"#,
        )
        .unwrap();
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn explicit_proxy_is_validated() {
        assert_eq!(
            parse_socks_proxy("socks5://127.0.0.1:1080").unwrap(),
            "127.0.0.1:1080"
        );
        assert!(parse_socks_proxy("socks5://user:pass@example.com:1080").is_err());
    }
}
