use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::StreamExt as _;
use polymarket_client_sdk_v2::{
    clob::types::{Interval, TimeRange, response::OrderBookSummaryResponse},
    data::types::response::{Activity, Position, Trade},
    gamma::types::request::{EventsRequest, MarketsRequest},
    gamma::types::response::{Event, Market},
    types::{Address, B256, DateTime, Decimal, U256, Utc},
};
use tokio::sync::RwLock;

use crate::{
    error::AppError,
    polymarket::{CLOB_V2_ENDPOINT, DATA_ENDPOINT, GAMMA_ENDPOINT, PolymarketClient},
    realtime::RealtimeService,
    recorder::RecorderService,
    trading::{PreviewParameters, TradingService},
    types::{
        AccountTradesOutput, AnalyzeEventConsistencyInput, AnalyzeOrderBookInput,
        BalanceAllowanceOutput, BatchPlacedOrdersOutput, BinaryComplementCheck, CancelOrdersOutput,
        CompareMarketsOutput, EventConsistencyLeg, EventConsistencyOutput, EventDetail,
        GetMarketBriefInput, HolderDetail, ListMarketsInput, ListMarketsOutput,
        ListRecordingsOutput, LiveSnapshotOutput, MarketBriefOutcome, MarketBriefOutput,
        MarketDetail, MarketHoldersOutput, MarketMicrostructureSummary, MarketSummary, OpenOrder,
        OpenOrdersOutput, OrderApprovalStatusOutput, OrderBookAnalysisOutput, OrderBookDetail,
        OrderBookSnapshot, OrderPreviewOutput, OutcomeDetail, OutcomeHolders,
        OutcomeMicrostructureSummary, OutcomeQuote, PlacedOrderOutput, PreviewOrderInput,
        PriceHistoryOutput, PriceHistorySummary, PriceLevel, PricePoint, RealtimeEventsOutput,
        RealtimeStatusOutput, RecordingInfo, ReplayEventsOutput, ReplayMarketOutput,
        ScanMarketMicrostructureInput, ScanMarketMicrostructureOutput, SearchMarketsOutput,
        ServerStatus, SimulationOutput, SourceReference, StopWatchOutput, TagSummary,
        TradingStatusOutput, UserRealtimeEventsOutput, UserRealtimeStatusOutput, UserWatchInfo,
        WalletActivity, WalletActivityOutput, WalletPosition, WalletPositionsOutput,
        WalletRiskOutput, WalletSummaryInput, WalletSummaryOutput, WalletTrade, WalletTradesOutput,
        WalletValueOutput, WatchInfo,
    },
};

const DEFAULT_SEARCH_LIMIT: u8 = 10;
const MAX_SEARCH_LIMIT: u8 = 25;
type BriefCache = Arc<RwLock<HashMap<String, (Instant, MarketBriefOutput)>>>;

#[derive(Clone, Debug)]
pub struct App {
    client: PolymarketClient,
    realtime: RealtimeService,
    recorder: RecorderService,
    trading: TradingService,
    brief_cache: BriefCache,
    brief_cache_ttl: Duration,
}

impl App {
    pub fn new() -> Result<Self, AppError> {
        let database_path = std::env::var_os("POLYMARKET_MCP_DB")
            .map(PathBuf::from)
            .unwrap_or_else(default_database_path);
        Self::new_with_database(database_path)
    }

    /// Build an app for catalog inspection without creating a persistent database file.
    pub fn new_ephemeral() -> Result<Self, AppError> {
        Self::new_with_database(PathBuf::from(":memory:"))
    }

    /// Build the credential-blind, non-persistent core used by public HTTP.
    pub fn new_public() -> Result<Self, AppError> {
        let database_path = PathBuf::from(":memory:");
        Ok(Self {
            client: PolymarketClient::new()?,
            realtime: RealtimeService::new()?,
            recorder: RecorderService::new(database_path.clone())?,
            trading: TradingService::disabled(database_path),
            brief_cache: Arc::new(RwLock::new(HashMap::new())),
            brief_cache_ttl: cache_ttl_from_environment(2_000)?,
        })
    }

