use std::{fmt, str::FromStr};

use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{
        Implementation, ListResourcesResult, MetaObject, PaginatedRequestParams,
        ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
        ResourceContents, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
};

const MARKET_CARD_URI: &str = "ui://polymarket/market-brief-v1.html";
const MCP_APP_MIME_TYPE: &str = "text/html;profile=mcp-app";
const MARKET_CARD_HTML: &str = include_str!("../web/market-brief.html");

fn market_card_tool_meta() -> MetaObject {
    let mut meta = MetaObject::new();
    meta.insert(
        "ui".to_owned(),
        serde_json::json!({ "resourceUri": MARKET_CARD_URI }),
    );
    // Compatibility alias for ChatGPT clients that predate the MCP Apps field.
    meta.insert(
        "openai/outputTemplate".to_owned(),
        serde_json::Value::String(MARKET_CARD_URI.to_owned()),
    );
    meta.insert(
        "openai/toolInvocation/invoking".to_owned(),
        serde_json::Value::String("Building market brief…".to_owned()),
    );
    meta.insert(
        "openai/toolInvocation/invoked".to_owned(),
        serde_json::Value::String("Market brief ready.".to_owned()),
    );
    meta
}

fn market_card_resource_meta() -> MetaObject {
    let mut meta = MetaObject::new();
    meta.insert(
        "ui".to_owned(),
        serde_json::json!({ "prefersBorder": true }),
    );
    meta
}

use crate::{
    App,
    error::AppError,
    types::{
        AccountTradesOutput, AnalyzeEventConsistencyInput, AnalyzeOrderBookInput, ApprovalIdInput,
        BalanceAllowanceInput, BalanceAllowanceOutput, BatchPlacedOrdersOutput,
        CancelAllOrdersInput, CancelMarketOrdersInput, CancelOrderInput, CancelOrdersOutput,
        CompareMarketsInput, CompareMarketsOutput, EventConsistencyOutput, EventDetail,
        GetEventInput, GetLiveSnapshotInput, GetMarketBriefInput, GetMarketHoldersInput,
        GetMarketInput, GetOrderBookInput, GetPriceHistoryInput, GetRealtimeEventsInput,
        GetUserEventsInput, ListAccountTradesInput, ListMarketsInput, ListMarketsOutput,
        ListOpenOrdersInput, ListRecordingsInput, ListRecordingsOutput, LiveSnapshotOutput,
        MarketBriefOutput, MarketDetail, MarketHoldersOutput, OpenOrder, OpenOrdersOutput,
        OrderApprovalStatusOutput, OrderBookAnalysisOutput, OrderBookDetail, OrderIdInput,
        OrderPreviewOutput, PlaceApprovedOrderInput, PlaceBatchOrdersInput, PlacedOrderOutput,
        PreviewOrderInput, PriceHistoryOutput, RealtimeEventsOutput, RealtimeStatusOutput,
        RecordingIdInput, RecordingInfo, ReplayEventsInput, ReplayEventsOutput, ReplayMarketInput,
        ReplayMarketOutput, ScanMarketMicrostructureInput, ScanMarketMicrostructureOutput,
        SearchMarketsInput, SearchMarketsOutput, ServerStatus, SimulateOrderInput,
        SimulationOutput, StartRecordingInput, StopWatchOutput, ToolError, TradingStatusOutput,
        UserRealtimeEventsOutput, UserRealtimeStatusOutput, UserWatchIdInput, UserWatchInfo,
        WalletActivityInput, WalletActivityOutput, WalletInput, WalletPageInput,
        WalletPositionsOutput, WalletRiskOutput, WalletSummaryInput, WalletSummaryOutput,
        WalletTradesOutput, WalletValueOutput, WatchIdInput, WatchInfo, WatchMarketsInput,
        WatchUserEventsInput,
    },
};

const RESEARCH_ONLY_TOOLS: &[&str] = &[
    "get_live_snapshot",
    "get_realtime_events",
    "get_realtime_status",
    "list_recordings",
    "replay_events",
    "replay_market",
    "start_recording",
    "stop_recording",
    "stop_watching",
    "watch_markets",
];

