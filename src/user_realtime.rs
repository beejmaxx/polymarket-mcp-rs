use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use futures::StreamExt as _;
use polymarket_client_sdk_v2::{
    auth::Credentials,
    clob::{
        types::TraderSide,
        ws::{
            ChannelType, Client as WsClient, OrderMessage, TradeMessage, WsMessage,
            types::response::TradeMessageStatus,
        },
    },
    types::{Address, B256},
    ws::connection::ConnectionState,
};
use tokio::sync::{RwLock, watch};

use crate::{
    error::AppError,
    types::{
        StopWatchOutput, UserRealtimeEvent, UserRealtimeEventsOutput, UserRealtimeStatusOutput,
        UserWatchInfo,
    },
};

const MAX_USER_WATCHES: usize = 4;
const MAX_RETAINED_EVENTS: usize = 1_024;

#[derive(Clone, Debug)]
pub struct UserRealtimeService {
    watches: Arc<RwLock<HashMap<String, UserWatchState>>>,
    next_id: Arc<AtomicU64>,
}

#[derive(Debug)]
struct UserWatchState {
    condition_ids: Vec<String>,
    started_at_ms: u64,
    last_event_at_ms: Option<u64>,
    connection_state: String,
    event_count: u64,
    dropped_event_count: u64,
    error_count: u64,
    last_error: Option<String>,
    next_sequence: u64,
    recent_events: VecDeque<UserRealtimeEvent>,
    cancel: watch::Sender<bool>,
}