    pub fn new_with_database(database_path: PathBuf) -> Result<Self, AppError> {
        if database_path.as_os_str() != ":memory:"
            && let Some(parent) = database_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| AppError::Upstream {
                service: "SQLite recorder",
                message: format!("could not create {}: {error}", parent.display()),
            })?;
        }
        let trading_database_path = database_path.clone();
        Ok(Self {
            client: PolymarketClient::new()?,
            realtime: RealtimeService::new()?,
            recorder: RecorderService::new(database_path)?,
            trading: TradingService::from_environment(trading_database_path)?,
            brief_cache: Arc::new(RwLock::new(HashMap::new())),
            brief_cache_ttl: cache_ttl_from_environment(0)?,
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
            tool_profile: "unconfigured".to_owned(),
            tool_count: 0,
        }
    }

    pub async fn shutdown(&self) -> Result<(usize, usize), AppError> {
        // Flush recordings while their source watches still exist, then cancel watches.
        let recording_result = self.recorder.stop_all().await;
        let stopped_watches =
            self.realtime.stop_all().await + self.trading.stop_user_watches().await;
        recording_result.map(|stopped_recordings| (stopped_recordings, stopped_watches))
    }

    pub async fn clob_health(&self) -> Result<String, AppError> {
        self.client.clob_health().await
    }

    pub async fn data_health(&self) -> Result<String, AppError> {
        self.client.data_health().await
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
            as_of_ms: now_ms(),
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

    pub async fn get_market_brief(
        &self,
        input: GetMarketBriefInput,
    ) -> Result<MarketBriefOutput, AppError> {
        let cache_key = format!("{input:?}");
        if !self.brief_cache_ttl.is_zero()
            && let Some((inserted, brief)) = self.brief_cache.read().await.get(&cache_key)
            && inserted.elapsed() <= self.brief_cache_ttl
        {
            return Ok(brief.clone());
        }
        let sample_shares = input
            .sample_shares
            .as_deref()
            .map(|value| parse_decimal("sample_shares", value))
            .transpose()?
            .unwrap_or_else(|| Decimal::from(100));
        if sample_shares <= Decimal::ZERO {
            return Err(AppError::InvalidInput(
                "sample_shares must be greater than zero".to_owned(),
            ));
        }
        let history_interval = input.history_interval.unwrap_or_else(|| "1d".to_owned());
        parse_interval(&history_interval)?;
        let history_limit = input.history_limit.unwrap_or(50).clamp(2, 250);
        let market = self
            .get_market_by_identifier(input.market_id, input.slug, input.condition_id)
            .await?;
        let mut outcome_research = Vec::with_capacity(market.outcomes.len());
        for outcome in &market.outcomes {
            let mut errors = Vec::new();
            let (analysis, history) = if let Some(token_id) = &outcome.token_id {
                let analysis = match self
                    .analyze_order_book(AnalyzeOrderBookInput {
                        token_id: token_id.clone(),
                        depth: Some(50),
                        price_band: Some("0.02".to_owned()),
                        sample_shares: Some(sample_shares.to_string()),
                    })
                    .await
                {
                    Ok(value) => Some(value),
                    Err(error) => {
                        errors.push(format!("order-book analysis unavailable: {error}"));
                        None
                    }
                };
                let history = match self
                    .get_price_history(
                        token_id.clone(),
                        Some(history_interval.clone()),
                        None,
                        None,
                        None,
                        Some(history_limit),
                    )
                    .await
                {
                    Ok(value) => Some(price_history_summary(&history_interval, &value.points)),
                    Err(error) => {
                        errors.push(format!("price history unavailable: {error}"));
                        None
                    }
                };
                (analysis, history)
            } else {
                errors.push("outcome has no CLOB token ID".to_owned());
                (None, None)
            };
            outcome_research.push(MarketBriefOutcome {
                outcome: outcome.name.clone(),
                token_id: outcome.token_id.clone(),
                gamma_price: outcome.gamma_price.clone(),
                analysis,
                history,
                errors,
            });
        }
        let as_of_ms = now_ms();
        let mut sources = vec![SourceReference {
            name: "Polymarket Gamma API".to_owned(),
            url: market.gamma_url.clone(),
            scope: "market metadata, outcome mappings, liquidity, and volume".to_owned(),
            retrieved_at_ms: as_of_ms,
        }];
        sources.push(SourceReference {
            name: "Polymarket CLOB V2".to_owned(),
            url: CLOB_V2_ENDPOINT.to_owned(),
            scope: "current executable books and public price history".to_owned(),
            retrieved_at_ms: as_of_ms,
        });
        if let Some(url) = &market.polymarket_url {
            sources.push(SourceReference {
                name: "Polymarket market page".to_owned(),
                url: url.clone(),
                scope: "human-readable market, rules, and resolution context".to_owned(),
                retrieved_at_ms: as_of_ms,
            });
        }
        let brief = MarketBriefOutput {
            as_of_ms,
            market,
            outcome_research,
            sources,
            limitations: vec![
                "Prices and books can change immediately after the reported timestamps.".to_owned(),
                "Execution estimates walk displayed L2 depth and exclude fees, queue position, latency, and future book changes.".to_owned(),
                "Probabilities are market prices, not factual forecasts or trading recommendations.".to_owned(),
            ],
        };
        if !self.brief_cache_ttl.is_zero() {
            let mut cache = self.brief_cache.write().await;
            if cache.len() >= 256 {
                cache.retain(|_, (inserted, _)| inserted.elapsed() <= self.brief_cache_ttl);
                if cache.len() >= 256
                    && let Some(oldest) = cache
                        .iter()
                        .min_by_key(|(_, (inserted, _))| *inserted)
                        .map(|(key, _)| key.clone())
                {
                    cache.remove(&oldest);
                }
            }
            cache.insert(cache_key, (Instant::now(), brief.clone()));
        }
        Ok(brief)
    }

    pub async fn get_wallet_summary(
        &self,
        input: WalletSummaryInput,
    ) -> Result<WalletSummaryOutput, AppError> {
        let wallet = validate_nonempty("wallet", input.wallet)?;
        let recent_limit = input.recent_limit.unwrap_or(20).clamp(1, 100);
        let (positions, value, risk, recent_trades, recent_activity) = tokio::try_join!(
            self.get_wallet_positions(wallet.clone(), Some(500), None),
            self.get_wallet_value(wallet.clone()),
            self.analyze_wallet_risk(wallet.clone()),
            self.get_wallet_trades(wallet.clone(), Some(recent_limit), None),
            self.get_wallet_activity(wallet.clone(), Some(recent_limit), None, None, None),
        )?;
        let as_of_ms = now_ms();
        Ok(WalletSummaryOutput {
            as_of_ms,
            wallet: wallet.clone(),
            positions,
            value,
            risk,
            recent_trades,
            recent_activity,
            sources: [
                ("positions", "current public positions and position P/L"),
                ("value", "reported aggregate position value"),
                ("trades", "recent public trades"),
                ("activity", "recent public on-chain activity"),
            ]
            .into_iter()
            .map(|(endpoint, scope)| SourceReference {
                name: format!("Polymarket Data API /{endpoint}"),
                url: format!("{DATA_ENDPOINT}/{endpoint}?user={wallet}"),
                scope: scope.to_owned(),
                retrieved_at_ms: as_of_ms,
            })
            .collect(),
            limitations: vec![
                "Wallet data is public proxy-wallet data and may not identify a person.".to_owned(),
                "Risk metrics describe current reported positions and are not financial advice."
                    .to_owned(),
            ],
        })
    }

    pub async fn analyze_event_consistency(
        &self,
        input: AnalyzeEventConsistencyInput,
    ) -> Result<EventConsistencyOutput, AppError> {
        if input.event_id.is_some() == input.slug.is_some() {
            return Err(AppError::InvalidInput(
                "provide exactly one of event_id or slug".to_owned(),
            ));
        }
        let event = if let Some(event_id) = input.event_id {
            self.client
                .event_by_id(&validate_nonempty("event_id", event_id)?)
                .await?
        } else {
            self.client
                .event_by_slug(&validate_nonempty("slug", input.slug.unwrap())?)
                .await?
        };
        let explicit_negative_risk =
            event.neg_risk == Some(true) || event.enable_neg_risk == Some(true);
        let event_id = event.id.clone();
        let event_title = event.title.clone();
        let event_slug = event.slug.clone();
        let polymarket_url = polymarket_event_url(event_slug.as_deref());
        let mut legs = Vec::new();
        for market in event.markets.unwrap_or_default() {
            let names = market.outcomes.as_deref().unwrap_or_default();
            let tokens = market.clob_token_ids.as_deref().unwrap_or_default();
            let yes_index = names
                .iter()
                .position(|name| name.eq_ignore_ascii_case("yes"));
            let outcome = yes_index.and_then(|index| names.get(index)).cloned();
            let token_id = yes_index
                .and_then(|index| tokens.get(index))
                .map(ToString::to_string);
            let leg = EventConsistencyLeg {
                market_id: market.id.clone(),
                question: market.question.clone(),
                market_slug: market.slug.clone(),
                polymarket_url: polymarket_url.clone(),
                outcome,
                token_id: token_id.clone(),
                best_bid: None,
                best_ask: None,
                top_bid_size: None,
                top_ask_size: None,
                book_timestamp: None,
                error: None,
            };
            legs.push(leg);
        }
        if explicit_negative_risk {
            let mut indexed_legs = futures::stream::iter(legs.into_iter().enumerate())
                .map(|(index, mut leg)| async move {
                    if let Some(token_id) = leg.token_id.clone() {
                        match self.get_order_book(token_id, Some(1)).await {
                            Ok(book) => {
                                leg.best_bid = book.summary.best_bid;
                                leg.best_ask = book.summary.best_ask;
                                leg.top_bid_size =
                                    book.bids.first().map(|level| level.size.clone());
                                leg.top_ask_size =
                                    book.asks.first().map(|level| level.size.clone());
                                leg.book_timestamp = Some(book.summary.timestamp);
                            }
                            Err(error) => leg.error = Some(error.to_string()),
                        }
                    } else {
                        leg.error = Some("market has no explicit Yes outcome token".to_owned());
                    }
                    (index, leg)
                })
                .buffer_unordered(8)
                .collect::<Vec<_>>()
                .await;
            indexed_legs.sort_by_key(|(index, _)| *index);
            legs = indexed_legs.into_iter().map(|(_, leg)| leg).collect();
        }
        let eligible_for_basket_math = explicit_negative_risk
            && legs.len() >= 2
            && legs
                .iter()
                .all(|leg| leg.error.is_none() && leg.token_id.is_some());
        let best_ask_sum = eligible_for_basket_math
            .then(|| decimal_sum(legs.iter().map(|leg| leg.best_ask.as_deref())))
            .flatten();
        let best_bid_sum = eligible_for_basket_math
            .then(|| decimal_sum(legs.iter().map(|leg| leg.best_bid.as_deref())))
            .flatten();
        let buy_capacity = eligible_for_basket_math
            .then(|| decimal_min(legs.iter().map(|leg| leg.top_ask_size.as_deref())))
            .flatten();
        let sell_capacity = eligible_for_basket_math
            .then(|| decimal_min(legs.iter().map(|leg| leg.top_bid_size.as_deref())))
            .flatten();
        let as_of_ms = now_ms();
        Ok(EventConsistencyOutput {
            as_of_ms,
            event_id: event_id.clone(),
            event_title,
            event_slug,
            polymarket_url,
            explicit_negative_risk,
            eligible_for_basket_math,
            legs,
            best_ask_sum: best_ask_sum.map(|value| value.to_string()),
            buy_all_gross_edge_per_basket: best_ask_sum
                .map(|value| (Decimal::ONE - value).to_string()),
            buy_all_top_level_capacity: buy_capacity.map(|value| value.to_string()),
            best_bid_sum: best_bid_sum.map(|value| value.to_string()),
            sell_all_gross_edge_per_basket: best_bid_sum
                .map(|value| (value - Decimal::ONE).to_string()),
            sell_all_top_level_capacity: sell_capacity.map(|value| value.to_string()),
            sources: vec![
                SourceReference {
                    name: "Polymarket Gamma API".to_owned(),
                    url: format!("{GAMMA_ENDPOINT}/events/{event_id}"),
                    scope: "explicit negative-risk event membership and outcome-token mappings"
                        .to_owned(),
                    retrieved_at_ms: as_of_ms,
                },
                SourceReference {
                    name: "Polymarket CLOB V2".to_owned(),
                    url: CLOB_V2_ENDPOINT.to_owned(),
                    scope: "current executable top-of-book prices and sizes".to_owned(),
                    retrieved_at_ms: as_of_ms,
                },
            ],
            methodology: "Only events explicitly marked negative-risk by Gamma are treated as mutually exclusive/exhaustive baskets. The tool sums current executable Yes asks or bids and takes the smallest displayed top-level size across legs."
                .to_owned(),
            limitations: vec![
                "Gross edges exclude fees, latency, partial fills beyond displayed top-level capacity, and book changes between requests.".to_owned(),
                "A positive displayed edge is not a guaranteed executable arbitrage.".to_owned(),
                "No semantic relationship is inferred for events lacking explicit negative-risk metadata.".to_owned(),
            ],
        })
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
        let market_offset = input.offset.unwrap_or(0).min(10_000) as usize;
        let requested_sort = input.sort_by.unwrap_or_else(|| "volume_24h".to_owned());
        let sort_by = match requested_sort.as_str() {
            "volume_24h" | "volume24hr" => "volume24hr",
            "volume_7d" | "volume1wk" => "volume1wk",
            "volume_30d" | "volume1mo" => "volume1mo",
            "volume" | "volumeNum" => "volumeNum",
            "liquidity" | "liquidityNum" => "liquidityNum",
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
        let tag_id = match tag_slug {
            Some(slug) => Some(self.client.tag_id_by_slug(&slug).await?),
            None => None,
        };
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
        let end_after = input
            .end_after
            .as_deref()
            .map(|value| parse_rfc3339("end_after", value))
            .transpose()?;
        let end_before = input
            .end_before
            .as_deref()
            .map(|value| parse_rfc3339("end_before", value))
            .transpose()?;
        if matches!((end_after, end_before), (Some(start), Some(end)) if start > end) {
            return Err(AppError::InvalidInput(
                "end_after must not be later than end_before".to_owned(),
            ));
        }

        if input.featured == Some(true) {
            return self
                .list_featured_markets(
                    limit,
                    market_offset,
                    sort_by,
                    input.ascending.unwrap_or(false),
                    tag_id,
                    min_liquidity,
                    min_volume,
                    end_after,
                    end_before,
                )
                .await;
        }

        // Gamma's event endpoint sorts and paginates events, not their nested markets.
        // Walk the directly sorted market endpoint from the beginning so `offset` applies
        // after active/order-accepting filters and adjacent pages cannot overlap.
        const PAGE_SIZE: i32 = 100;
        let target = market_offset.saturating_add(usize::from(limit));
        let ascending = input.ascending.unwrap_or(false);
        let mut upstream_offset = 0_i32;
        let mut eligible = Vec::with_capacity(target.min(1_000));
        while eligible.len() < target && upstream_offset <= 10_000 {
            let request = MarketsRequest::builder()
                .limit(PAGE_SIZE)
                .offset(upstream_offset)
                .order(sort_by.to_owned())
                .ascending(ascending)
                .closed(false)
                .include_tag(true)
                .maybe_tag_id(tag_id.clone())
                .maybe_liquidity_num_min(min_liquidity)
                .maybe_volume_num_min(min_volume)
                .maybe_end_date_min(end_after)
                .maybe_end_date_max(end_before)
                .build();
            let page = self.client.markets(&request).await?;
            let page_len = page.len();
            eligible.extend(page.into_iter().filter(|market| {
                market.active == Some(true)
                    && market.closed != Some(true)
                    && market.accepting_orders == Some(true)
                    && (input.featured != Some(true) || market.featured == Some(true))
            }));
            if page_len < PAGE_SIZE as usize {
                break;
            }
            upstream_offset += PAGE_SIZE;
        }

        let markets = eligible
            .into_iter()
            .skip(market_offset)
            .take(usize::from(limit))
            .map(|market| market_summary(&market, market.events.as_deref().and_then(|v| v.first())))
            .collect::<Vec<_>>();
        Ok(ListMarketsOutput {
            as_of_ms: now_ms(),
            count: markets.len(),
            markets,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn list_featured_markets(
        &self,
        limit: u16,
        market_offset: usize,
        sort_by: &str,
        ascending: bool,
        tag_id: Option<String>,
        min_liquidity: Option<Decimal>,
        min_volume: Option<Decimal>,
        end_after: Option<DateTime<Utc>>,
        end_before: Option<DateTime<Utc>>,
    ) -> Result<ListMarketsOutput, AppError> {
        // `featured` is an event-level Gamma filter. Applying it after walking
        // `/markets` can cross Gamma's offset ceiling before finding a result,
        // so fetch the intentionally small featured-event set and rank its
        // nested markets locally.
        const EVENT_PAGE_SIZE: i32 = 100;
        let mut upstream_offset = 0_i32;
        let mut candidates = Vec::<(Market, String, Option<String>, Option<String>)>::new();
        loop {
            let request = EventsRequest::builder()
                .limit(EVENT_PAGE_SIZE)
                .offset(upstream_offset)
                .active(true)
                .closed(false)
                .featured(true)
                .maybe_tag_id(tag_id.clone())
                .build();
            let events = self.client.events(&request).await?;
            let page_len = events.len();
            for event in events {
                let event_id = event.id;
                let event_title = event.title;
                let event_slug = event.slug;
                candidates.extend(
                    event
                        .markets
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|market| {
                            market.active == Some(true)
                                && market.closed != Some(true)
                                && market.accepting_orders == Some(true)
                                && min_liquidity.is_none_or(|minimum| {
                                    market.liquidity_num.is_some_and(|value| value >= minimum)
                                })
                                && min_volume.is_none_or(|minimum| {
                                    market.volume_num.is_some_and(|value| value >= minimum)
                                })
                                && end_after.is_none_or(|minimum| {
                                    market.end_date.is_some_and(|value| value >= minimum)
                                })
                                && end_before.is_none_or(|maximum| {
                                    market.end_date.is_some_and(|value| value <= maximum)
                                })
                        })
                        .map(|market| {
                            (
                                market,
                                event_id.clone(),
                                event_title.clone(),
                                event_slug.clone(),
                            )
                        }),
                );
            }
            if page_len < EVENT_PAGE_SIZE as usize {
                break;
            }
            upstream_offset += EVENT_PAGE_SIZE;
            if upstream_offset >= 10_000 {
                break;
            }
        }
        candidates.sort_by(|(left, _, _, _), (right, _, _, _)| {
            let ordering = compare_market_field(left, right, sort_by);
            if ascending {
                ordering
            } else {
                ordering.reverse()
            }
        });
        let markets = candidates
            .into_iter()
            .skip(market_offset)
            .take(usize::from(limit))
            .map(|(market, event_id, event_title, event_slug)| {
                let mut summary = market_summary(&market, None);
                summary.event_id = event_id;
                summary.event_title = event_title;
                summary.event_slug = event_slug.clone();
                summary.polymarket_url = polymarket_event_url(event_slug.as_deref());
                summary
            })
            .collect::<Vec<_>>();
        Ok(ListMarketsOutput {
            as_of_ms: now_ms(),
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

    pub async fn analyze_order_book(
        &self,
        input: AnalyzeOrderBookInput,
    ) -> Result<OrderBookAnalysisOutput, AppError> {
        let token_id = parse_u256("token_id", &input.token_id)?;
        let depth = usize::from(input.depth.unwrap_or(50).clamp(1, 100));
        let price_band = input
            .price_band
            .as_deref()
            .map(|value| parse_decimal("price_band", value))
            .transpose()?
            .unwrap_or_else(|| Decimal::new(2, 2));
        if price_band <= Decimal::ZERO || price_band > Decimal::ONE {
            return Err(AppError::InvalidInput(
                "price_band must be greater than 0 and no greater than 1".to_owned(),
            ));
        }
        let sample_shares = input
            .sample_shares
            .as_deref()
            .map(|value| parse_decimal("sample_shares", value))
            .transpose()?
            .unwrap_or_else(|| Decimal::from(100));
        if sample_shares <= Decimal::ZERO {
            return Err(AppError::InvalidInput(
                "sample_shares must be greater than zero".to_owned(),
            ));
        }
        let book = self.client.order_book(token_id).await?;
        Ok(analyze_book(&book, depth, price_band, sample_shares))
    }

    pub async fn scan_market_microstructure(
        &self,
        input: ScanMarketMicrostructureInput,
    ) -> Result<ScanMarketMicrostructureOutput, AppError> {
        let limit = input.limit.unwrap_or(10).clamp(1, 20);
        let depth = usize::from(input.depth.unwrap_or(20).clamp(1, 100));
        let price_band = input
            .price_band
            .as_deref()
            .map(|value| parse_decimal("price_band", value))
            .transpose()?
            .unwrap_or_else(|| Decimal::new(2, 2));
        if price_band <= Decimal::ZERO || price_band > Decimal::ONE {
            return Err(AppError::InvalidInput(
                "price_band must be greater than 0 and no greater than 1".to_owned(),
            ));
        }
        let sample_shares = input
            .sample_shares
            .as_deref()
            .map(|value| parse_decimal("sample_shares", value))
            .transpose()?
            .unwrap_or_else(|| Decimal::from(100));
        if sample_shares <= Decimal::ZERO {
            return Err(AppError::InvalidInput(
                "sample_shares must be greater than zero".to_owned(),
            ));
        }

        let candidate_limit = u16::from(limit).saturating_mul(5).min(100);
        let mut listed = self
            .list_markets(ListMarketsInput {
                limit: Some(candidate_limit),
                offset: Some(0),
                tag_slug: input.tag_slug,
                featured: None,
                min_liquidity: input.min_liquidity,
                min_volume: None,
                end_after: None,
                end_before: None,
                sort_by: Some("volume_24h".to_owned()),
                ascending: Some(false),
            })
            .await?;
        listed.markets.sort_by(|left, right| {
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
        listed.markets.truncate(usize::from(limit));

        let mut invalid_token_count = 0;
        let mut tokens = HashMap::new();
        for market in &listed.markets {
            for outcome in &market.outcomes {
                if let Some(token_id) = outcome.token_id.as_deref() {
                    match parse_u256("token_id", token_id) {
                        Ok(token) => {
                            tokens.insert(token.to_string(), token);
                        }
                        Err(_) => invalid_token_count += 1,
                    }
                }
            }
        }

        let client = self.client.clone();
        let book_results = futures::stream::iter(tokens.into_iter().map(|(token_id, token)| {
            let client = client.clone();
            async move { (token_id, client.order_book(token).await) }
        }))
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;

        let mut books = HashMap::new();
        let mut book_error_count = invalid_token_count;
        for (token_id, result) in book_results {
            match result {
                Ok(book) => {
                    books.insert(token_id, book);
                }
                Err(_) => book_error_count += 1,
            }
        }

        let mut outcome_book_count = 0;
        let markets = listed
            .markets
            .into_iter()
            .map(|market| {
                let mut binary_quotes = Vec::new();
                let outcomes = market
                    .outcomes
                    .iter()
                    .filter_map(|outcome| {
                        let token_id = outcome.token_id.as_deref()?;
                        let book = books.get(token_id)?;
                        outcome_book_count += 1;
                        binary_quotes.push(best_quotes(book));
                        let analysis = analyze_book(book, depth, price_band, sample_shares);
                        Some(compact_microstructure(outcome.name.clone(), analysis))
                    })
                    .collect::<Vec<_>>();
                let binary_complement = (market.outcomes.len() == 2 && outcomes.len() == 2)
                    .then(|| binary_complement_check(&binary_quotes))
                    .flatten();

                MarketMicrostructureSummary {
                    market_id: market.market_id,
                    question: market.question,
                    slug: market.slug,
                    volume_24h: market.volume_24h,
                    liquidity: market.liquidity,
                    outcomes,
                    binary_complement,
                }
            })
            .collect::<Vec<_>>();

        Ok(ScanMarketMicrostructureOutput {
            market_count: markets.len(),
            outcome_book_count,
            book_error_count,
            markets,
            methodology: "Scans a bounded set of active, order-accepting markets ranked by trailing 24-hour volume and fetches outcome books concurrently with at most eight requests in flight. Metrics use current public displayed L2 liquidity. Binary complement checks compare the two outcomes of one binary market only; gross edges exclude fees, latency, queue position, and the mechanics or inventory required to execute both legs. Descriptive only; not a trading recommendation."
                .to_owned(),
        })
    }

    pub async fn get_price_history(
        &self,
        token_id: String,
        interval: Option<String>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        fidelity: Option<u32>,
        limit: Option<u16>,
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
        let upstream_point_count = points.len();
        let limit = usize::from(limit.unwrap_or(250).clamp(1, 1_000));
        let points = downsample_points(points, limit);
        Ok(PriceHistoryOutput {
            token_id: token_id.to_string(),
            upstream_point_count,
            point_count: points.len(),
            truncated: points.len() < upstream_point_count,
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
            value_pusd: value.to_string(),
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
        parsed.sort_unstable();
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
        let events = self.realtime.subscribe_events();
        let initial = self.realtime.snapshot(&watch_id, 10_000).await?;
        self.recorder
            .start(watch_id, label, initial, updates, events)
            .await
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

    pub async fn replay_events(
        &self,
        recording_id: String,
        after_sequence: Option<u64>,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
        limit: Option<u32>,
    ) -> Result<ReplayEventsOutput, AppError> {
        let recording_id = validate_nonempty("recording_id", recording_id)?;
        if matches!((start_ms, end_ms), (Some(start), Some(end)) if start > end) {
            return Err(AppError::InvalidInput(
                "start_ms must not be later than end_ms".to_owned(),
            ));
        }
        let recorder = self.recorder.clone();
        let after_sequence = after_sequence.unwrap_or(0);
        let limit = limit.unwrap_or(100).clamp(1, 5_000);
        tokio::task::spawn_blocking(move || {
            recorder.replay_events(&recording_id, after_sequence, start_ms, end_ms, limit)
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

    pub async fn watch_user_events(
        &self,
        condition_ids: Vec<String>,
    ) -> Result<UserWatchInfo, AppError> {
        if !(1..=50).contains(&condition_ids.len()) {
            return Err(AppError::InvalidInput(
                "condition_ids must contain between 1 and 50 IDs".to_owned(),
            ));
        }
        let mut parsed = Vec::with_capacity(condition_ids.len());
        for condition_id in condition_ids {
            let condition_id = parse_b256("condition_id", &condition_id)?;
            if !parsed.contains(&condition_id) {
                parsed.push(condition_id);
            }
        }
        self.trading.watch_user_events(parsed).await
    }

    pub async fn get_user_events(
        &self,
        watch_id: String,
        after_sequence: Option<u64>,
        limit: Option<u16>,
    ) -> Result<UserRealtimeEventsOutput, AppError> {
        let watch_id = validate_nonempty("watch_id", watch_id)?;
        self.trading
            .user_events(
                &watch_id,
                after_sequence.unwrap_or(0),
                usize::from(limit.unwrap_or(100).clamp(1, 500)),
            )
            .await
    }

    pub async fn get_user_realtime_status(&self) -> UserRealtimeStatusOutput {
        self.trading.user_realtime_status().await
    }

    pub async fn stop_user_watch(&self, watch_id: String) -> Result<StopWatchOutput, AppError> {
        let watch_id = validate_nonempty("watch_id", watch_id)?;
        Ok(self.trading.stop_user_watch(&watch_id).await)
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

fn default_database_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(root)
            .join("polymarket-mcp")
            .join("polymarket-mcp.sqlite3");
    }

    #[cfg(target_os = "macos")]
    if let Some(root) = std::env::var_os("HOME") {
        return PathBuf::from(root)
            .join("Library")
            .join("Application Support")
            .join("polymarket-mcp")
            .join("polymarket-mcp.sqlite3");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(root) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(root)
                .join("polymarket-mcp")
                .join("polymarket-mcp.sqlite3");
        }
        if let Some(root) = std::env::var_os("HOME") {
            return PathBuf::from(root)
                .join(".local")
                .join("share")
                .join("polymarket-mcp")
                .join("polymarket-mcp.sqlite3");
        }
    }

    PathBuf::from("polymarket-mcp.sqlite3")
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

fn parse_rfc3339(field: &str, value: &str) -> Result<DateTime<Utc>, AppError> {
    DateTime::parse_from_rfc3339(value.trim())
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| AppError::InvalidInput(format!("{field} must be RFC3339: {error}")))
}

fn cache_ttl_from_environment(default_ms: u64) -> Result<Duration, AppError> {
    let milliseconds = std::env::var("POLYMARKET_CACHE_TTL_MS")
        .ok()
        .map(|value| {
            value.parse::<u64>().map_err(|error| {
                AppError::InvalidInput(format!(
                    "POLYMARKET_CACHE_TTL_MS must be an integer from 0 to 60000: {error}"
                ))
            })
        })
        .transpose()?
        .unwrap_or(default_ms)
        .min(60_000);
    Ok(Duration::from_millis(milliseconds))
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

fn downsample_points(points: Vec<PricePoint>, limit: usize) -> Vec<PricePoint> {
    if points.len() <= limit {
        return points;
    }
    if limit == 1 {
        return points.into_iter().next_back().into_iter().collect();
    }
    let last = points.len() - 1;
    (0..limit)
        .map(|index| {
            let source = index * last / (limit - 1);
            points[source].clone()
        })
        .collect()
}

fn price_history_summary(interval: &str, points: &[PricePoint]) -> PriceHistorySummary {
    let parsed = points
        .iter()
        .filter_map(|point| Decimal::from_str(&point.price).ok())
        .collect::<Vec<_>>();
    let minimum = parsed.iter().copied().min();
    let maximum = parsed.iter().copied().max();
    let first = parsed.first().copied();
    let last = parsed.last().copied();
    let absolute_change = first.zip(last).map(|(start, end)| end - start);
    let percent_change = first
        .filter(|start| !start.is_zero())
        .zip(last)
        .map(|(start, end)| (end - start) / start * Decimal::from(100));
    PriceHistorySummary {
        interval: interval.to_owned(),
        point_count: points.len(),
        first_timestamp: points.first().map(|point| point.timestamp),
        last_timestamp: points.last().map(|point| point.timestamp),
        first_price: first.map(|value| value.to_string()),
        last_price: last.map(|value| value.to_string()),
        minimum_price: minimum.map(|value| value.to_string()),
        maximum_price: maximum.map(|value| value.to_string()),
        absolute_change: absolute_change.map(|value| value.to_string()),
        percent_change: percent_change.map(|value| value.to_string()),
    }
}

fn decimal_sum<'a>(mut values: impl Iterator<Item = Option<&'a str>>) -> Option<Decimal> {
    values.try_fold(Decimal::ZERO, |sum, value| {
        Decimal::from_str(value?).ok().map(|value| sum + value)
    })
}

fn decimal_min<'a>(mut values: impl Iterator<Item = Option<&'a str>>) -> Option<Decimal> {
    values.try_fold(None, |minimum, value| {
        let value = Decimal::from_str(value?).ok()?;
        Some(Some(
            minimum.map_or(value, |current: Decimal| current.min(value)),
        ))
    })?
}

fn polymarket_event_url(slug: Option<&str>) -> Option<String> {
    slug.filter(|value| !value.is_empty())
        .map(|slug| format!("https://polymarket.com/event/{slug}"))
}

fn gamma_market_url(market_id: &str) -> String {
    format!("{GAMMA_ENDPOINT}/markets/{market_id}")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

fn market_summaries(event: &Event) -> impl Iterator<Item = MarketSummary> + '_ {
    event
        .markets
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|market| market_summary(market, Some(event)))
}

fn compare_market_field(left: &Market, right: &Market, field: &str) -> std::cmp::Ordering {
    match field {
        "volume24hr" => left.volume_24hr.cmp(&right.volume_24hr),
        "volume1wk" => left.volume_1wk.cmp(&right.volume_1wk),
        "volume1mo" => left.volume_1mo.cmp(&right.volume_1mo),
        "volumeNum" => left.volume_num.cmp(&right.volume_num),
        "liquidityNum" => left.liquidity_num.cmp(&right.liquidity_num),
        "startDate" => left.start_date.cmp(&right.start_date),
        "endDate" => left.end_date.cmp(&right.end_date),
        _ => std::cmp::Ordering::Equal,
    }
    .then_with(|| left.id.cmp(&right.id))
}

fn market_summary(market: &Market, event: Option<&Event>) -> MarketSummary {
    let event_slug = event.and_then(|event| event.slug.clone());
    MarketSummary {
        event_id: event.map_or_else(String::new, |event| event.id.clone()),
        event_title: event.and_then(|event| event.title.clone()),
        event_slug: event_slug.clone(),
        market_id: market.id.clone(),
        question: market.question.clone(),
        slug: market.slug.clone(),
        active: market.active,
        closed: market.closed,
        accepting_orders: market.accepting_orders,
        end_date: market.end_date.map(|value| value.to_rfc3339()),
        volume_24h: market.volume_24hr.map(|value| value.to_string()),
        volume_7d: market.volume_1wk.map(|value| value.to_string()),
        volume_30d: market.volume_1mo.map(|value| value.to_string()),
        liquidity: market.liquidity_num.map(|value| value.to_string()),
        outcomes: outcome_quotes(market),
        polymarket_url: polymarket_event_url(event_slug.as_deref()),
        gamma_url: gamma_market_url(&market.id),
    }
}

fn event_detail(event: &Event) -> EventDetail {
    EventDetail {
        as_of_ms: now_ms(),
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
        polymarket_url: polymarket_event_url(event.slug.as_deref()),
        gamma_url: format!("{GAMMA_ENDPOINT}/events/{}", event.id),
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
    let event_slug = event.and_then(|value| value.slug.clone());

    MarketDetail {
        as_of_ms: now_ms(),
        market_id: market.id.clone(),
        event_id: event.map(|value| value.id.clone()),
        event_title: event.and_then(|value| value.title.clone()),
        event_slug: event_slug.clone(),
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
        polymarket_url: polymarket_event_url(event_slug.as_deref()),
        gamma_url: gamma_market_url(&market.id),
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
        tick_size: book.tick_size.as_decimal().to_string(),
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
        pusd_size: activity.usdc_size.to_string(),
        price: activity.price.map(|value| value.to_string()),
        side: activity.side.map(|value| value.to_string()),
        title: activity.title,
        outcome: activity.outcome,
        transaction_hash: activity.transaction_hash.to_string(),
    }
}

fn analyze_book(
    book: &OrderBookSummaryResponse,
    depth: usize,
    price_band: Decimal,
    sample_shares: Decimal,
) -> OrderBookAnalysisOutput {
    let mut bids = book
        .bids
        .iter()
        .map(|level| (level.price, level.size))
        .collect::<Vec<_>>();
    bids.sort_by(|left, right| right.0.cmp(&left.0));
    bids.truncate(depth);
    let mut asks = book
        .asks
        .iter()
        .map(|level| (level.price, level.size))
        .collect::<Vec<_>>();
    asks.sort_by_key(|(price, _)| *price);
    asks.truncate(depth);

    let best_bid = bids.first().copied();
    let best_ask = asks.first().copied();
    let spread = best_bid.zip(best_ask).map(|((bid, _), (ask, _))| ask - bid);
    let midpoint = best_bid
        .zip(best_ask)
        .map(|((bid, _), (ask, _))| (bid + ask) / Decimal::TWO);
    let microprice =
        best_bid
            .zip(best_ask)
            .and_then(|((bid_price, bid_size), (ask_price, ask_size))| {
                let top_size = bid_size + ask_size;
                (top_size > Decimal::ZERO)
                    .then(|| (ask_price * bid_size + bid_price * ask_size) / top_size)
            });
    let top_level_imbalance = best_bid
        .zip(best_ask)
        .and_then(|((_, bid_size), (_, ask_size))| imbalance_percent(bid_size, ask_size));

    let (bid_depth_shares, bid_depth_notional) = level_totals(&bids);
    let (ask_depth_shares, ask_depth_notional) = level_totals(&asks);
    let depth_imbalance = imbalance_percent(bid_depth_shares, ask_depth_shares);

    let near_bids = best_bid.map_or_else(Vec::new, |(price, _)| {
        let minimum = price - price_band;
        bids.iter()
            .copied()
            .filter(|(level_price, _)| *level_price >= minimum)
            .collect::<Vec<_>>()
    });
    let near_asks = best_ask.map_or_else(Vec::new, |(price, _)| {
        let maximum = price + price_band;
        asks.iter()
            .copied()
            .filter(|(level_price, _)| *level_price <= maximum)
            .collect::<Vec<_>>()
    });
    let (near_touch_bid_shares, near_touch_bid_notional) = level_totals(&near_bids);
    let (near_touch_ask_shares, near_touch_ask_notional) = level_totals(&near_asks);

    OrderBookAnalysisOutput {
        token_id: book.asset_id.to_string(),
        condition_id: book.market.to_string(),
        book_timestamp: book.timestamp.to_rfc3339(),
        book_hash: book.hash.clone(),
        tick_size: book.tick_size.as_decimal().to_string(),
        min_order_size: book.min_order_size.to_string(),
        last_trade_price: book.last_trade_price.map(|value| value.to_string()),
        best_bid: best_bid.map(|(price, _)| price.to_string()),
        best_ask: best_ask.map(|(price, _)| price.to_string()),
        spread: spread.map(|value| value.to_string()),
        midpoint: midpoint.map(|value| value.to_string()),
        microprice: microprice.map(|value| value.to_string()),
        top_bid_size: best_bid.map(|(_, size)| size.to_string()),
        top_ask_size: best_ask.map(|(_, size)| size.to_string()),
        top_level_imbalance_percent: top_level_imbalance.map(|value| value.to_string()),
        depth_limit: depth,
        bid_levels_considered: bids.len(),
        ask_levels_considered: asks.len(),
        bid_depth_shares: bid_depth_shares.to_string(),
        ask_depth_shares: ask_depth_shares.to_string(),
        bid_depth_notional: bid_depth_notional.to_string(),
        ask_depth_notional: ask_depth_notional.to_string(),
        depth_imbalance_percent: depth_imbalance.map(|value| value.to_string()),
        near_touch_price_band: price_band.to_string(),
        near_touch_bid_shares: near_touch_bid_shares.to_string(),
        near_touch_ask_shares: near_touch_ask_shares.to_string(),
        near_touch_bid_notional: near_touch_bid_notional.to_string(),
        near_touch_ask_notional: near_touch_ask_notional.to_string(),
        sample_shares: sample_shares.to_string(),
        sample_buy: simulate_book(book, "buy", sample_shares),
        sample_sell: simulate_book(book, "sell", sample_shares),
        methodology: "Uses the current public L2 book. Microprice weights the best prices by opposite-side displayed size; imbalance is (bid size - ask size) / total size. Depth and near-touch totals use at most depth_limit levels per side. Sample executions walk the full displayed book and exclude fees, latency, queue position, and subsequent price changes. Descriptive only; not a prediction or trading recommendation.".to_owned(),
    }
}

fn level_totals(levels: &[(Decimal, Decimal)]) -> (Decimal, Decimal) {
    levels.iter().fold(
        (Decimal::ZERO, Decimal::ZERO),
        |(shares, notional), (price, size)| (shares + *size, notional + *price * *size),
    )
}

fn imbalance_percent(bid_size: Decimal, ask_size: Decimal) -> Option<Decimal> {
    let total = bid_size + ask_size;
    (total > Decimal::ZERO).then(|| (bid_size - ask_size) / total * Decimal::from(100))
}

type BestQuote = (Option<(Decimal, Decimal)>, Option<(Decimal, Decimal)>);

fn best_quotes(book: &OrderBookSummaryResponse) -> BestQuote {
    let best_bid = book
        .bids
        .iter()
        .map(|level| (level.price, level.size))
        .max_by_key(|(price, _)| *price);
    let best_ask = book
        .asks
        .iter()
        .map(|level| (level.price, level.size))
        .min_by_key(|(price, _)| *price);
    (best_bid, best_ask)
}

fn compact_microstructure(
    outcome: String,
    analysis: OrderBookAnalysisOutput,
) -> OutcomeMicrostructureSummary {
    OutcomeMicrostructureSummary {
        outcome,
        token_id: analysis.token_id,
        best_bid: analysis.best_bid,
        best_ask: analysis.best_ask,
        spread: analysis.spread,
        midpoint: analysis.midpoint,
        microprice: analysis.microprice,
        top_level_imbalance_percent: analysis.top_level_imbalance_percent,
        bid_depth_shares: analysis.bid_depth_shares,
        ask_depth_shares: analysis.ask_depth_shares,
        depth_imbalance_percent: analysis.depth_imbalance_percent,
        near_touch_bid_shares: analysis.near_touch_bid_shares,
        near_touch_ask_shares: analysis.near_touch_ask_shares,
        sample_buy_average_price: analysis.sample_buy.average_price,
        sample_buy_slippage_bps: analysis.sample_buy.slippage_bps,
        sample_buy_complete: analysis.sample_buy.complete_fill,
        sample_sell_average_price: analysis.sample_sell.average_price,
        sample_sell_slippage_bps: analysis.sample_sell.slippage_bps,
        sample_sell_complete: analysis.sample_sell.complete_fill,
    }
}

fn binary_complement_check(quotes: &[BestQuote]) -> Option<BinaryComplementCheck> {
    if quotes.len() != 2 {
        return None;
    }
    let asks = quotes[0].1.zip(quotes[1].1);
    let bids = quotes[0].0.zip(quotes[1].0);

    Some(BinaryComplementCheck {
        best_ask_sum: asks.map(|((left, _), (right, _))| (left + right).to_string()),
        buy_both_gross_edge_per_share: asks
            .map(|((left, _), (right, _))| (Decimal::ONE - left - right).to_string()),
        buy_both_top_level_capacity_shares: asks
            .map(|((_, left), (_, right))| left.min(right).to_string()),
        best_bid_sum: bids.map(|((left, _), (right, _))| (left + right).to_string()),
        sell_both_gross_edge_per_share: bids
            .map(|((left, _), (right, _))| (left + right - Decimal::ONE).to_string()),
        sell_both_top_level_capacity_shares: bids
            .map(|((_, left), (_, right))| left.min(right).to_string()),
    })
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
    use polymarket_client_sdk_v2::{
        clob::types::{TickSize, response::OrderSummary},
        types::{B256, DateTime, U256},
    };

    fn order(price: &str, size: &str) -> OrderSummary {
        OrderSummary::builder()
            .price(price.parse::<Decimal>().unwrap())
            .size(size.parse::<Decimal>().unwrap())
            .build()
    }

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
    fn history_downsampling_is_bounded_and_keeps_endpoints() {
        let points = (0..10)
            .map(|timestamp| PricePoint {
                timestamp,
                price: timestamp.to_string(),
            })
            .collect::<Vec<_>>();
        let sampled = downsample_points(points, 4);
        assert_eq!(sampled.len(), 4);
        assert_eq!(sampled.first().unwrap().timestamp, 0);
        assert_eq!(sampled.last().unwrap().timestamp, 9);
        assert!(
            sampled
                .windows(2)
                .all(|pair| pair[0].timestamp < pair[1].timestamp)
        );
    }

    #[test]
    fn order_book_analysis_calculates_microstructure_and_impact() {
        let book = OrderBookSummaryResponse::builder()
            .market(B256::ZERO)
            .asset_id(U256::from(123_u64))
            .timestamp(DateTime::from_timestamp_millis(1_700_000_000_000).unwrap())
            .bids(vec![order("0.48", "70"), order("0.49", "30")])
            .asks(vec![order("0.52", "40"), order("0.51", "10")])
            .min_order_size("5".parse::<Decimal>().unwrap())
            .neg_risk(false)
            .tick_size(TickSize::Hundredth)
            .build();

        let analysis = analyze_book(
            &book,
            2,
            "0.01".parse::<Decimal>().unwrap(),
            Decimal::from(20),
        );

        assert_eq!(
            analysis
                .microprice
                .as_deref()
                .unwrap()
                .parse::<Decimal>()
                .unwrap(),
            "0.505".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            analysis
                .top_level_imbalance_percent
                .as_deref()
                .unwrap()
                .parse::<Decimal>()
                .unwrap(),
            Decimal::from(50)
        );
        assert_eq!(analysis.bid_depth_shares, "100");
        assert_eq!(analysis.ask_depth_shares, "50");
        assert_eq!(analysis.near_touch_bid_shares, "100");
        assert_eq!(analysis.near_touch_ask_shares, "50");
        assert_eq!(analysis.sample_buy.filled_shares, "20");
        assert_eq!(
            analysis
                .sample_buy
                .average_price
                .as_deref()
                .unwrap()
                .parse::<Decimal>()
                .unwrap(),
            "0.515".parse::<Decimal>().unwrap()
        );
        assert_eq!(analysis.sample_sell.worst_price.as_deref(), Some("0.49"));
    }

    #[test]
    fn binary_complement_check_reports_gross_edges_and_capacity() {
        let quotes = [
            (
                Some(("0.48".parse().unwrap(), "70".parse().unwrap())),
                Some(("0.51".parse().unwrap(), "10".parse().unwrap())),
            ),
            (
                Some(("0.47".parse().unwrap(), "25".parse().unwrap())),
                Some(("0.50".parse().unwrap(), "40".parse().unwrap())),
            ),
        ];
        let check = binary_complement_check(&quotes).unwrap();

        assert_eq!(check.best_ask_sum.as_deref(), Some("1.01"));
        assert_eq!(
            check.buy_both_gross_edge_per_share.as_deref(),
            Some("-0.01")
        );
        assert_eq!(
            check.buy_both_top_level_capacity_shares.as_deref(),
            Some("10")
        );
        assert_eq!(check.best_bid_sum.as_deref(), Some("0.95"));
        assert_eq!(
            check.sell_both_gross_edge_per_share.as_deref(),
            Some("-0.05")
        );
        assert_eq!(
            check.sell_both_top_level_capacity_shares.as_deref(),
            Some("25")
        );
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
        assert_eq!(preview.maximum_notional_pusd, "5.0");
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
