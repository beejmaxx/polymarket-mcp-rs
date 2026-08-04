use std::{collections::HashMap, path::PathBuf, str::FromStr};

use polymarket_client_sdk_v2::{
    clob::types::{Interval, TimeRange, response::OrderBookSummaryResponse},
    data::types::response::{Activity, Position, Trade},
    gamma::types::request::EventsRequest,
    gamma::types::response::{Event, Market},
    types::{Address, B256, Decimal, U256},
};

use crate::{
    error::AppError,
    polymarket::{CLOB_V2_ENDPOINT, DATA_ENDPOINT, GAMMA_ENDPOINT, PolymarketClient},
    realtime::RealtimeService,
    recorder::RecorderService,
    trading::{PreviewParameters, TradingService},
    types::{
        AccountTradesOutput, BalanceAllowanceOutput, BatchPlacedOrdersOutput, CancelOrdersOutput,
        CompareMarketsOutput, EventDetail, HolderDetail, ListMarketsInput, ListMarketsOutput,
        ListRecordingsOutput, LiveSnapshotOutput, MarketDetail, MarketHoldersOutput, MarketSummary,
        OpenOrder, OpenOrdersOutput, OrderApprovalStatusOutput, OrderBookDetail, OrderBookSnapshot,
        OrderPreviewOutput, OutcomeDetail, OutcomeHolders, OutcomeQuote, PlacedOrderOutput,
        PreviewOrderInput, PriceHistoryOutput, PriceLevel, PricePoint, RealtimeEventsOutput,
        RealtimeStatusOutput, RecordingInfo, ReplayMarketOutput, SearchMarketsOutput, ServerStatus,
        SimulationOutput, StopWatchOutput, TagSummary, TradingStatusOutput, WalletActivity,
        WalletActivityOutput, WalletPosition, WalletPositionsOutput, WalletRiskOutput, WalletTrade,
        WalletTradesOutput, WalletValueOutput, WatchInfo,
    },
};

const DEFAULT_SEARCH_LIMIT: u8 = 10;
const MAX_SEARCH_LIMIT: u8 = 25;

#[derive(Clone, Debug)]
pub struct App {
    client: PolymarketClient,
    realtime: RealtimeService,
    recorder: RecorderService,
    trading: TradingService,
}