impl UserRealtimeService {
    #[must_use]
    pub fn new() -> Self {
        Self {
            watches: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    pub async fn watch(
        &self,
        credentials: Credentials,
        address: Address,
        mut condition_ids: Vec<B256>,
    ) -> Result<UserWatchInfo, AppError> {
        condition_ids.sort_unstable();
        condition_ids.dedup();
        if !(1..=50).contains(&condition_ids.len()) {
            return Err(AppError::InvalidInput(
                "condition_ids must contain between 1 and 50 unique IDs".to_owned(),
            ));
        }
        let condition_id_strings = condition_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        {
            let states = self.watches.read().await;
            if let Some((watch_id, state)) = states
                .iter()
                .find(|(_, state)| state.condition_ids == condition_id_strings)
            {
                return Ok(watch_info(watch_id, state));
            }
            if states.len() >= MAX_USER_WATCHES {
                return Err(AppError::InvalidInput(format!(
                    "at most {MAX_USER_WATCHES} authenticated user watches are allowed"
                )));
            }
        }

        let client = WsClient::default()
            .authenticate(credentials, address)
            .map_err(user_ws_error)?;
        let stream = client
            .subscribe_user_events(condition_ids)
            .map_err(user_ws_error)?;
        let watch_id = format!(
            "user-watch-{}",
            self.next_id.fetch_add(1, Ordering::Relaxed)
        );
        let (cancel, mut cancel_rx) = watch::channel(false);
        let state = UserWatchState {
            condition_ids: condition_id_strings.clone(),
            started_at_ms: now_ms(),
            last_event_at_ms: None,
            connection_state: "Connecting".to_owned(),
            event_count: 0,
            dropped_event_count: 0,
            error_count: 0,
            last_error: None,
            next_sequence: 1,
            recent_events: VecDeque::with_capacity(MAX_RETAINED_EVENTS),
            cancel,
        };
        let info = watch_info(&watch_id, &state);
        {
            let mut states = self.watches.write().await;
            if let Some((existing_id, existing)) = states
                .iter()
                .find(|(_, existing)| existing.condition_ids == condition_id_strings)
            {
                return Ok(watch_info(existing_id, existing));
            }
            if states.len() >= MAX_USER_WATCHES {
                return Err(AppError::InvalidInput(format!(
                    "at most {MAX_USER_WATCHES} authenticated user watches are allowed"
                )));
            }
            states.insert(watch_id.clone(), state);
        }

        let watches = Arc::clone(&self.watches);
        tokio::spawn(async move {
            let mut stream = Box::pin(stream);
            let mut connection_poll = tokio::time::interval(std::time::Duration::from_millis(500));
            loop {
                tokio::select! {
                    changed = cancel_rx.changed() => {
                        if changed.is_err() || *cancel_rx.borrow() {
                            break;
                        }
                    }
                    _ = connection_poll.tick() => {
                        let state = connection_state(client.connection_state(ChannelType::User));
                        set_state(&watches, &watch_id, state).await;
                    }
                    message = stream.next() => {
                        match message {
                            Some(Ok(message)) => push_message(&watches, &watch_id, message).await,
                            Some(Err(error)) => record_error(&watches, &watch_id, error.to_string()).await,
                            None => {
                                set_state(&watches, &watch_id, "Disconnected").await;
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(info)
    }

    pub async fn events(
        &self,
        watch_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<UserRealtimeEventsOutput, AppError> {
        let states = self.watches.read().await;
        let state = states
            .get(watch_id)
            .ok_or_else(|| AppError::InvalidInput(format!("unknown user watch_id {watch_id}")))?;
        let events = state
            .recent_events
            .iter()
            .filter(|event| event.sequence > after_sequence)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_sequence = events.last().map_or(after_sequence, |event| event.sequence);
        let oldest_available_sequence = state.recent_events.front().map(|event| event.sequence);
        Ok(UserRealtimeEventsOutput {
            watch: watch_info(watch_id, state),
            after_sequence,
            next_sequence,
            oldest_available_sequence,
            truncated_before: oldest_available_sequence
                .is_some_and(|oldest| after_sequence.saturating_add(1) < oldest),
            events,
        })
    }

    pub async fn status(&self) -> UserRealtimeStatusOutput {
        let states = self.watches.read().await;
        let mut watches = states
            .iter()
            .map(|(watch_id, state)| watch_info(watch_id, state))
            .collect::<Vec<_>>();
        watches.sort_by(|left, right| left.watch_id.cmp(&right.watch_id));
        UserRealtimeStatusOutput {
            active_watch_count: watches.len(),
            watches,
        }
    }

    pub async fn stop(&self, watch_id: &str) -> StopWatchOutput {
        let state = self.watches.write().await.remove(watch_id);
        let stopped = if let Some(state) = state {
            let _ = state.cancel.send(true);
            true
        } else {
            false
        };
        StopWatchOutput {
            watch_id: watch_id.to_owned(),
            stopped,
        }
    }

    pub async fn stop_all(&self) -> usize {
        let mut states = self.watches.write().await;
        let count = states.len();
        for (_, state) in states.drain() {
            let _ = state.cancel.send(true);
        }
        count
    }
}

impl Default for UserRealtimeService {
    fn default() -> Self {
        Self::new()
    }
}

async fn push_message(
    watches: &RwLock<HashMap<String, UserWatchState>>,
    watch_id: &str,
    message: WsMessage,
) {
    let mut event = match message {
        WsMessage::Order(order) => user_order_event(order),
        WsMessage::Trade(trade) => user_trade_event(trade),
        _ => return,
    };
    let mut states = watches.write().await;
    let Some(state) = states.get_mut(watch_id) else {
        return;
    };
    event.sequence = state.next_sequence;
    state.next_sequence += 1;
    state.event_count += 1;
    state.last_event_at_ms = Some(now_ms());
    state.last_error = None;
    if state.recent_events.len() == MAX_RETAINED_EVENTS {
        state.recent_events.pop_front();
        state.dropped_event_count += 1;
    }
    state.recent_events.push_back(event);
}

async fn record_error(
    watches: &RwLock<HashMap<String, UserWatchState>>,
    watch_id: &str,
    error: String,
) {
    if let Some(state) = watches.write().await.get_mut(watch_id) {
        state.error_count += 1;
        state.last_error = Some(error);
        state.connection_state = "Error".to_owned();
    }
}

async fn set_state(
    watches: &RwLock<HashMap<String, UserWatchState>>,
    watch_id: &str,
    connection_state: &str,
) {
    if let Some(state) = watches.write().await.get_mut(watch_id) {
        state.connection_state = connection_state.to_owned();
    }
}

fn user_order_event(message: OrderMessage) -> UserRealtimeEvent {
    UserRealtimeEvent {
        sequence: 0,
        event_type: "order".to_owned(),
        timestamp: message.timestamp,
        condition_id: message.market.to_string(),
        token_id: message.asset_id.to_string(),
        id: message.id,
        side: message.side.to_string(),
        price: message.price.to_string(),
        size: message.original_size.map(|value| value.to_string()),
        matched_size: message.size_matched.map(|value| value.to_string()),
        status: message.status.map(|value| value.to_string()),
        outcome: message.outcome,
        transaction_hash: None,
        trader_side: None,
        taker_order_id: None,
        associated_trade_ids: message.associate_trades.unwrap_or_default(),
    }
}

fn user_trade_event(message: TradeMessage) -> UserRealtimeEvent {
    UserRealtimeEvent {
        sequence: 0,
        event_type: "trade".to_owned(),
        timestamp: message
            .timestamp
            .or(message.last_update)
            .or(message.matchtime),
        condition_id: message.market.to_string(),
        token_id: message.asset_id.to_string(),
        id: message.id,
        side: message.side.to_string(),
        price: message.price.to_string(),
        size: Some(message.size.to_string()),
        matched_size: None,
        status: Some(trade_status(&message.status)),
        outcome: message.outcome,
        transaction_hash: message.transaction_hash.map(|value| value.to_string()),
        trader_side: message.trader_side.map(trader_side),
        taker_order_id: message.taker_order_id,
        associated_trade_ids: message
            .maker_orders
            .into_iter()
            .map(|order| order.order_id)
            .collect(),
    }
}

fn trade_status(status: &TradeMessageStatus) -> String {
    match status {
        TradeMessageStatus::Matched => "MATCHED".to_owned(),
        TradeMessageStatus::Mined => "MINED".to_owned(),
        TradeMessageStatus::Confirmed => "CONFIRMED".to_owned(),
        TradeMessageStatus::Retrying => "RETRYING".to_owned(),
        TradeMessageStatus::Failed => "FAILED".to_owned(),
        TradeMessageStatus::Unknown(value) => value.clone(),
        _ => "UNKNOWN".to_owned(),
    }
}

fn trader_side(side: TraderSide) -> String {
    match side {
        TraderSide::Taker => "TAKER".to_owned(),
        TraderSide::Maker => "MAKER".to_owned(),
        TraderSide::Unknown(value) => value,
        _ => "UNKNOWN".to_owned(),
    }
}

fn watch_info(watch_id: &str, state: &UserWatchState) -> UserWatchInfo {
    UserWatchInfo {
        watch_id: watch_id.to_owned(),
        condition_ids: state.condition_ids.clone(),
        started_at_ms: state.started_at_ms,
        last_event_at_ms: state.last_event_at_ms,
        connection_state: state.connection_state.clone(),
        event_count: state.event_count,
        retained_event_count: state.recent_events.len(),
        dropped_event_count: state.dropped_event_count,
        error_count: state.error_count,
        last_error: state.last_error.clone(),
    }
}

fn connection_state(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::Disconnected => "Disconnected",
        ConnectionState::Connecting => "Connecting",
        ConnectionState::Connected { .. } => "Connected",
        ConnectionState::Reconnecting { .. } => "Reconnecting",
        _ => "Unknown",
    }
}

fn user_ws_error(error: impl ToString) -> AppError {
    AppError::Upstream {
        service: "CLOB user WebSocket",
        message: error.to_string(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}
