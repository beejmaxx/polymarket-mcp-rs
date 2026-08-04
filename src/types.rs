use rmcp::{schemars, schemars::JsonSchema};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchMarketsInput {
    #[schemars(description = "Words from the event title, market question, or topic")]
    pub query: String,

    #[schemars(description = "Maximum markets to return; defaults to 10 and is capped at 25")]
    pub limit: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetMarketInput {
    #[schemars(description = "Gamma market ID; provide exactly one market identifier")]
    pub market_id: Option<String>,

    #[schemars(description = "Gamma market slug; provide exactly one market identifier")]
    pub slug: Option<String>,

    #[schemars(description = "0x-prefixed market condition ID; provide exactly one identifier")]
    pub condition_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListMarketsInput {
    #[schemars(description = "Maximum markets to return; defaults to 25 and is capped at 100")]
    pub limit: Option<u16>,
    #[schemars(description = "Number of events to skip; defaults to zero")]
    pub offset: Option<u32>,
    #[schemars(description = "Optional Gamma topic tag slug, such as politics or crypto")]
    pub tag_slug: Option<String>,
    #[schemars(description = "Include only featured events when true")]
    pub featured: Option<bool>,
    #[schemars(description = "Minimum event liquidity as an exact decimal string")]
    pub min_liquidity: Option<String>,
    #[schemars(description = "Minimum event volume as an exact decimal string")]
    pub min_volume: Option<String>,
    #[schemars(description = "Sort field: volume_24h, volume, liquidity, start_date, or end_date")]
    pub sort_by: Option<String>,
    #[schemars(description = "Sort in ascending order; defaults to false")]
    pub ascending: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetEventInput {
    #[schemars(description = "Gamma event ID; provide either event_id or slug")]
    pub event_id: Option<String>,
    #[schemars(description = "Gamma event slug; provide either slug or event_id")]
    pub slug: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetOrderBookInput {
    #[schemars(description = "Decimal CLOB outcome token ID")]
    pub token_id: String,
    #[schemars(
        description = "Maximum bid and ask levels to return; defaults to 20, capped at 100"
    )]
    pub depth: Option<u16>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeOrderBookInput {
    #[schemars(description = "Decimal CLOB outcome token ID")]
    pub token_id: String,
    #[schemars(
        description = "Maximum price levels per side used for aggregate depth metrics; defaults to 50 and is capped at 100"
    )]
    pub depth: Option<u16>,
    #[schemars(
        description = "Absolute price distance from each best price used for near-touch liquidity; defaults to 0.02"
    )]
    pub price_band: Option<String>,
    #[schemars(
        description = "Outcome shares used for two-sided execution-impact estimates; defaults to 100"
    )]
    pub sample_shares: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScanMarketMicrostructureInput {
    #[schemars(
        description = "Maximum active markets to inspect; defaults to 10 and is capped at 20"
    )]
    pub limit: Option<u8>,
    #[schemars(description = "Optional Gamma topic tag slug, such as politics or crypto")]
    pub tag_slug: Option<String>,
    #[schemars(description = "Optional minimum market liquidity as an exact decimal string")]
    pub min_liquidity: Option<String>,
    #[schemars(
        description = "Maximum levels per outcome side used for depth metrics; defaults to 20 and is capped at 100"
    )]
    pub depth: Option<u16>,
    #[schemars(description = "Absolute near-touch price distance; defaults to 0.02")]
    pub price_band: Option<String>,
    #[schemars(description = "Shares used for sample buy and sell impact; defaults to 100")]
    pub sample_shares: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetPriceHistoryInput {
    #[schemars(description = "Decimal CLOB outcome token ID")]
    pub token_id: String,
    #[schemars(description = "Preset range: 1m, 1h, 6h, 1d, 1w, or max; defaults to 1d")]
    pub interval: Option<String>,
    #[schemars(
        description = "Unix start time; must be supplied together with end_ts instead of interval"
    )]
    pub start_ts: Option<i64>,
    #[schemars(
        description = "Unix end time; must be supplied together with start_ts instead of interval"
    )]
    pub end_ts: Option<i64>,
    #[schemars(description = "Sampling fidelity in minutes")]
    pub fidelity: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetMarketHoldersInput {
    #[schemars(description = "0x-prefixed market condition ID")]
    pub condition_id: String,
    #[schemars(description = "Maximum holders per outcome token; defaults to 20, capped at 20")]
    pub limit: Option<u8>,
    #[schemars(description = "Minimum outcome-token balance; defaults to 1")]
    pub min_balance: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompareMarketsInput {
    #[schemars(description = "Two to ten Gamma market IDs")]
    pub market_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WalletPageInput {
    #[schemars(description = "0x-prefixed public Polymarket proxy-wallet address")]
    pub wallet: String,
    #[schemars(description = "Maximum records; defaults to 100 and is capped at 500")]
    pub limit: Option<u16>,
    #[schemars(description = "Pagination offset; defaults to zero and is capped at 10000")]
    pub offset: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WalletActivityInput {
    #[schemars(description = "0x-prefixed public Polymarket proxy-wallet address")]
    pub wallet: String,
    #[schemars(description = "Maximum records; defaults to 100 and is capped at 500")]
    pub limit: Option<u16>,
    #[schemars(description = "Pagination offset; defaults to zero and is capped at 10000")]
    pub offset: Option<u32>,
    #[schemars(description = "Optional inclusive Unix start timestamp")]
    pub start_ts: Option<u64>,
    #[schemars(description = "Optional inclusive Unix end timestamp")]
    pub end_ts: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WalletInput {
    #[schemars(description = "0x-prefixed public Polymarket proxy-wallet address")]
    pub wallet: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WatchMarketsInput {
    #[schemars(description = "One to fifty decimal CLOB outcome token IDs")]
    pub token_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WatchIdInput {
    #[schemars(description = "Watch ID returned by watch_markets")]
    pub watch_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetLiveSnapshotInput {
    #[schemars(description = "Watch ID returned by watch_markets")]
    pub watch_id: String,
    #[schemars(description = "Maximum levels per side; defaults to 20, capped at 100")]
    pub depth: Option<u16>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetRealtimeEventsInput {
    #[schemars(description = "Watch ID returned by watch_markets")]
    pub watch_id: String,
    #[schemars(description = "Return events after this sequence number")]
    pub after_sequence: Option<u64>,
    #[schemars(description = "Maximum events; defaults to 100 and is capped at 500")]
    pub limit: Option<u16>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartRecordingInput {
    #[schemars(
        description = "Active watch ID whose initial and websocket books should be recorded"
    )]
    pub watch_id: String,
    #[schemars(description = "Optional human-readable recording label")]
    pub label: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordingIdInput {
    #[schemars(description = "Recording ID returned by start_recording")]
    pub recording_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListRecordingsInput {
    #[schemars(description = "Maximum recordings to return; defaults to 50, capped at 200")]
    pub limit: Option<u16>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReplayMarketInput {
    pub recording_id: String,
    #[schemars(description = "Optional decimal token ID filter")]
    pub token_id: Option<String>,
    #[schemars(description = "Optional inclusive upstream timestamp in milliseconds")]
    pub start_ms: Option<i64>,
    #[schemars(description = "Optional inclusive upstream timestamp in milliseconds")]
    pub end_ms: Option<i64>,
    #[schemars(description = "Maximum snapshots; defaults to 100, capped at 5000")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReplayEventsInput {
    #[schemars(description = "Recording ID returned by start_recording")]
    pub recording_id: String,
    #[schemars(description = "Return events after this watch-local sequence number")]
    pub after_sequence: Option<u64>,
    #[schemars(description = "Optional inclusive upstream timestamp in milliseconds")]
    pub start_ms: Option<i64>,
    #[schemars(description = "Optional inclusive upstream timestamp in milliseconds")]
    pub end_ms: Option<i64>,
    #[schemars(description = "Maximum events; defaults to 100 and is capped at 5000")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SimulateOrderInput {
    #[schemars(description = "Decimal CLOB outcome token ID")]
    pub token_id: String,
    #[schemars(description = "Order side: buy consumes asks; sell consumes bids")]
    pub side: String,
    #[schemars(description = "Requested outcome shares as an exact positive decimal string")]
    pub shares: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PreviewOrderInput {
    pub token_id: String,
    #[schemars(description = "limit or market")]
    pub kind: String,
    #[schemars(description = "buy or sell")]
    pub side: String,
    #[schemars(description = "Limit shares; market-buy USDC; market-sell shares")]
    pub amount: String,
    #[schemars(description = "Required exact decimal price for limit orders")]
    pub price: Option<String>,
    #[schemars(description = "GTC for limit; FOK or FAK for market; safe defaults apply")]
    pub order_type: Option<String>,
    #[schemars(
        description = "Fetch and validate current exchange tick/minimum rules; defaults true"
    )]
    pub live_validation: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlaceApprovedOrderInput {
    pub approval_id: String,
    #[schemars(
        description = "Must be true; approvals are single-use and expire after five minutes"
    )]
    pub confirm: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ApprovalIdInput {
    pub approval_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlaceBatchOrdersInput {
    #[schemars(description = "One to ten fresh approval IDs")]
    pub approval_ids: Vec<String>,
    #[schemars(description = "Must exactly equal PLACE_BATCH")]
    pub confirmation: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListOpenOrdersInput {
    pub token_id: Option<String>,
    #[schemars(description = "Opaque cursor returned by the previous page")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListAccountTradesInput {
    pub token_id: Option<String>,
    #[schemars(description = "Opaque cursor returned by the previous page")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OrderIdInput {
    pub order_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CancelOrderInput {
    pub order_id: String,
    #[schemars(description = "Must be true")]
    pub confirm: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CancelAllOrdersInput {
    #[schemars(description = "Must exactly equal CANCEL_ALL")]
    pub confirmation: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BalanceAllowanceInput {
    #[schemars(description = "collateral for pUSD or conditional for outcome tokens")]
    pub asset_type: String,
    #[schemars(description = "Required decimal outcome token ID for conditional assets")]
    pub token_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CancelMarketOrdersInput {
    #[schemars(description = "Optional 0x-prefixed condition ID")]
    pub condition_id: Option<String>,
    #[schemars(description = "Optional decimal outcome token ID")]
    pub token_id: Option<String>,
    #[schemars(description = "Must exactly equal CANCEL_MARKET")]
    pub confirmation: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ServerStatus {
    pub name: String,
    pub version: String,
    pub mode: String,
    pub clob_protocol: String,
    pub gamma_endpoint: String,
    pub clob_endpoint: String,
    pub data_endpoint: String,
    pub websocket_endpoint: String,
    pub database_path: String,
    pub tool_profile: String,
    pub tool_count: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ListMarketsOutput {
    pub count: usize,
    pub markets: Vec<MarketSummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct EventDetail {
    pub event_id: String,
    pub title: Option<String>,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub active: Option<bool>,
    pub closed: Option<bool>,
    pub featured: Option<bool>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub volume: Option<String>,
    pub volume_24h: Option<String>,
    pub liquidity: Option<String>,
    pub tags: Vec<TagSummary>,
    pub markets: Vec<MarketSummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct TagSummary {
    pub id: String,
    pub label: Option<String>,
    pub slug: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct SearchMarketsOutput {
    pub query: String,
    pub count: usize,
    pub markets: Vec<MarketSummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct MarketSummary {
    pub event_id: String,
    pub event_title: Option<String>,
    pub market_id: String,
    pub question: Option<String>,
    pub slug: Option<String>,
    pub active: Option<bool>,
    pub closed: Option<bool>,
    pub accepting_orders: Option<bool>,
    pub end_date: Option<String>,
    pub volume_24h: Option<String>,
    pub liquidity: Option<String>,
    pub outcomes: Vec<OutcomeQuote>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct MarketDetail {
    pub market_id: String,
    pub event_id: Option<String>,
    pub event_title: Option<String>,
    pub question: Option<String>,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub resolution_source: Option<String>,
    pub condition_id: Option<String>,
    pub active: Option<bool>,
    pub closed: Option<bool>,
    pub accepting_orders: Option<bool>,
    pub end_date: Option<String>,
    pub volume: Option<String>,
    pub volume_24h: Option<String>,
    pub liquidity: Option<String>,
    pub outcomes: Vec<OutcomeDetail>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OutcomeQuote {
    pub name: String,
    pub token_id: Option<String>,
    pub gamma_price: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OutcomeDetail {
    pub name: String,
    pub token_id: Option<String>,
    pub gamma_price: Option<String>,
    pub order_book: Option<OrderBookSnapshot>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OrderBookSnapshot {
    pub token_id: String,
    pub timestamp: String,
    pub hash: Option<String>,
    pub best_bid: Option<String>,
    pub best_ask: Option<String>,
    pub spread: Option<String>,
    pub midpoint: Option<String>,
    pub bid_levels: usize,
    pub ask_levels: usize,
    pub min_order_size: String,
    pub tick_size: String,
    pub last_trade_price: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct PriceLevel {
    pub price: String,
    pub size: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OrderBookDetail {
    pub summary: OrderBookSnapshot,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OrderBookAnalysisOutput {
    pub token_id: String,
    pub condition_id: String,
    pub book_timestamp: String,
    pub book_hash: Option<String>,
    pub tick_size: String,
    pub min_order_size: String,
    pub last_trade_price: Option<String>,
    pub best_bid: Option<String>,
    pub best_ask: Option<String>,
    pub spread: Option<String>,
    pub midpoint: Option<String>,
    pub microprice: Option<String>,
    pub top_bid_size: Option<String>,
    pub top_ask_size: Option<String>,
    pub top_level_imbalance_percent: Option<String>,
    pub depth_limit: usize,
    pub bid_levels_considered: usize,
    pub ask_levels_considered: usize,
    pub bid_depth_shares: String,
    pub ask_depth_shares: String,
    pub bid_depth_notional: String,
    pub ask_depth_notional: String,
    pub depth_imbalance_percent: Option<String>,
    pub near_touch_price_band: String,
    pub near_touch_bid_shares: String,
    pub near_touch_ask_shares: String,
    pub near_touch_bid_notional: String,
    pub near_touch_ask_notional: String,
    pub sample_shares: String,
    pub sample_buy: SimulationOutput,
    pub sample_sell: SimulationOutput,
    pub methodology: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ScanMarketMicrostructureOutput {
    pub market_count: usize,
    pub outcome_book_count: usize,
    pub book_error_count: usize,
    pub markets: Vec<MarketMicrostructureSummary>,
    pub methodology: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct MarketMicrostructureSummary {
    pub market_id: String,
    pub question: Option<String>,
    pub slug: Option<String>,
    pub volume_24h: Option<String>,
    pub liquidity: Option<String>,
    pub outcomes: Vec<OutcomeMicrostructureSummary>,
    pub binary_complement: Option<BinaryComplementCheck>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OutcomeMicrostructureSummary {
    pub outcome: String,
    pub token_id: String,
    pub best_bid: Option<String>,
    pub best_ask: Option<String>,
    pub spread: Option<String>,
    pub midpoint: Option<String>,
    pub microprice: Option<String>,
    pub top_level_imbalance_percent: Option<String>,
    pub bid_depth_shares: String,
    pub ask_depth_shares: String,
    pub depth_imbalance_percent: Option<String>,
    pub near_touch_bid_shares: String,
    pub near_touch_ask_shares: String,
    pub sample_buy_average_price: Option<String>,
    pub sample_buy_slippage_bps: Option<String>,
    pub sample_buy_complete: bool,
    pub sample_sell_average_price: Option<String>,
    pub sample_sell_slippage_bps: Option<String>,
    pub sample_sell_complete: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct BinaryComplementCheck {
    pub best_ask_sum: Option<String>,
    pub buy_both_gross_edge_per_share: Option<String>,
    pub buy_both_top_level_capacity_shares: Option<String>,
    pub best_bid_sum: Option<String>,
    pub sell_both_gross_edge_per_share: Option<String>,
    pub sell_both_top_level_capacity_shares: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct PriceHistoryOutput {
    pub token_id: String,
    pub point_count: usize,
    pub points: Vec<PricePoint>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct PricePoint {
    pub timestamp: i64,
    pub price: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct MarketHoldersOutput {
    pub condition_id: String,
    pub outcomes: Vec<OutcomeHolders>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OutcomeHolders {
    pub token_id: String,
    pub holders: Vec<HolderDetail>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct HolderDetail {
    pub wallet: String,
    pub amount: String,
    pub outcome_index: i32,
    pub pseudonym: Option<String>,
    pub name: Option<String>,
    pub verified: Option<bool>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct CompareMarketsOutput {
    pub count: usize,
    pub markets: Vec<MarketDetail>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct WalletPositionsOutput {
    pub wallet: String,
    pub count: usize,
    pub positions: Vec<WalletPosition>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct WalletPosition {
    pub token_id: String,
    pub condition_id: String,
    pub title: String,
    pub market_slug: String,
    pub event_slug: String,
    pub outcome: String,
    pub size: String,
    pub average_price: String,
    pub current_price: String,
    pub initial_value: String,
    pub current_value: String,
    pub cash_pnl: String,
    pub percent_pnl: String,
    pub realized_pnl: String,
    pub redeemable: bool,
    pub mergeable: bool,
    pub end_date: Option<String>,
    pub negative_risk: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct WalletValueOutput {
    pub wallet: String,
    pub value_usdc: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct WalletTradesOutput {
    pub wallet: String,
    pub count: usize,
    pub trades: Vec<WalletTrade>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct WalletTrade {
    pub side: String,
    pub token_id: String,
    pub condition_id: String,
    pub size: String,
    pub price: String,
    pub timestamp: i64,
    pub title: String,
    pub market_slug: String,
    pub event_slug: String,
    pub outcome: String,
    pub transaction_hash: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct WalletActivityOutput {
    pub wallet: String,
    pub count: usize,
    pub activities: Vec<WalletActivity>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct WalletActivity {
    pub activity_type: String,
    pub timestamp: i64,
    pub condition_id: Option<String>,
    pub token_id: Option<String>,
    pub size: String,
    pub usdc_size: String,
    pub price: Option<String>,
    pub side: Option<String>,
    pub title: Option<String>,
    pub outcome: Option<String>,
    pub transaction_hash: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct WalletRiskOutput {
    pub wallet: String,
    pub position_count: usize,
    pub total_current_value: String,
    pub total_initial_value: String,
    pub total_unrealized_pnl: String,
    pub total_realized_pnl: String,
    pub largest_position_value: String,
    pub largest_position_share_percent: Option<String>,
    pub redeemable_position_count: usize,
    pub negative_risk_position_count: usize,
    pub positions: Vec<WalletPosition>,
    pub methodology: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct WatchInfo {
    pub watch_id: String,
    pub token_ids: Vec<String>,
    pub started_at_ms: u64,
    pub last_update_at_ms: Option<u64>,
    pub connection_state: String,
    pub update_count: u64,
    pub rest_seed_count: u64,
    pub websocket_update_count: u64,
    pub price_change_count: u64,
    pub reconnect_count: u64,
    pub error_count: u64,
    pub last_error: Option<String>,
    pub snapshot_count: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct RealtimeStatusOutput {
    pub connection_state: String,
    pub active_watch_count: usize,
    pub subscription_count: usize,
    pub watches: Vec<WatchInfo>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct RealtimeEventsOutput {
    pub watch_id: String,
    pub count: usize,
    pub latest_sequence: u64,
    pub events: Vec<RealtimeEvent>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct RealtimeEvent {
    pub sequence: u64,
    pub event_type: String,
    pub timestamp_ms: i64,
    pub condition_id: Option<String>,
    pub token_id: Option<String>,
    pub price: Option<String>,
    pub size: Option<String>,
    pub side: Option<String>,
    pub best_bid: Option<String>,
    pub best_ask: Option<String>,
    pub old_tick_size: Option<String>,
    pub new_tick_size: Option<String>,
    pub winning_token_id: Option<String>,
    pub winning_outcome: Option<String>,
    pub question: Option<String>,
    pub slug: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct LiveSnapshotOutput {
    pub watch: WatchInfo,
    pub books: Vec<LiveOrderBook>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct LiveOrderBook {
    pub source: String,
    pub token_id: String,
    pub condition_id: String,
    pub upstream_timestamp_ms: i64,
    pub received_at_ms: u64,
    pub feed_age_ms: Option<u64>,
    pub hash: Option<String>,
    pub best_bid: Option<String>,
    pub best_ask: Option<String>,
    pub spread: Option<String>,
    pub midpoint: Option<String>,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct StopWatchOutput {
    pub watch_id: String,
    pub stopped: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct RecordingInfo {
    pub recording_id: String,
    pub watch_id: String,
    pub label: Option<String>,
    pub token_ids: Vec<String>,
    pub started_at_ms: u64,
    pub stopped_at_ms: Option<u64>,
    pub snapshot_count: u64,
    pub dropped_update_count: u64,
    pub event_count: u64,
    pub dropped_event_count: u64,
    pub writer_error_count: u64,
    pub last_writer_error: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ListRecordingsOutput {
    pub count: usize,
    pub recordings: Vec<RecordingInfo>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ReplayMarketOutput {
    pub recording_id: String,
    pub count: usize,
    pub books: Vec<LiveOrderBook>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ReplayEventsOutput {
    pub recording_id: String,
    pub count: usize,
    pub events: Vec<RealtimeEvent>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct SimulationOutput {
    pub token_id: String,
    pub side: String,
    pub requested_shares: String,
    pub filled_shares: String,
    pub unfilled_shares: String,
    pub complete_fill: bool,
    pub total_notional: String,
    pub average_price: Option<String>,
    pub best_price: Option<String>,
    pub worst_price: Option<String>,
    pub slippage_bps: Option<String>,
    pub levels_consumed: usize,
    pub book_timestamp: String,
    pub methodology: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct TradingStatusOutput {
    pub enabled: bool,
    pub signer_configured: bool,
    pub signer_address: Option<String>,
    pub signature_type: String,
    pub funder_address: Option<String>,
    pub automatic_heartbeats_enabled: bool,
    pub max_order_notional_usdc: String,
    pub safety_model: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct BalanceAllowanceOutput {
    pub asset_type: String,
    pub token_id: Option<String>,
    pub balance: String,
    pub allowances: Vec<ContractAllowance>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ContractAllowance {
    pub contract: String,
    pub allowance: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OrderPreviewOutput {
    pub approval_id: String,
    pub expires_at_ms: u64,
    pub token_id: String,
    pub kind: String,
    pub side: String,
    pub amount: String,
    pub amount_unit: String,
    pub price: Option<String>,
    pub order_type: String,
    pub maximum_notional_usdc: String,
    pub live_validation_performed: bool,
    pub current_tick_size: Option<String>,
    pub current_min_order_size: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OrderApprovalStatusOutput {
    pub approval_id: String,
    pub status: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub expires_at_ms: u64,
    pub token_id: String,
    pub kind: String,
    pub side: String,
    pub amount: String,
    pub price: Option<String>,
    pub order_type: String,
    pub order_ids: Vec<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct PlacedOrderOutput {
    pub order_id: String,
    pub status: String,
    pub success: bool,
    pub error_message: Option<String>,
    pub making_amount: String,
    pub taking_amount: String,
    pub transaction_hashes: Vec<String>,
    pub trade_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct BatchPlacedOrdersOutput {
    pub count: usize,
    pub orders: Vec<PlacedOrderOutput>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OpenOrdersOutput {
    pub count: usize,
    pub next_cursor: Option<String>,
    pub orders: Vec<OpenOrder>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OpenOrder {
    pub order_id: String,
    pub status: String,
    pub condition_id: String,
    pub token_id: String,
    pub side: String,
    pub original_size: String,
    pub size_matched: String,
    pub price: String,
    pub outcome: String,
    pub created_at: String,
    pub expiration: String,
    pub order_type: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct CancelOrdersOutput {
    pub canceled: Vec<String>,
    pub not_canceled: Vec<CancelFailure>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct CancelFailure {
    pub order_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AccountTradesOutput {
    pub count: usize,
    pub next_cursor: Option<String>,
    pub trades: Vec<AccountTrade>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AccountTrade {
    pub trade_id: String,
    pub taker_order_id: String,
    pub condition_id: String,
    pub token_id: String,
    pub side: String,
    pub size: String,
    pub price: String,
    pub fee_rate_bps: String,
    pub status: String,
    pub match_time: String,
    pub last_update: String,
    pub outcome: String,
    pub transaction_hash: String,
    pub trader_side: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ToolError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}