const TRADING_TOOLS: &[&str] = &[
    "cancel_all_orders",
    "cancel_market_orders",
    "cancel_order",
    "get_balance_allowance",
    "get_order",
    "get_order_approval",
    "get_user_events",
    "get_user_realtime_status",
    "list_account_trades",
    "list_open_orders",
    "place_batch_orders",
    "place_order",
    "preview_order",
    "trading_status",
    "stop_user_watch",
    "watch_user_events",
];

const CHATGPT_TOOLS: &[&str] = &[
    "analyze_event_consistency",
    "analyze_order_book",
    "compare_markets",
    "get_event",
    "get_market_brief",
    "get_price_history",
    "get_wallet_summary",
    "list_markets",
    "scan_market_microstructure",
    "search_markets",
    "simulate_order",
];

/// Controls which capabilities are advertised and callable over MCP.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolProfile {
    /// Compact, stateless, credential-free surface for hosted LLM clients.
    Chatgpt,
    /// Public REST market and wallet data, analysis, and simulation.
    Core,
    /// Core plus realtime watching and local recording/replay.
    #[default]
    Research,
    /// Core plus authenticated account and trading operations.
    Trading,
    /// Every tool, including both research and trading capabilities.
    All,
}

impl ToolProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chatgpt => "chatgpt",
            Self::Core => "core",
            Self::Research => "research",
            Self::Trading => "trading",
            Self::All => "all",
        }
    }

    fn configure(self, router: &mut ToolRouter<PolymarketServer>) {
        if self == Self::Chatgpt {
            let disabled = router
                .list_all()
                .into_iter()
                .map(|tool| tool.name.to_string())
                .filter(|name| !CHATGPT_TOOLS.contains(&name.as_str()))
                .collect::<Vec<_>>();
            for name in disabled {
                router.disable_route(name);
            }
            return;
        }
        let disabled = match self {
            Self::Chatgpt => unreachable!("handled above"),
            Self::Core => RESEARCH_ONLY_TOOLS.iter().chain(TRADING_TOOLS).copied(),
            Self::Research => RESEARCH_ONLY_TOOLS[..0]
                .iter()
                .chain(TRADING_TOOLS)
                .copied(),
            Self::Trading => RESEARCH_ONLY_TOOLS
                .iter()
                .chain(TRADING_TOOLS[..0].iter())
                .copied(),
            Self::All => RESEARCH_ONLY_TOOLS[..0]
                .iter()
                .chain(TRADING_TOOLS[..0].iter())
                .copied(),
        };
        for name in disabled {
            router.disable_route(name.to_owned());
        }
    }
}