impl App {
    pub fn new() -> Result<Self, AppError> {
        let database_path = std::env::var_os("POLYMARKET_MCP_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("polymarket-mcp.sqlite3"));
        Self::new_with_database(database_path)
    }

    pub fn new_with_database(database_path: PathBuf) -> Result<Self, AppError> {
        let trading_database_path = database_path.clone();
        Ok(Self {
            client: PolymarketClient::new()?,
            realtime: RealtimeService::new()?,
            recorder: RecorderService::new(database_path)?,
            trading: TradingService::from_environment(trading_database_path)?,
        })
    }

    #[must_use]
    pub fn status(&self) -> ServerStatus {
        ServerStatus {
            name: env!("CARGO_PKG_NAME").to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            mode: if self.trading.status().enabled {
                "trading-enabled".to_owned()
            } else {
                "read-only".to_owned()
            },
            clob_protocol: "v2".to_owned(),
            gamma_endpoint: GAMMA_ENDPOINT.to_owned(),
            clob_endpoint: CLOB_V2_ENDPOINT.to_owned(),
            data_endpoint: DATA_ENDPOINT.to_owned(),
            websocket_endpoint: self.realtime.endpoint().to_owned(),
            database_path: self.recorder.path().display().to_string(),
        }
    }

    pub async fn search_markets(
        &self,
        query: String,
        limit: Option<u8>,
    ) -> Result<SearchMarketsOutput, AppError> {
        let query = validate_nonempty("query", query)?;
        let limit = limit
            .unwrap_or(DEFAULT_SEARCH_LIMIT)
            .clamp(1, MAX_SEARCH_LIMIT);
        let results = self.client.search(&query, i32::from(limit)).await?;

        let mut markets = results
            .events
            .unwrap_or_default()
            .iter()
            .flat_map(market_summaries)
            .collect::<Vec<_>>();

        markets.sort_by(|left, right| {
            right
                .volume_24h
                .as_deref()
                .and_then(|value| value.parse::<Decimal>().ok())
                .cmp(
                    &left
                        .volume_24h
                        .as_deref()
                        .and_then(|value| value.parse::<Decimal>().ok()),
                )
        });
        markets.truncate(usize::from(limit));

        Ok(SearchMarketsOutput {
            query,
            count: markets.len(),
            markets,
        })
    }

    pub async fn get_market(&self, market_id: String) -> Result<MarketDetail, AppError> {
        let market_id = validate_nonempty("market_id", market_id)?;
        let market = self.client.market(&market_id).await?;
        self.enrich_market(market).await
    }

    pub async fn get_market_by_identifier(
        &self,
        market_id: Option<String>,
        slug: Option<String>,
        condition_id: Option<String>,
    ) -> Result<MarketDetail, AppError> {
        let present = usize::from(market_id.is_some())
            + usize::from(slug.is_some())
            + usize::from(condition_id.is_some());
        if present != 1 {
            return Err(AppError::InvalidInput(
                "provide exactly one of market_id, slug, or condition_id".to_owned(),
            ));
        }
        let market = if let Some(id) = market_id {
            self.client
                .market(&validate_nonempty("market_id", id)?)
                .await?
        } else if let Some(slug) = slug {
            self.client
                .market_by_slug(&validate_nonempty("slug", slug)?)
                .await?
        } else {
            self.client
                .market_by_condition(parse_b256("condition_id", &condition_id.unwrap())?)
                .await?
        };
        self.enrich_market(market).await
    }

    async fn enrich_market(&self, market: Market) -> Result<MarketDetail, AppError> {
        let token_ids = market.clob_token_ids.clone().unwrap_or_default();
        let books = if market.enable_order_book.unwrap_or(false) && !market.closed.unwrap_or(false)
        {
            self.client.order_books(&token_ids).await?
        } else {
            Vec::new()
        };

        Ok(market_detail(&market, books))
    }

    pub async fn list_markets(
        &self,
        input: ListMarketsInput,
    ) -> Result<ListMarketsOutput, AppError> {
        let limit = input.limit.unwrap_or(25).clamp(1, 100);
        let offset = i32::try_from(input.offset.unwrap_or(0))
            .map_err(|_| AppError::InvalidInput("offset is too large".to_owned()))?;
        let requested_sort = input.sort_by.unwrap_or_else(|| "volume_24h".to_owned());
        let sort_by = match requested_sort.as_str() {
            "volume_24h" | "volume24hr" => "volume24hr",
            "volume" => "volume",
            "liquidity" => "liquidity",
            "start_date" | "startDate" => "startDate",
            "end_date" | "endDate" => "endDate",
            _ => {
                return Err(AppError::InvalidInput(format!(
                    "unsupported sort_by value {requested_sort}"
                )));
            }
        };
        let tag_slug = input
            .tag_slug
            .map(|value| validate_nonempty("tag_slug", value))
            .transpose()?;
        let min_liquidity = input
            .min_liquidity
            .as_deref()
            .map(|value| parse_decimal("min_liquidity", value))
            .transpose()?;
        let min_volume = input
            .min_volume
            .as_deref()
            .map(|value| parse_decimal("min_volume", value))
            .transpose()?;
        let request = EventsRequest::builder()
            .limit(i32::from(limit))
            .offset(offset)
            .order(vec![sort_by.to_owned()])
            .ascending(input.ascending.unwrap_or(false))
            .active(true)
            .closed(false)
            .maybe_tag_slug(tag_slug)
            .maybe_featured(input.featured)
            .maybe_liquidity_min(min_liquidity)
            .maybe_volume_min(min_volume)
            .build();
        let events = self.client.events(&request).await?;
        let markets = events
            .iter()
            .flat_map(market_summaries)
            .filter(|market| {
                market.active == Some(true)
                    && market.closed != Some(true)
                    && market.accepting_orders == Some(true)
            })
            .take(usize::from(limit))
            .collect::<Vec<_>>();
        Ok(ListMarketsOutput {
            count: markets.len(),
            markets,
        })
    }

    pub async fn get_event(
        &self,
        event_id: Option<String>,
        slug: Option<String>,
    ) -> Result<EventDetail, AppError> {
        if event_id.is_some() == slug.is_some() {
            return Err(AppError::InvalidInput(
                "provide exactly one of event_id or slug".to_owned(),
            ));
        }
        let event = if let Some(id) = event_id {
            self.client
                .event_by_id(&validate_nonempty("event_id", id)?)
                .await?
        } else {
            self.client
                .event_by_slug(&validate_nonempty("slug", slug.unwrap())?)
                .await?
        };
        Ok(event_detail(&event))
    }

    pub async fn get_order_book(
        &self,
        token_id: String,
        depth: Option<u16>,
    ) -> Result<OrderBookDetail, AppError> {
        let token_id = parse_u256("token_id", &token_id)?;
        let depth = usize::from(depth.unwrap_or(20).clamp(1, 100));
        let book = self.client.order_book(token_id).await?;
        Ok(order_book_detail(book, depth))
    }

    pub async fn get_price_history(
        &self,
        token_id: String,
        interval: Option<String>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        fidelity: Option<u32>,
    ) -> Result<PriceHistoryOutput, AppError> {
        let token_id = parse_u256("token_id", &token_id)?;
        let time_range = match (interval, start_ts, end_ts) {
            (None, None, None) => TimeRange::from_interval(Interval::OneDay),
            (Some(value), None, None) => TimeRange::from_interval(parse_interval(&value)?),
            (None, Some(start), Some(end)) if start < end => TimeRange::from_range(start, end),
            (None, Some(_), Some(_)) => {
                return Err(AppError::InvalidInput(
                    "start_ts must be earlier than end_ts".to_owned(),
                ));
            }
            _ => {
                return Err(AppError::InvalidInput(
                    "use interval or provide both start_ts and end_ts, but not both".to_owned(),
                ));
            }
        };
        if fidelity == Some(0) {
            return Err(AppError::InvalidInput(
                "fidelity must be greater than zero".to_owned(),
            ));
        }
        let history = self
            .client
            .price_history(token_id, time_range, fidelity)
            .await?;
        let points = history
            .history
            .into_iter()
            .map(|point| PricePoint {
                timestamp: point.t,
                price: point.p.to_string(),
            })
            .collect::<Vec<_>>();
        Ok(PriceHistoryOutput {
            token_id: token_id.to_string(),
            point_count: points.len(),
            points,
        })
    }

    pub async fn get_market_holders(
        &self,
        condition_id: String,
        limit: Option<u8>,
        min_balance: Option<u32>,
    ) -> Result<MarketHoldersOutput, AppError> {
        let condition_id = parse_b256("condition_id", &condition_id)?;
        let limit = i32::from(limit.unwrap_or(20).min(20));
        let min_balance = i32::try_from(min_balance.unwrap_or(1).min(999_999)).unwrap();
        let outcomes = self
            .client
            .holders(condition_id, limit, min_balance)
            .await?
            .into_iter()
            .map(|outcome| OutcomeHolders {
                token_id: outcome.token.to_string(),
                holders: outcome
                    .holders
                    .into_iter()
                    .map(|holder| HolderDetail {
                        wallet: holder.proxy_wallet.to_string(),
                        amount: holder.amount.to_string(),
                        outcome_index: holder.outcome_index,
                        pseudonym: holder.pseudonym,
                        name: holder.name,
                        verified: holder.verified,
                    })
                    .collect(),
            })
            .collect();
        Ok(MarketHoldersOutput {
            condition_id: condition_id.to_string(),
            outcomes,
        })
    }

    pub async fn compare_markets(
        &self,
        market_ids: Vec<String>,
    ) -> Result<CompareMarketsOutput, AppError> {
        if !(2..=10).contains(&market_ids.len()) {
            return Err(AppError::InvalidInput(
                "market_ids must contain between 2 and 10 IDs".to_owned(),
            ));
        }
        let markets = futures::future::try_join_all(
            market_ids
                .into_iter()
                .map(|market_id| self.get_market(market_id)),
        )
        .await?;
        Ok(CompareMarketsOutput {
            count: markets.len(),
            markets,
        })
    }

    pub async fn get_wallet_positions(
        &self,
        wallet: String,
        limit: Option<u16>,
        offset: Option<u32>,
    ) -> Result<WalletPositionsOutput, AppError> {
        let wallet = parse_address("wallet", &wallet)?;
        let (limit, offset) = wallet_page(limit, offset);
        let positions = self.client.positions(wallet, limit, offset).await?;
        Ok(WalletPositionsOutput {
            wallet: wallet.to_string(),
            count: positions.len(),
            positions: positions.into_iter().map(wallet_position).collect(),
        })
    }

    pub async fn get_wallet_value(&self, wallet: String) -> Result<WalletValueOutput, AppError> {
        let wallet = parse_address("wallet", &wallet)?;
        let value = self
            .client
            .wallet_value(wallet)
            .await?
            .into_iter()
            .fold(Decimal::ZERO, |total, item| total + item.value);
        Ok(WalletValueOutput {
            wallet: wallet.to_string(),
            value_usdc: value.to_string(),
        })
    }

    pub async fn get_wallet_trades(
        &self,
        wallet: String,
        limit: Option<u16>,
        offset: Option<u32>,
    ) -> Result<WalletTradesOutput, AppError> {
        let wallet = parse_address("wallet", &wallet)?;
        let (limit, offset) = wallet_page(limit, offset);
        let trades = self.client.wallet_trades(wallet, limit, offset).await?;
        Ok(WalletTradesOutput {
            wallet: wallet.to_string(),
            count: trades.len(),
            trades: trades.into_iter().map(wallet_trade).collect(),
        })
    }

    pub async fn get_wallet_activity(
        &self,
        wallet: String,
        limit: Option<u16>,
        offset: Option<u32>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<WalletActivityOutput, AppError> {
        if matches!((start, end), (Some(start), Some(end)) if start > end) {
            return Err(AppError::InvalidInput(
                "start_ts must not be later than end_ts".to_owned(),
            ));
        }
        let wallet = parse_address("wallet", &wallet)?;
        let (limit, offset) = wallet_page(limit, offset);
        let activities = self
            .client
            .wallet_activity(wallet, limit, offset, start, end)
            .await?;
        Ok(WalletActivityOutput {
            wallet: wallet.to_string(),
            count: activities.len(),
            activities: activities.into_iter().map(wallet_activity).collect(),
        })
    }

    pub async fn analyze_wallet_risk(&self, wallet: String) -> Result<WalletRiskOutput, AppError> {
        let wallet = parse_address("wallet", &wallet)?;
        let raw_positions = self.client.positions(wallet, 500, 0).await?;
        let total_current = raw_positions
            .iter()
            .fold(Decimal::ZERO, |sum, item| sum + item.current_value);
        let total_initial = raw_positions
            .iter()
            .fold(Decimal::ZERO, |sum, item| sum + item.initial_value);
        let total_unrealized = raw_positions
            .iter()
            .fold(Decimal::ZERO, |sum, item| sum + item.cash_pnl);
        let total_realized = raw_positions
            .iter()
            .fold(Decimal::ZERO, |sum, item| sum + item.realized_pnl);
        let largest = raw_positions
            .iter()
            .map(|item| item.current_value)
            .max()
            .unwrap_or(Decimal::ZERO);
        let largest_share = if total_current > Decimal::ZERO {
            Some((largest / total_current * Decimal::from(100)).to_string())
        } else {
            None
        };
        let redeemable_position_count = raw_positions.iter().filter(|item| item.redeemable).count();
        let negative_risk_position_count = raw_positions
            .iter()
            .filter(|item| item.negative_risk)
            .count();
        let positions = raw_positions
            .into_iter()
            .map(wallet_position)
            .collect::<Vec<_>>();
        Ok(WalletRiskOutput {
            wallet: wallet.to_string(),
            position_count: positions.len(),
            total_current_value: total_current.to_string(),
            total_initial_value: total_initial.to_string(),
            total_unrealized_pnl: total_unrealized.to_string(),
            total_realized_pnl: total_realized.to_string(),
            largest_position_value: largest.to_string(),
            largest_position_share_percent: largest_share,
            redeemable_position_count,
            negative_risk_position_count,
            positions,
            methodology: "Aggregates up to 500 open positions returned by the public Data API; concentration is largest current position value divided by total current value. This is descriptive exposure data, not financial advice.".to_owned(),
        })
    }

    pub async fn watch_markets(&self, token_ids: Vec<String>) -> Result<WatchInfo, AppError> {
        if !(1..=50).contains(&token_ids.len()) {
            return Err(AppError::InvalidInput(
                "token_ids must contain between 1 and 50 IDs".to_owned(),
            ));
        }
        let mut parsed = Vec::with_capacity(token_ids.len());
        for token_id in token_ids {
            let token_id = parse_u256("token_id", &token_id)?;
            if !parsed.contains(&token_id) {
                parsed.push(token_id);
            }
        }
        let initial_books = self.client.order_books(&parsed).await?;
        self.realtime.watch_markets(parsed, initial_books).await
    }

    pub async fn get_live_snapshot(
        &self,
        watch_id: String,
        depth: Option<u16>,
    ) -> Result<LiveSnapshotOutput, AppError> {
        let watch_id = validate_nonempty("watch_id", watch_id)?;
        self.realtime
            .snapshot(&watch_id, usize::from(depth.unwrap_or(20).clamp(1, 100)))
            .await
    }

    pub async fn get_realtime_status(&self) -> RealtimeStatusOutput {
        self.realtime.status().await
    }

    pub async fn get_realtime_events(
        &self,
        watch_id: String,
        after_sequence: Option<u64>,
        limit: Option<u16>,
    ) -> Result<RealtimeEventsOutput, AppError> {
        let watch_id = validate_nonempty("watch_id", watch_id)?;
        self.realtime
            .events(
                &watch_id,
                after_sequence.unwrap_or(0),
                usize::from(limit.unwrap_or(100).clamp(1, 500)),
            )
            .await
    }

    pub async fn stop_watching(&self, watch_id: String) -> Result<StopWatchOutput, AppError> {
        let watch_id = validate_nonempty("watch_id", watch_id)?;
        Ok(self.realtime.stop(&watch_id).await)
    }

    pub async fn start_recording(
        &self,
        watch_id: String,
        label: Option<String>,
    ) -> Result<RecordingInfo, AppError> {
        let watch_id = validate_nonempty("watch_id", watch_id)?;
        let label = label
            .map(|value| validate_nonempty("label", value))
            .transpose()?;
        let updates = self.realtime.subscribe_updates();
        let initial = self.realtime.snapshot(&watch_id, 10_000).await?;
        self.recorder.start(watch_id, label, initial, updates).await
    }

    pub async fn stop_recording(&self, recording_id: String) -> Result<RecordingInfo, AppError> {
        let recording_id = validate_nonempty("recording_id", recording_id)?;
        self.recorder.stop(&recording_id).await
    }

    pub async fn list_recordings(
        &self,
        limit: Option<u16>,
    ) -> Result<ListRecordingsOutput, AppError> {
        let recorder = self.recorder.clone();
        let limit = limit.unwrap_or(50).clamp(1, 200);
        tokio::task::spawn_blocking(move || recorder.list(limit))
            .await
            .map_err(database_task_error)?
    }

    pub async fn replay_market(
        &self,
        recording_id: String,
        token_id: Option<String>,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
        limit: Option<u32>,
    ) -> Result<ReplayMarketOutput, AppError> {
        let recording_id = validate_nonempty("recording_id", recording_id)?;
        if matches!((start_ms, end_ms), (Some(start), Some(end)) if start > end) {
            return Err(AppError::InvalidInput(
                "start_ms must not be later than end_ms".to_owned(),
            ));
        }
        let token_id = token_id
            .map(|value| parse_u256("token_id", &value).map(|id| id.to_string()))
            .transpose()?;
        let recorder = self.recorder.clone();
        let limit = limit.unwrap_or(100).clamp(1, 5_000);
        tokio::task::spawn_blocking(move || {
            recorder.replay(&recording_id, token_id.as_deref(), start_ms, end_ms, limit)
        })
        .await
        .map_err(database_task_error)?
    }

    pub async fn simulate_order(
        &self,
        token_id: String,
        side: String,
        shares: String,
    ) -> Result<SimulationOutput, AppError> {
        let token_id = parse_u256("token_id", &token_id)?;
        let side = side.trim().to_ascii_lowercase();
        if !["buy", "sell"].contains(&side.as_str()) {
            return Err(AppError::InvalidInput(
                "side must be buy or sell".to_owned(),
            ));
        }
        let requested = parse_decimal("shares", &shares)?;
        if requested <= Decimal::ZERO {
            return Err(AppError::InvalidInput(
                "shares must be greater than zero".to_owned(),
            ));
        }
        let book = self.client.order_book(token_id).await?;
        Ok(simulate_book(&book, &side, requested))
    }

    #[must_use]
    pub fn trading_status(&self) -> TradingStatusOutput {
        self.trading.status()
    }

    pub async fn preview_order(
        &self,
        input: PreviewOrderInput,
    ) -> Result<OrderPreviewOutput, AppError> {
        let token_id = parse_u256("token_id", &input.token_id)?;
        let amount = parse_decimal("amount", &input.amount)?;
        let price = input
            .price
            .as_deref()
            .map(|value| parse_decimal("price", value))
            .transpose()?;
        let market_rules = if input.live_validation.unwrap_or(true) {
            let book = self.client.order_book(token_id).await?;
            Some((book.tick_size.as_decimal(), book.min_order_size))
        } else {
            None
        };
        self.trading
            .preview(PreviewParameters {
                token_id,
                kind: input.kind,
                side: input.side,
                amount,
                price,
                order_type: input.order_type,
                market_rules,
            })
            .await
    }

    pub async fn place_order(
        &self,
        approval_id: String,
        confirm: bool,
    ) -> Result<PlacedOrderOutput, AppError> {
        let approval_id = validate_nonempty("approval_id", approval_id)?;
        self.trading.place(&approval_id, confirm).await
    }

    pub async fn get_order_approval(
        &self,
        approval_id: String,
    ) -> Result<OrderApprovalStatusOutput, AppError> {
        let approval_id = validate_nonempty("approval_id", approval_id)?;
        self.trading.approval_status(&approval_id).await
    }

    pub async fn place_batch_orders(
        &self,
        approval_ids: Vec<String>,
        confirmation: String,
    ) -> Result<BatchPlacedOrdersOutput, AppError> {
        self.trading.place_batch(approval_ids, &confirmation).await
    }

    pub async fn get_order(&self, order_id: String) -> Result<OpenOrder, AppError> {
        let order_id = validate_nonempty("order_id", order_id)?;
        self.trading.order(&order_id).await
    }

    pub async fn list_open_orders(
        &self,
        token_id: Option<String>,
        next_cursor: Option<String>,
    ) -> Result<OpenOrdersOutput, AppError> {
        let token_id = token_id
            .as_deref()
            .map(|value| parse_u256("token_id", value))
            .transpose()?;
        let next_cursor = optional_nonempty("next_cursor", next_cursor)?;
        self.trading.open_orders(token_id, next_cursor).await
    }

    pub async fn list_account_trades(
        &self,
        token_id: Option<String>,
        next_cursor: Option<String>,
    ) -> Result<AccountTradesOutput, AppError> {
        let token_id = token_id
            .as_deref()
            .map(|value| parse_u256("token_id", value))
            .transpose()?;
        let next_cursor = optional_nonempty("next_cursor", next_cursor)?;
        self.trading.account_trades(token_id, next_cursor).await
    }

    pub async fn cancel_order(
        &self,
        order_id: String,
        confirm: bool,
    ) -> Result<CancelOrdersOutput, AppError> {
        let order_id = validate_nonempty("order_id", order_id)?;
        self.trading.cancel_order(&order_id, confirm).await
    }

    pub async fn cancel_all_orders(
        &self,
        confirmation: String,
    ) -> Result<CancelOrdersOutput, AppError> {
        self.trading.cancel_all(&confirmation).await
    }

    pub async fn get_balance_allowance(
        &self,
        asset_type: String,
        token_id: Option<String>,
    ) -> Result<BalanceAllowanceOutput, AppError> {
        use polymarket_client_sdk_v2::clob::types::AssetType;
        let asset_type = match asset_type.trim().to_ascii_lowercase().as_str() {
            "collateral" | "pusd" | "usdc" => AssetType::Collateral,
            "conditional" | "outcome" => AssetType::Conditional,
            _ => {
                return Err(AppError::InvalidInput(
                    "asset_type must be collateral or conditional".to_owned(),
                ));
            }
        };
        let token_id = token_id
            .as_deref()
            .map(|value| parse_u256("token_id", value))
            .transpose()?;
        self.trading.balance_allowance(asset_type, token_id).await
    }

    pub async fn cancel_market_orders(
        &self,
        condition_id: Option<String>,
        token_id: Option<String>,
        confirmation: String,
    ) -> Result<CancelOrdersOutput, AppError> {
        let condition_id = condition_id
            .as_deref()
            .map(|value| parse_b256("condition_id", value))
            .transpose()?;
        let token_id = token_id
            .as_deref()
            .map(|value| parse_u256("token_id", value))
            .transpose()?;
        self.trading
            .cancel_market(condition_id, token_id, &confirmation)
            .await
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new().expect("hard-coded Polymarket endpoints must be valid URLs")
    }
}

fn validate_nonempty(field: &str, value: String) -> Result<String, AppError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(AppError::InvalidInput(format!("{field} must not be empty")));
    }
    Ok(value)
}

