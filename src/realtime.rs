use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use futures::StreamExt as _;
use polymarket_client_sdk_v2::{
    clob::{
        types::response::OrderBookSummaryResponse,
        ws::{BookUpdate, ChannelType, Client as WsClient},
    },
    types::{Decimal, U256},
    ws::config::Config as WsConfig,
};
use tokio::sync::{RwLock, broadcast, watch};

use crate::{
    error::AppError,
    types::{
        LiveOrderBook, LiveSnapshotOutput, PriceLevel, RealtimeStatusOutput, StopWatchOutput,
        WatchInfo,
    },
};

pub const CLOB_WS_ENDPOINT: &str = "wss://ws-subscriptions-clob.polymarket.com";

#[derive(Clone, Debug)]
pub struct RealtimeUpdate {
    pub watch_id: String,
    pub book: LiveOrderBook,
}

#[derive(Clone)]
pub struct RealtimeService {
    client: WsClient,
    watches: Arc<RwLock<HashMap<String, WatchState>>>,
    next_id: Arc<AtomicU64>,
    updates: broadcast::Sender<RealtimeUpdate>,
}

impl fmt::Debug for RealtimeService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RealtimeService")
            .field("endpoint", &CLOB_WS_ENDPOINT)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct WatchState {
    token_ids: Vec<String>,
    started_at_ms: u64,
    last_update_at_ms: Option<u64>,
    update_count: u64,
    error_count: u64,
    last_error: Option<String>,
    books: HashMap<String, LiveOrderBook>,
    cancel: watch::Sender<bool>,
}