impl fmt::Display for ToolProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ToolProfile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chatgpt" => Ok(Self::Chatgpt),
            "core" => Ok(Self::Core),
            "research" => Ok(Self::Research),
            "trading" => Ok(Self::Trading),
            "all" => Ok(Self::All),
            _ => Err(format!(
                "unknown tool profile {value:?}; expected chatgpt, core, research, trading, or all"
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PolymarketServer {
    app: App,
    tool_router: ToolRouter<Self>,
    profile: ToolProfile,
}

impl PolymarketServer {
    #[must_use]
    pub fn new(app: App) -> Self {
        Self::with_profile(app, ToolProfile::default())
    }

    #[must_use]
    pub fn with_profile(app: App, profile: ToolProfile) -> Self {
        let mut tool_router = Self::tool_router();
        profile.configure(&mut tool_router);
        Self {
            app,
            tool_router,
            profile,
        }
    }

    pub fn from_environment(app: App) -> Result<Self, AppError> {
        let profile = std::env::var("POLYMARKET_TOOL_PROFILE")
            .unwrap_or_else(|_| ToolProfile::default().to_string())
            .parse()
            .map_err(AppError::InvalidInput)?;
        Ok(Self::with_profile(app, profile))
    }

    #[must_use]
    pub const fn profile(&self) -> ToolProfile {
        self.profile
    }

    #[must_use]
    pub fn tools(&self) -> Vec<rmcp::model::Tool> {
        self.tool_router.list_all()
    }
}

#[tool_router(router = tool_router)]
impl PolymarketServer {
    #[tool(
        name = "server_status",
        description = "Report this server's version, operating mode, database, and configured Polymarket API endpoints. This tool makes no network request.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn server_status(&self) -> Json<ServerStatus> {
        let mut status = self.app.status();
        status.tool_profile = self.profile.to_string();
        status.tool_count = self.tool_router.list_all().len();
        Json(status)
    }

    #[tool(
        name = "search_markets",
        description = "Search active Polymarket events and return matching markets with IDs, outcome token mappings, prices, liquidity, and 24-hour volume.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn search_markets(
        &self,
        Parameters(input): Parameters<SearchMarketsInput>,
    ) -> Result<Json<SearchMarketsOutput>, Json<ToolError>> {
        self.app
            .search_markets(input.query, input.limit)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_market",
        description = "Get one market by its Gamma market ID, including outcome token IDs and current CLOB V2 order-book summaries when the market is open.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_market(
        &self,
        Parameters(input): Parameters<GetMarketInput>,
    ) -> Result<Json<MarketDetail>, Json<ToolError>> {
        self.app
            .get_market_by_identifier(input.market_id, input.slug, input.condition_id)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_market_brief",
        description = "Build one answer-ready, source-linked market brief from a market ID, slug, or condition ID. Combines current metadata, exact executable books, microstructure, bounded price history, sample fill impact, timestamps, and limitations. Prefer this for a complete market explanation.",
        annotations(read_only_hint = true, open_world_hint = true),
        meta = market_card_tool_meta()
    )]
    async fn get_market_brief(
        &self,
        Parameters(input): Parameters<GetMarketBriefInput>,
    ) -> Result<Json<MarketBriefOutput>, Json<ToolError>> {
        self.app
            .get_market_brief(input)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "list_markets",
        description = "List active open Polymarket markets with stable market-level ranking and pagination plus topic, featured, liquidity, volume, and end-time filters. Prefer this over text search for scans and rankings.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn list_markets(
        &self,
        Parameters(input): Parameters<ListMarketsInput>,
    ) -> Result<Json<ListMarketsOutput>, Json<ToolError>> {
        self.app
            .list_markets(input)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_event",
        description = "Get an event by Gamma event ID or slug, including metadata, tags, and all related markets with outcome-token mappings.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_event(
        &self,
        Parameters(input): Parameters<GetEventInput>,
    ) -> Result<Json<EventDetail>, Json<ToolError>> {
        self.app
            .get_event(input.event_id, input.slug)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "analyze_event_consistency",
        description = "Analyze an event-level mutually exclusive/exhaustive basket only when Gamma explicitly marks the event negative-risk. Sums executable Yes bids/asks, reports top-level capacity and gross edge, and refuses to infer logical relationships from text alone. Descriptive only; never places an order.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn analyze_event_consistency(
        &self,
        Parameters(input): Parameters<AnalyzeEventConsistencyInput>,
    ) -> Result<Json<EventConsistencyOutput>, Json<ToolError>> {
        self.app
            .analyze_event_consistency(input)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_order_book",
        description = "Get a current CLOB V2 order book by outcome token ID, including exact bid/ask levels, spread, midpoint, tick size, and feed timestamp.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_order_book(
        &self,
        Parameters(input): Parameters<GetOrderBookInput>,
    ) -> Result<Json<OrderBookDetail>, Json<ToolError>> {
        self.app
            .get_order_book(input.token_id, input.depth)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "analyze_order_book",
        description = "Compute exact-decimal L2 microstructure metrics for one outcome token: spread, midpoint, microprice, top-level and aggregate imbalance, depth, near-touch liquidity, and two-sided sample execution impact. Descriptive only; never places an order.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn analyze_order_book(
        &self,
        Parameters(input): Parameters<AnalyzeOrderBookInput>,
    ) -> Result<Json<OrderBookAnalysisOutput>, Json<ToolError>> {
        self.app
            .analyze_order_book(input)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "scan_market_microstructure",
        description = "Scan up to 20 high-volume active markets concurrently and return compact per-outcome L2 microstructure, sample execution impact, and binary-market complement checks. Public read-only analysis; never places an order.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn scan_market_microstructure(
        &self,
        Parameters(input): Parameters<ScanMarketMicrostructureInput>,
    ) -> Result<Json<ScanMarketMicrostructureOutput>, Json<ToolError>> {
        self.app
            .scan_market_microstructure(input)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_price_history",
        description = "Get bounded public historical prices for one CLOB outcome token using a preset interval or explicit Unix timestamp range.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_price_history(
        &self,
        Parameters(input): Parameters<GetPriceHistoryInput>,
    ) -> Result<Json<PriceHistoryOutput>, Json<ToolError>> {
        self.app
            .get_price_history(
                input.token_id,
                input.interval,
                input.start_ts,
                input.end_ts,
                input.fidelity,
                input.limit,
            )
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_market_holders",
        description = "Get the largest public holders for each outcome token in a market, identified by its 0x-prefixed condition ID.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_market_holders(
        &self,
        Parameters(input): Parameters<GetMarketHoldersInput>,
    ) -> Result<Json<MarketHoldersOutput>, Json<ToolError>> {
        self.app
            .get_market_holders(input.condition_id, input.limit, input.min_balance)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "compare_markets",
        description = "Compare two to ten markets using the same current metadata, outcome prices, liquidity, volume, and CLOB order-book metrics.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn compare_markets(
        &self,
        Parameters(input): Parameters<CompareMarketsInput>,
    ) -> Result<Json<CompareMarketsOutput>, Json<ToolError>> {
        self.app
            .compare_markets(input.market_ids)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_wallet_positions",
        description = "Get current open positions and P/L fields for a public Polymarket proxy-wallet address. No credentials are required.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_wallet_positions(
        &self,
        Parameters(input): Parameters<WalletPageInput>,
    ) -> Result<Json<WalletPositionsOutput>, Json<ToolError>> {
        self.app
            .get_wallet_positions(input.wallet, input.limit, input.offset)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_wallet_value",
        description = "Get the current total pUSD value of open positions for a public Polymarket proxy wallet.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_wallet_value(
        &self,
        Parameters(input): Parameters<WalletInput>,
    ) -> Result<Json<WalletValueOutput>, Json<ToolError>> {
        self.app
            .get_wallet_value(input.wallet)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_wallet_trades",
        description = "Get public executed-trade history for a Polymarket proxy wallet, with exact token IDs, prices, sizes, sides, and transaction hashes.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_wallet_trades(
        &self,
        Parameters(input): Parameters<WalletPageInput>,
    ) -> Result<Json<WalletTradesOutput>, Json<ToolError>> {
        self.app
            .get_wallet_trades(input.wallet, input.limit, input.offset)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_wallet_activity",
        description = "Get public on-chain wallet activity including trades, splits, merges, redemptions, rewards, and conversions.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_wallet_activity(
        &self,
        Parameters(input): Parameters<WalletActivityInput>,
    ) -> Result<Json<WalletActivityOutput>, Json<ToolError>> {
        self.app
            .get_wallet_activity(
                input.wallet,
                input.limit,
                input.offset,
                input.start_ts,
                input.end_ts,
            )
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "analyze_wallet_risk",
        description = "Compute transparent exposure, P/L, concentration, redeemability, and negative-risk summaries from a wallet's public open positions. Descriptive only; not financial advice.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn analyze_wallet_risk(
        &self,
        Parameters(input): Parameters<WalletInput>,
    ) -> Result<Json<WalletRiskOutput>, Json<ToolError>> {
        self.app
            .analyze_wallet_risk(input.wallet)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_wallet_summary",
        description = "Build one source-linked summary for a public Polymarket proxy wallet: open positions, current value, transparent concentration and P/L metrics, recent trades, recent activity, timestamps, and limitations. No credentials are required.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_wallet_summary(
        &self,
        Parameters(input): Parameters<WalletSummaryInput>,
    ) -> Result<Json<WalletSummaryOutput>, Json<ToolError>> {
        self.app
            .get_wallet_summary(input)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "watch_markets",
        description = "Start a persistent public CLOB websocket watch for one to fifty outcome token IDs. Returns immediately with a watch ID while Rust maintains full books in the background.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn watch_markets(
        &self,
        Parameters(input): Parameters<WatchMarketsInput>,
    ) -> Result<Json<WatchInfo>, Json<ToolError>> {
        self.app
            .watch_markets(input.token_ids)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_live_snapshot",
        description = "Read compact current order-book snapshots from a background watch, including upstream and local timestamps, feed age, update counts, and errors.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_live_snapshot(
        &self,
        Parameters(input): Parameters<GetLiveSnapshotInput>,
    ) -> Result<Json<LiveSnapshotOutput>, Json<ToolError>> {
        self.app
            .get_live_snapshot(input.watch_id, input.depth)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_realtime_status",
        description = "Report websocket connection state, active subscriptions, update counts, feed timestamps, and the most recent error for every watch.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_realtime_status(&self) -> Json<RealtimeStatusOutput> {
        Json(self.app.get_realtime_status().await)
    }

    #[tool(
        name = "get_realtime_events",
        description = "Read sequenced trade, best-bid/ask, tick-size, new-market, and resolution events retained for an active watch.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_realtime_events(
        &self,
        Parameters(input): Parameters<GetRealtimeEventsInput>,
    ) -> Result<Json<RealtimeEventsOutput>, Json<ToolError>> {
        self.app
            .get_realtime_events(input.watch_id, input.after_sequence, input.limit)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "stop_watching",
        description = "Stop and remove a background market-data watch. Safe and idempotent; stopped is false when the watch no longer exists.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn stop_watching(
        &self,
        Parameters(input): Parameters<WatchIdInput>,
    ) -> Result<Json<StopWatchOutput>, Json<ToolError>> {
        self.app
            .stop_watching(input.watch_id)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "start_recording",
        description = "Persist a watch's initial full books and subsequent websocket updates to SQLite. Returns a recording ID; dropped broadcast updates are counted explicitly.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn start_recording(
        &self,
        Parameters(input): Parameters<StartRecordingInput>,
    ) -> Result<Json<RecordingInfo>, Json<ToolError>> {
        self.app
            .start_recording(input.watch_id, input.label)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "stop_recording",
        description = "Flush and stop an active SQLite recording, returning final snapshot and dropped-update counts.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn stop_recording(
        &self,
        Parameters(input): Parameters<RecordingIdInput>,
    ) -> Result<Json<RecordingInfo>, Json<ToolError>> {
        self.app
            .stop_recording(input.recording_id)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "list_recordings",
        description = "List local SQLite recordings with token coverage, lifecycle timestamps, snapshot counts, and any detected dropped updates.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_recordings(
        &self,
        Parameters(input): Parameters<ListRecordingsInput>,
    ) -> Result<Json<ListRecordingsOutput>, Json<ToolError>> {
        self.app
            .list_recordings(input.limit)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "replay_market",
        description = "Replay locally recorded full-book states in deterministic upstream-timestamp order, optionally filtered by token and time range.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn replay_market(
        &self,
        Parameters(input): Parameters<ReplayMarketInput>,
    ) -> Result<Json<ReplayMarketOutput>, Json<ToolError>> {
        self.app
            .replay_market(
                input.recording_id,
                input.token_id,
                input.start_ms,
                input.end_ms,
                input.limit,
            )
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "replay_events",
        description = "Replay sequenced trades, best-price changes, tick-size changes, new-market notices, and resolutions persisted by a local recording. This returns captured observations, not complete exchange history.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn replay_events(
        &self,
        Parameters(input): Parameters<ReplayEventsInput>,
    ) -> Result<Json<ReplayEventsOutput>, Json<ToolError>> {
        self.app
            .replay_events(
                input.recording_id,
                input.after_sequence,
                input.start_ms,
                input.end_ms,
                input.limit,
            )
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "simulate_order",
        description = "Walk the current public CLOB book to estimate fill, notional, average/worst price, and slippage for a buy or sell in outcome shares. Never places an order.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn simulate_order(
        &self,
        Parameters(input): Parameters<SimulateOrderInput>,
    ) -> Result<Json<SimulationOutput>, Json<ToolError>> {
        self.app
            .simulate_order(input.token_id, input.side, input.shares)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "trading_status",
        description = "Report whether trading is explicitly enabled, whether a signer is configured, the signer address, the pUSD order cap, and the confirmation model. Never authenticates or trades.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn trading_status(&self) -> Json<TradingStatusOutput> {
        Json(self.app.trading_status())
    }

    #[tool(
        name = "preview_order",
        description = "Apply local decimal and notional policy checks to a limit or market order, fetch current tick/minimum rules by default, then issue a single-use five-minute approval. This does not guarantee exchange acceptance or a fill and never places an order.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn preview_order(
        &self,
        Parameters(input): Parameters<PreviewOrderInput>,
    ) -> Result<Json<OrderPreviewOutput>, Json<ToolError>> {
        self.app
            .preview_order(input)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_order_approval",
        description = "Read the durable lifecycle and result of an order approval, including ambiguous submission failures.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_order_approval(
        &self,
        Parameters(input): Parameters<ApprovalIdInput>,
    ) -> Result<Json<OrderApprovalStatusOutput>, Json<ToolError>> {
        self.app
            .get_order_approval(input.approval_id)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "place_order",
        description = "Consume a fresh single-use approval and place its exact signed order. Requires POLYMARKET_ENABLE_TRADING=true, a private key, and confirm=true.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn place_order(
        &self,
        Parameters(input): Parameters<PlaceApprovedOrderInput>,
    ) -> Result<Json<PlacedOrderOutput>, Json<ToolError>> {
        self.app
            .place_order(input.approval_id, input.confirm)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "place_batch_orders",
        description = "Atomically submit one to ten fresh approved orders. The batch total shares the configured notional cap and requires the exact confirmation PLACE_BATCH.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn place_batch_orders(
        &self,
        Parameters(input): Parameters<PlaceBatchOrdersInput>,
    ) -> Result<Json<BatchPlacedOrdersOutput>, Json<ToolError>> {
        self.app
            .place_batch_orders(input.approval_ids, input.confirmation)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_order",
        description = "Get one authenticated account order by order ID.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_order(
        &self,
        Parameters(input): Parameters<OrderIdInput>,
    ) -> Result<Json<OpenOrder>, Json<ToolError>> {
        self.app
            .get_order(input.order_id)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "list_open_orders",
        description = "List the authenticated account's open orders, optionally filtered by outcome token ID.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn list_open_orders(
        &self,
        Parameters(input): Parameters<ListOpenOrdersInput>,
    ) -> Result<Json<OpenOrdersOutput>, Json<ToolError>> {
        self.app
            .list_open_orders(input.token_id, input.next_cursor)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "list_account_trades",
        description = "List the authenticated account's CLOB trades, optionally filtered by outcome token ID.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn list_account_trades(
        &self,
        Parameters(input): Parameters<ListAccountTradesInput>,
    ) -> Result<Json<AccountTradesOutput>, Json<ToolError>> {
        self.app
            .list_account_trades(input.token_id, input.next_cursor)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "watch_user_events",
        description = "Start an authenticated CLOB WebSocket watch for this account's order and trade events in one to fifty condition IDs. Requires a configured private key but not trading enablement; it never places or cancels an order.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn watch_user_events(
        &self,
        Parameters(input): Parameters<WatchUserEventsInput>,
    ) -> Result<Json<UserWatchInfo>, Json<ToolError>> {
        self.app
            .watch_user_events(input.condition_ids)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_user_events",
        description = "Read buffered authenticated order and trade events after a watch-local sequence number.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_user_events(
        &self,
        Parameters(input): Parameters<GetUserEventsInput>,
    ) -> Result<Json<UserRealtimeEventsOutput>, Json<ToolError>> {
        self.app
            .get_user_events(input.watch_id, input.after_sequence, input.limit)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_user_realtime_status",
        description = "Report authenticated user-event watches, event counts, and connection errors. This makes no network request.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_user_realtime_status(&self) -> Json<UserRealtimeStatusOutput> {
        Json(self.app.get_user_realtime_status().await)
    }

    #[tool(
        name = "stop_user_watch",
        description = "Stop one authenticated user-event WebSocket watch. This never cancels exchange orders.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn stop_user_watch(
        &self,
        Parameters(input): Parameters<UserWatchIdInput>,
    ) -> Result<Json<StopWatchOutput>, Json<ToolError>> {
        self.app
            .stop_user_watch(input.watch_id)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_balance_allowance",
        description = "Get the authenticated account's pUSD collateral or outcome-token balance and contract allowances.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn get_balance_allowance(
        &self,
        Parameters(input): Parameters<BalanceAllowanceInput>,
    ) -> Result<Json<BalanceAllowanceOutput>, Json<ToolError>> {
        self.app
            .get_balance_allowance(input.asset_type, input.token_id)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "cancel_order",
        description = "Cancel one authenticated account order. Requires trading enablement and confirm=true.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn cancel_order(
        &self,
        Parameters(input): Parameters<CancelOrderInput>,
    ) -> Result<Json<CancelOrdersOutput>, Json<ToolError>> {
        self.app
            .cancel_order(input.order_id, input.confirm)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "cancel_all_orders",
        description = "Cancel every authenticated account order. Requires the exact confirmation string CANCEL_ALL.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn cancel_all_orders(
        &self,
        Parameters(input): Parameters<CancelAllOrdersInput>,
    ) -> Result<Json<CancelOrdersOutput>, Json<ToolError>> {
        self.app
            .cancel_all_orders(input.confirmation)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "cancel_market_orders",
        description = "Cancel authenticated orders for one condition, outcome token, or both. Requires the exact confirmation CANCEL_MARKET.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn cancel_market_orders(
        &self,
        Parameters(input): Parameters<CancelMarketOrdersInput>,
    ) -> Result<Json<CancelOrdersOutput>, Json<ToolError>> {
        self.app
            .cancel_market_orders(input.condition_id, input.token_id, input.confirmation)
            .await
            .map(Json)
            .map_err(tool_error)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for PolymarketServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                format!(
                    "Polymarket market intelligence using the {} tool profile. Public discovery, Data API, CLOB V2 books, analysis, and simulation are available without credentials. Trading tools are hidden by default; enabling the trading or all profile only exposes them, while order mutation separately requires explicit trading enablement and confirmation. Use search_markets or list_markets before inspecting IDs or creating a watch.",
                    self.profile
                ),
            )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(vec![
            Resource::new(MARKET_CARD_URI, "polymarket-market-brief")
                .with_title("Polymarket market brief")
                .with_description("Portable MCP Apps card for get_market_brief results")
                .with_mime_type(MCP_APP_MIME_TYPE),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if request.uri != MARKET_CARD_URI {
            return Err(ErrorData::resource_not_found(
                format!("unknown resource URI: {}", request.uri),
                None,
            ));
        }
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(MARKET_CARD_HTML, MARKET_CARD_URI)
                .with_mime_type(MCP_APP_MIME_TYPE)
                .with_meta(market_card_resource_meta()),
        ])
        .into())
    }
}