fn optional_nonempty(field: &str, value: Option<String>) -> Result<Option<String>, AppError> {
    value
        .map(|value| validate_nonempty(field, value))
        .transpose()
}

fn database_task_error(error: impl ToString) -> AppError {
    AppError::Upstream {
        service: "SQLite recorder",
        message: error.to_string(),
    }
}

fn parse_decimal(field: &str, value: &str) -> Result<Decimal, AppError> {
    Decimal::from_str(value.trim()).map_err(|error| {
        AppError::InvalidInput(format!("{field} must be a decimal number: {error}"))
    })
}

fn parse_u256(field: &str, value: &str) -> Result<U256, AppError> {
    U256::from_str(value.trim()).map_err(|error| {
        AppError::InvalidInput(format!("{field} must be a decimal token ID: {error}"))
    })
}

fn parse_b256(field: &str, value: &str) -> Result<B256, AppError> {
    B256::from_str(value.trim()).map_err(|error| {
        AppError::InvalidInput(format!("{field} must be a 0x-prefixed 32-byte ID: {error}"))
    })
}

fn parse_address(field: &str, value: &str) -> Result<Address, AppError> {
    Address::from_str(value.trim()).map_err(|error| {
        AppError::InvalidInput(format!(
            "{field} must be a 0x-prefixed 20-byte address: {error}"
        ))
    })
}