impl RealtimeService {
    pub fn new() -> Result<Self, AppError> {
        let client = WsClient::new(CLOB_WS_ENDPOINT, WsConfig::default()).map_err(|error| {
            AppError::ClientInitialization {
                service: "CLOB WebSocket",
                message: error.to_string(),
            }
        })?;
        Ok(Self {
            client,
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
        let stream = self
            .client
            .subscribe_orderbook(token_ids.clone())
            .map_err(|error| AppError::Upstream {
                service: "CLOB WebSocket",
                message: error.to_string(),
            })?;
        let watch_id = format!("watch-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let token_id_strings = token_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let (cancel, mut cancel_rx) = watch::channel(false);
        let seeded_at = now_ms();
        let books = initial_books
            .into_iter()
            .map(|book| {
                let book = rest_live_book(book, seeded_at);
                (book.token_id.clone(), book)
            })
            .collect::<HashMap<_, _>>();
        let state = WatchState {
            token_ids: token_id_strings,
            started_at_ms: now_ms(),
            last_update_at_ms: (!books.is_empty()).then_some(seeded_at),
            update_count: books.len() as u64,
            error_count: 0,
            last_error: None,
            books,
            cancel,
        };
        let info = watch_info(&watch_id, &state);
        self.watches.write().await.insert(watch_id.clone(), state);

        let watches = Arc::clone(&self.watches);
        let updates = self.updates.clone();
        let task_watch_id = watch_id;
        tokio::spawn(async move {
            let mut stream = Box::pin(stream);
            loop {
                tokio::select! {
                    changed = cancel_rx.changed() => {
                        if changed.is_err() || *cancel_rx.borrow() {
                            break;
                        }
                    }
                    message = stream.next() => {
                        match message {
                            Some(Ok(book)) => {
                                let received_at_ms = now_ms();
                                let book = live_book(book, received_at_ms);
                                let mut states = watches.write().await;
                                let Some(state) = states.get_mut(&task_watch_id) else { break };
                                state.last_update_at_ms = Some(received_at_ms);
                                state.update_count += 1;
                                state.last_error = None;
                                let _ = updates.send(RealtimeUpdate {
                                    watch_id: task_watch_id.clone(),
                                    book: book.clone(),
                                });
                                state.books.insert(book.token_id.clone(), book);
                            }
                            Some(Err(error)) => {
                                let mut states = watches.write().await;
                                let Some(state) = states.get_mut(&task_watch_id) else { break };
                                state.error_count += 1;
                                state.last_error = Some(error.to_string());
                            }
                            None => {
                                let mut states = watches.write().await;
                                if let Some(state) = states.get_mut(&task_watch_id) {
                                    state.error_count += 1;
                                    state.last_error = Some("websocket subscription ended".to_owned());
                                }
                                break;
                            }
                        }
                    }
                }
            }
        });

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
        RealtimeStatusOutput {
            connection_state: format!("{:?}", self.client.connection_state(ChannelType::Market)),
            active_watch_count: states.len(),
            subscription_count: self.client.subscription_count(),
            watches,
        }
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
}

fn watch_info(watch_id: &str, state: &WatchState) -> WatchInfo {
    WatchInfo {
        watch_id: watch_id.to_owned(),
        token_ids: state.token_ids.clone(),
        started_at_ms: state.started_at_ms,
        last_update_at_ms: state.last_update_at_ms,
        update_count: state.update_count,
        error_count: state.error_count,
        last_error: state.last_error.clone(),
        snapshot_count: state.books.len(),
    }
}

fn live_book(book: BookUpdate, received_at_ms: u64) -> LiveOrderBook {
    let best_bid = book.bids.iter().map(|level| level.price).max();
    let best_ask = book.asks.iter().map(|level| level.price).min();
    let mut bids = book.bids;
    bids.sort_by(|left, right| right.price.cmp(&left.price));
    let mut asks = book.asks;
    asks.sort_by(|left, right| left.price.cmp(&right.price));
    LiveOrderBook {
        source: "websocket".to_owned(),
        token_id: book.asset_id.to_string(),
        condition_id: book.market.to_string(),
        upstream_timestamp_ms: book.timestamp,
        received_at_ms,
        feed_age_ms: Some(received_at_ms.saturating_sub(book.timestamp.max(0) as u64)),
        hash: book.hash,
        best_bid: best_bid.map(|value| value.to_string()),
        best_ask: best_ask.map(|value| value.to_string()),
        spread: best_bid
            .zip(best_ask)
            .map(|(bid, ask)| (ask - bid).to_string()),
        midpoint: best_bid
            .zip(best_ask)
            .map(|(bid, ask)| ((bid + ask) / Decimal::TWO).to_string()),
        bids: bids
            .into_iter()
            .map(|level| PriceLevel {
                price: level.price.to_string(),
                size: level.size.to_string(),
            })
            .collect(),
        asks: asks
            .into_iter()
            .map(|level| PriceLevel {
                price: level.price.to_string(),
                size: level.size.to_string(),
            })
            .collect(),
    }
}

fn rest_live_book(book: OrderBookSummaryResponse, received_at_ms: u64) -> LiveOrderBook {
    let best_bid = book.bids.iter().map(|level| level.price).max();
    let best_ask = book.asks.iter().map(|level| level.price).min();
    let mut bids = book.bids;
    bids.sort_by(|left, right| right.price.cmp(&left.price));
    let mut asks = book.asks;
    asks.sort_by(|left, right| left.price.cmp(&right.price));
    LiveOrderBook {
        source: "rest_seed".to_owned(),
        token_id: book.asset_id.to_string(),
        condition_id: book.market.to_string(),
        upstream_timestamp_ms: book.timestamp.timestamp_millis(),
        received_at_ms,
        feed_age_ms: Some(
            received_at_ms.saturating_sub(book.timestamp.timestamp_millis().max(0) as u64),
        ),
        hash: book.hash,
        best_bid: best_bid.map(|value| value.to_string()),
        best_ask: best_ask.map(|value| value.to_string()),
        spread: best_bid
            .zip(best_ask)
            .map(|(bid, ask)| (ask - bid).to_string()),
        midpoint: best_bid
            .zip(best_ask)
            .map(|(bid, ask)| ((bid + ask) / Decimal::TWO).to_string()),
        bids: bids
            .into_iter()
            .map(|level| PriceLevel {
                price: level.price.to_string(),
                size: level.size.to_string(),
            })
            .collect(),
        asks: asks
            .into_iter()
            .map(|level| PriceLevel {
                price: level.price.to_string(),
                size: level.size.to_string(),
            })
            .collect(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