fn tool_error(error: AppError) -> Json<ToolError> {
    Json(ToolError {
        code: error.code().to_owned(),
        message: error.to_string(),
        retryable: error.retryable(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_expose_intentional_tool_surfaces() {
        let app = App::new_ephemeral().unwrap();
        let names = |profile| {
            PolymarketServer::with_profile(app.clone(), profile)
                .tools()
                .into_iter()
                .map(|tool| tool.name.to_string())
                .collect::<Vec<_>>()
        };

        let chatgpt = names(ToolProfile::Chatgpt);
        let core = names(ToolProfile::Core);
        let research = names(ToolProfile::Research);
        let trading = names(ToolProfile::Trading);
        let all = names(ToolProfile::All);
        assert_eq!(
            (
                chatgpt.len(),
                core.len(),
                research.len(),
                trading.len(),
                all.len()
            ),
            (11, 20, 30, 36, 46)
        );
        assert!(chatgpt.contains(&"get_market_brief".to_owned()));
        assert!(!chatgpt.contains(&"watch_markets".to_owned()));
        assert!(!chatgpt.contains(&"server_status".to_owned()));
        assert!(!research.contains(&"place_order".to_owned()));
        assert!(!research.contains(&"trading_status".to_owned()));
        assert!(trading.contains(&"place_order".to_owned()));
        assert!(!trading.contains(&"watch_markets".to_owned()));
        assert!(all.contains(&"place_order".to_owned()));
        assert!(all.contains(&"watch_markets".to_owned()));
    }

    #[test]
    fn tool_errors_are_structured() {
        let Json(error) = tool_error(AppError::InvalidInput("bad query".to_owned()));
        assert_eq!(error.code, "invalid_input");
        assert!(!error.retryable);
    }

    #[test]
    fn advertises_project_identity() {
        let server = PolymarketServer::new(App::new_ephemeral().unwrap());
        let info = server.get_info();
        assert_eq!(info.server_info.name, "polymarket-mcp-rs");
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
        assert!(info.instructions.unwrap().contains("research tool profile"));
    }
}