fn wallet_page(limit: Option<u16>, offset: Option<u32>) -> (i32, i32) {
    (
        i32::from(limit.unwrap_or(100).clamp(1, 500)),
        i32::try_from(offset.unwrap_or(0).min(10_000)).unwrap(),
    )
}

fn parse_interval(value: &str) -> Result<Interval, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1m" => Ok(Interval::OneMinute),
        "1h" => Ok(Interval::OneHour),
        "6h" => Ok(Interval::SixHours),
        "1d" => Ok(Interval::OneDay),
        "1w" => Ok(Interval::OneWeek),
        "max" => Ok(Interval::Max),
        _ => Err(AppError::InvalidInput(
            "interval must be one of 1m, 1h, 6h, 1d, 1w, or max".to_owned(),
        )),
    }
}

fn market_summaries(event: &Event) -> impl Iterator<Item = MarketSummary> + '_ {
    event
        .markets
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|market| MarketSummary {
            event_id: event.id.clone(),
            event_title: event.title.clone(),
            market_id: market.id.clone(),
            question: market.question.clone(),
            slug: market.slug.clone(),
            active: market.active,
            closed: market.closed,
            accepting_orders: market.accepting_orders,
            end_date: market.end_date.map(|value| value.to_rfc3339()),
            volume_24h: market.volume_24hr.map(|value| value.to_string()),
            liquidity: market.liquidity_num.map(|value| value.to_string()),
            outcomes: outcome_quotes(market),
        })
}

fn event_detail(event: &Event) -> EventDetail {
    EventDetail {
        event_id: event.id.clone(),
        title: event.title.clone(),
        slug: event.slug.clone(),
        description: event.description.clone(),
        category: event.category.clone(),
        active: event.active,
        closed: event.closed,
        featured: event.featured,
        start_date: event.start_date.map(|value| value.to_rfc3339()),
        end_date: event.end_date.map(|value| value.to_rfc3339()),
        volume: event.volume.map(|value| value.to_string()),
        volume_24h: event.volume_24hr.map(|value| value.to_string()),
        liquidity: event.liquidity.map(|value| value.to_string()),
        tags: event
            .tags
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|tag| TagSummary {
                id: tag.id.clone(),
                label: tag.label.clone(),
                slug: tag.slug.clone(),
            })
            .collect(),
        markets: market_summaries(event).collect(),
    }
}

fn outcome_quotes(market: &Market) -> Vec<OutcomeQuote> {
    let names = market.outcomes.as_deref().unwrap_or_default();
    let tokens = market.clob_token_ids.as_deref().unwrap_or_default();
    let prices = market.outcome_prices.as_deref().unwrap_or_default();
    let count = names.len().max(tokens.len()).max(prices.len());

    (0..count)
        .map(|index| OutcomeQuote {
            name: names
                .get(index)
                .cloned()
                .unwrap_or_else(|| format!("Outcome {}", index + 1)),
            token_id: tokens.get(index).map(ToString::to_string),
            gamma_price: prices.get(index).map(ToString::to_string),
        })
        .collect()
}

fn market_detail(market: &Market, books: Vec<OrderBookSummaryResponse>) -> MarketDetail {
    let book_by_token = books
        .into_iter()
        .map(|book| (book.asset_id.to_string(), order_book_snapshot(book)))
        .collect::<HashMap<_, _>>();
    let event = market.events.as_deref().and_then(|events| events.first());

    MarketDetail {
        market_id: market.id.clone(),
        event_id: event.map(|value| value.id.clone()),
        event_title: event.and_then(|value| value.title.clone()),
        question: market.question.clone(),
        slug: market.slug.clone(),
        description: market.description.clone(),
        resolution_source: market.resolution_source.clone(),
        condition_id: market.condition_id.map(|value| value.to_string()),
        active: market.active,
        closed: market.closed,
        accepting_orders: market.accepting_orders,
        end_date: market.end_date.map(|value| value.to_rfc3339()),
        volume: market.volume_num.map(|value| value.to_string()),
        volume_24h: market.volume_24hr.map(|value| value.to_string()),
        liquidity: market.liquidity_num.map(|value| value.to_string()),
        outcomes: outcome_quotes(market)
            .into_iter()
            .map(|outcome| OutcomeDetail {
                order_book: outcome
                    .token_id
                    .as_ref()
                    .and_then(|token_id| book_by_token.get(token_id).cloned()),
                name: outcome.name,
                token_id: outcome.token_id,
                gamma_price: outcome.gamma_price,
            })
            .collect(),
    }
}

fn order_book_snapshot(book: OrderBookSummaryResponse) -> OrderBookSnapshot {
    let best_bid = book.bids.iter().map(|level| level.price).max();
    let best_ask = book.asks.iter().map(|level| level.price).min();
    let spread = best_bid.zip(best_ask).map(|(bid, ask)| ask - bid);
    let midpoint = best_bid
        .zip(best_ask)
        .map(|(bid, ask)| (bid + ask) / Decimal::TWO);

    OrderBookSnapshot {
        token_id: book.asset_id.to_string(),
        timestamp: book.timestamp.to_rfc3339(),
        hash: book.hash,
        best_bid: best_bid.map(|value| value.to_string()),
        best_ask: best_ask.map(|value| value.to_string()),
        spread: spread.map(|value| value.to_string()),
        midpoint: midpoint.map(|value| value.to_string()),
        bid_levels: book.bids.len(),
        ask_levels: book.asks.len(),
        min_order_size: book.min_order_size.to_string(),
        tick_size: book.tick_size.to_string(),
        last_trade_price: book.last_trade_price.map(|value| value.to_string()),
    }
}

fn order_book_detail(book: OrderBookSummaryResponse, depth: usize) -> OrderBookDetail {
    let mut bids = book.bids.clone();
    bids.sort_by(|left, right| right.price.cmp(&left.price));
    let mut asks = book.asks.clone();
    asks.sort_by(|left, right| left.price.cmp(&right.price));
    let bids = bids
        .into_iter()
        .take(depth)
        .map(|level| PriceLevel {
            price: level.price.to_string(),
            size: level.size.to_string(),
        })
        .collect();
    let asks = asks
        .into_iter()
        .take(depth)
        .map(|level| PriceLevel {
            price: level.price.to_string(),
            size: level.size.to_string(),
        })
        .collect();
    OrderBookDetail {
        summary: order_book_snapshot(book),
        bids,
        asks,
    }
}

fn wallet_position(position: Position) -> WalletPosition {
    WalletPosition {
        token_id: position.asset.to_string(),
        condition_id: position.condition_id.to_string(),
        title: position.title,
        market_slug: position.slug,
        event_slug: position.event_slug,
        outcome: position.outcome,
        size: position.size.to_string(),
        average_price: position.avg_price.to_string(),
        current_price: position.cur_price.to_string(),
        initial_value: position.initial_value.to_string(),
        current_value: position.current_value.to_string(),
        cash_pnl: position.cash_pnl.to_string(),
        percent_pnl: position.percent_pnl.to_string(),
        realized_pnl: position.realized_pnl.to_string(),
        redeemable: position.redeemable,
        mergeable: position.mergeable,
        end_date: position.end_date.map(|date| date.to_string()),
        negative_risk: position.negative_risk,
    }
}

fn wallet_trade(trade: Trade) -> WalletTrade {
    WalletTrade {
        side: trade.side.to_string(),
        token_id: trade.asset.to_string(),
        condition_id: trade.condition_id.to_string(),
        size: trade.size.to_string(),
        price: trade.price.to_string(),
        timestamp: trade.timestamp,
        title: trade.title,
        market_slug: trade.slug,
        event_slug: trade.event_slug,
        outcome: trade.outcome,
        transaction_hash: trade.transaction_hash.to_string(),
    }
}

fn wallet_activity(activity: Activity) -> WalletActivity {
    WalletActivity {
        activity_type: activity.activity_type.to_string(),
        timestamp: activity.timestamp,
        condition_id: activity.condition_id.map(|value| value.to_string()),
        token_id: activity.asset.map(|value| value.to_string()),
        size: activity.size.to_string(),
        usdc_size: activity.usdc_size.to_string(),
        price: activity.price.map(|value| value.to_string()),
        side: activity.side.map(|value| value.to_string()),
        title: activity.title,
        outcome: activity.outcome,
        transaction_hash: activity.transaction_hash.to_string(),
    }
}

fn simulate_book(
    book: &OrderBookSummaryResponse,
    side: &str,
    requested: Decimal,
) -> SimulationOutput {
    let mut levels = if side == "buy" {
        book.asks
            .iter()
            .map(|level| (level.price, level.size))
            .collect::<Vec<_>>()
    } else {
        book.bids
            .iter()
            .map(|level| (level.price, level.size))
            .collect::<Vec<_>>()
    };
    if side == "buy" {
        levels.sort_by_key(|(price, _)| *price);
    } else {
        levels.sort_by(|left, right| right.0.cmp(&left.0));
    }
    let best = levels.first().map(|(price, _)| *price);
    let mut remaining = requested;
    let mut filled = Decimal::ZERO;
    let mut notional = Decimal::ZERO;
    let mut worst = None;
    let mut levels_consumed = 0;
    for (price, available) in levels {
        if remaining <= Decimal::ZERO {
            break;
        }
        let quantity = remaining.min(available);
        if quantity > Decimal::ZERO {
            filled += quantity;
            notional += quantity * price;
            remaining -= quantity;
            worst = Some(price);
            levels_consumed += 1;
        }
    }
    let average = (filled > Decimal::ZERO).then(|| notional / filled);
    let slippage = average.zip(best).and_then(|(average, best)| {
        if best <= Decimal::ZERO {
            None
        } else if side == "buy" {
            Some((average - best) / best * Decimal::from(10_000))
        } else {
            Some((best - average) / best * Decimal::from(10_000))
        }
    });
    SimulationOutput {
        token_id: book.asset_id.to_string(),
        side: side.to_owned(),
        requested_shares: requested.to_string(),
        filled_shares: filled.to_string(),
        unfilled_shares: remaining.to_string(),
        complete_fill: remaining == Decimal::ZERO,
        total_notional: notional.to_string(),
        average_price: average.map(|value| value.to_string()),
        best_price: best.map(|value| value.to_string()),
        worst_price: worst.map(|value| value.to_string()),
        slippage_bps: slippage.map(|value| value.to_string()),
        levels_consumed,
        book_timestamp: book.timestamp.to_rfc3339(),
        methodology: "Walks the current public CLOB book in executable price order. It assumes displayed size remains available and excludes fees, latency, queue position, and price changes; it does not place an order.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use polymarket_client_sdk_v2::types::U256;

    #[test]
    fn rejects_blank_values() {
        let error = validate_nonempty("query", "   ".to_owned()).unwrap_err();
        assert_eq!(error.code(), "invalid_input");
        assert_eq!(error.to_string(), "invalid input: query must not be empty");
    }

    #[test]
    fn trims_valid_values() {
        assert_eq!(
            validate_nonempty("query", "  bitcoin  ".to_owned()).unwrap(),
            "bitcoin"
        );
    }

    #[test]
    fn status_is_offline_and_explicitly_v2() {
        let app = App::new().unwrap();
        let status = app.status();
        assert_eq!(status.mode, "read-only");
        assert_eq!(status.clob_protocol, "v2");
        assert_eq!(status.clob_endpoint, CLOB_V2_ENDPOINT);
    }

    #[test]
    fn u256_string_mapping_is_lossless() {
        let token = U256::MAX;
        assert_eq!(token.to_string().parse::<U256>().unwrap(), token);
    }

    #[test]
    fn interval_validation_is_explicit() {
        assert_eq!(parse_interval("1d").unwrap(), Interval::OneDay);
        let error = parse_interval("daily").unwrap_err();
        assert_eq!(error.code(), "invalid_input");
    }

    #[test]
    fn identifier_parsers_do_not_mix_id_types() {
        assert!(parse_u256("token_id", "1234").is_ok());
        assert!(parse_u256("token_id", "not-a-token").is_err());
        assert!(
            parse_b256(
                "condition_id",
                "0x0000000000000000000000000000000000000000000000000000000000000001"
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn order_preview_is_offline_bounded_and_non_executing() {
        let directory = tempfile::tempdir().unwrap();
        let app = App::new_with_database(directory.path().join("preview.sqlite3")).unwrap();
        let preview = app
            .preview_order(PreviewOrderInput {
                token_id: "123".to_owned(),
                kind: "limit".to_owned(),
                side: "buy".to_owned(),
                amount: "10".to_owned(),
                price: Some("0.5".to_owned()),
                order_type: None,
                live_validation: Some(false),
            })
            .await
            .unwrap();
        assert_eq!(preview.maximum_notional_usdc, "5.0");
        assert_eq!(preview.amount_unit, "shares");
        let approval = app
            .get_order_approval(preview.approval_id.clone())
            .await
            .unwrap();
        assert_eq!(approval.status, "approved");
        assert_eq!(approval.token_id, "123");

        let error = app
            .preview_order(PreviewOrderInput {
                token_id: "123".to_owned(),
                kind: "limit".to_owned(),
                side: "buy".to_owned(),
                amount: "1000".to_owned(),
                price: Some("0.5".to_owned()),
                order_type: None,
                live_validation: Some(false),
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), "invalid_input");
    }
}
