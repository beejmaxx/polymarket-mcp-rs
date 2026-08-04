use rmcp::{
    ServerHandler,
    handler::server::wrapper::{Json, Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};

use crate::{
    App,
    error::AppError,
    types::{
        AccountTradesOutput, BatchPlacedOrdersOutput, CancelAllOrdersInput, CancelOrderInput,
        CancelOrdersOutput, CompareMarketsInput, CompareMarketsOutput, EventDetail, GetEventInput,
        GetLiveSnapshotInput, GetMarketHoldersInput, GetMarketInput, GetOrderBookInput,
        GetPriceHistoryInput, ListAccountTradesInput, ListMarketsInput, ListMarketsOutput,
        ListOpenOrdersInput, ListRecordingsInput, ListRecordingsOutput, LiveSnapshotOutput,
        MarketDetail, MarketHoldersOutput, OpenOrder, OpenOrdersOutput, OrderBookDetail,
        OrderIdInput, OrderPreviewOutput, PlaceApprovedOrderInput, PlaceBatchOrdersInput,
        PlacedOrderOutput, PreviewOrderInput, PriceHistoryOutput, RealtimeStatusOutput,
        RecordingIdInput, RecordingInfo, ReplayMarketInput, ReplayMarketOutput, SearchMarketsInput,
        SearchMarketsOutput, ServerStatus, SimulateOrderInput, SimulationOutput,
        StartRecordingInput, StopWatchOutput, ToolError, TradingStatusOutput, WalletActivityInput,
        WalletActivityOutput, WalletInput, WalletPageInput, WalletPositionsOutput,
        WalletRiskOutput, WalletTradesOutput, WalletValueOutput, WatchIdInput, WatchInfo,
        WatchMarketsInput,
    },
};

#[derive(Clone, Debug)]
pub struct PolymarketServer {
    app: App,
}

impl PolymarketServer {
    #[must_use]
    pub fn new(app: App) -> Self {
        Self { app }
    }
}

#[tool_router]
impl PolymarketServer {
    #[tool(
        name = "server_status",
        description = "Report this server's version, read-only mode, and configured Polymarket API endpoints. This tool makes no network request."
    )]
    fn server_status(&self) -> Json<ServerStatus> {
        Json(self.app.status())
    }

    #[tool(
        name = "search_markets",
        description = "Search active Polymarket events and return matching markets with IDs, outcome token mappings, prices, liquidity, and 24-hour volume."
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
        description = "Get one market by its Gamma market ID, including outcome token IDs and current CLOB V2 order-book summaries when the market is open."
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
        name = "list_markets",
        description = "List active open Polymarket markets, with topic, featured, liquidity, volume, sorting, and pagination filters. Prefer this over text search for scans and rankings."
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
        description = "Get an event by Gamma event ID or slug, including metadata, tags, and all related markets with outcome-token mappings."
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
        name = "get_order_book",
        description = "Get a current CLOB V2 order book by outcome token ID, including exact bid/ask levels, spread, midpoint, tick size, and feed timestamp."
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
        name = "get_price_history",
        description = "Get public historical prices for one CLOB outcome token using a preset interval or explicit Unix timestamp range."
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
            )
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "get_market_holders",
        description = "Get the largest public holders for each outcome token in a market, identified by its 0x-prefixed condition ID."
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
        description = "Compare two to ten markets using the same current metadata, outcome prices, liquidity, volume, and CLOB order-book metrics."
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
        description = "Get current open positions and P/L fields for a public Polymarket proxy-wallet address. No credentials are required."
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
        description = "Get the current total USDC value of open positions for a public Polymarket proxy wallet."
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
        description = "Get public executed-trade history for a Polymarket proxy wallet, with exact token IDs, prices, sizes, sides, and transaction hashes."
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
        description = "Get public on-chain wallet activity including trades, splits, merges, redemptions, rewards, and conversions."
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
        description = "Compute transparent exposure, P/L, concentration, redeemability, and negative-risk summaries from a wallet's public open positions. Descriptive only; not financial advice."
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
        name = "watch_markets",
        description = "Start a persistent public CLOB websocket watch for one to fifty outcome token IDs. Returns immediately with a watch ID while Rust maintains full books in the background."
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
        description = "Read compact current order-book snapshots from a background watch, including upstream and local timestamps, feed age, update counts, and errors."
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
        description = "Report websocket connection state, active subscriptions, update counts, feed timestamps, and the most recent error for every watch."
    )]
    async fn get_realtime_status(&self) -> Json<RealtimeStatusOutput> {
        Json(self.app.get_realtime_status().await)
    }

    #[tool(
        name = "stop_watching",
        description = "Stop and remove a background market-data watch. Safe and idempotent; stopped is false when the watch no longer exists."
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
        description = "Persist a watch's initial full books and subsequent websocket updates to SQLite. Returns a recording ID; dropped broadcast updates are counted explicitly."
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
        description = "Flush and stop an active SQLite recording, returning final snapshot and dropped-update counts."
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
        description = "List local SQLite recordings with token coverage, lifecycle timestamps, snapshot counts, and any detected dropped updates."
    )]
    fn list_recordings(
        &self,
        Parameters(input): Parameters<ListRecordingsInput>,
    ) -> Result<Json<ListRecordingsOutput>, Json<ToolError>> {
        self.app
            .list_recordings(input.limit)
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "replay_market",
        description = "Replay locally recorded full-book states in deterministic upstream-timestamp order, optionally filtered by token and time range."
    )]
    fn replay_market(
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
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "simulate_order",
        description = "Walk the current public CLOB book to estimate fill, notional, average/worst price, and slippage for a buy or sell in outcome shares. Never places an order."
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
        description = "Report whether trading is explicitly enabled, whether a signer is configured, the signer address, the order cap, and the confirmation model. Never authenticates or trades."
    )]
    fn trading_status(&self) -> Json<TradingStatusOutput> {
        Json(self.app.trading_status())
    }

    #[tool(
        name = "preview_order",
        description = "Validate a limit or market order against exact decimal rules and the configured notional cap, then issue a single-use five-minute approval. Never places an order."
    )]
    async fn preview_order(
        &self,
        Parameters(input): Parameters<PreviewOrderInput>,
    ) -> Result<Json<OrderPreviewOutput>, Json<ToolError>> {
        self.app
            .preview_order(
                input.token_id,
                input.kind,
                input.side,
                input.amount,
                input.price,
                input.order_type,
            )
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "place_order",
        description = "Consume a fresh single-use approval and place its exact signed order. Requires POLYMARKET_ENABLE_TRADING=true, a private key, and confirm=true."
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
        description = "Atomically submit one to ten fresh approved orders. The batch total shares the configured notional cap and requires the exact confirmation PLACE_BATCH."
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
        description = "Get one authenticated account order by order ID."
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
        description = "List the authenticated account's open orders, optionally filtered by outcome token ID."
    )]
    async fn list_open_orders(
        &self,
        Parameters(input): Parameters<ListOpenOrdersInput>,
    ) -> Result<Json<OpenOrdersOutput>, Json<ToolError>> {
        self.app
            .list_open_orders(input.token_id)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "list_account_trades",
        description = "List the authenticated account's CLOB trades, optionally filtered by outcome token ID."
    )]
    async fn list_account_trades(
        &self,
        Parameters(input): Parameters<ListAccountTradesInput>,
    ) -> Result<Json<AccountTradesOutput>, Json<ToolError>> {
        self.app
            .list_account_trades(input.token_id)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "cancel_order",
        description = "Cancel one authenticated account order. Requires trading enablement and confirm=true."
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
        description = "Cancel every authenticated account order. Requires the exact confirmation string CANCEL_ALL."
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
}

#[tool_handler]
impl ServerHandler for PolymarketServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Read-only access to Polymarket Gamma discovery, Data API public data, and production CLOB V2 market data. Use search_markets or list_markets first, then inspect returned IDs with get_event, get_market, get_order_book, price history, holders, or comparison tools.",
        )
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
    fn exposes_market_data_tools() {
        let mut names = PolymarketServer::tool_router()
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<Vec<_>>();
        names.sort();

        assert_eq!(
            names,
            [
                "analyze_wallet_risk",
                "cancel_all_orders",
                "cancel_order",
                "compare_markets",
                "get_event",
                "get_live_snapshot",
                "get_market",
                "get_market_holders",
                "get_order",
                "get_order_book",
                "get_price_history",
                "get_realtime_status",
                "get_wallet_activity",
                "get_wallet_positions",
                "get_wallet_trades",
                "get_wallet_value",
                "list_account_trades",
                "list_markets",
                "list_open_orders",
                "list_recordings",
                "place_batch_orders",
                "place_order",
                "preview_order",
                "replay_market",
                "search_markets",
                "server_status",
                "simulate_order",
                "start_recording",
                "stop_recording",
                "stop_watching",
                "trading_status",
                "watch_markets"
            ]
        );
    }

    #[test]
    fn tool_errors_are_structured() {
        let Json(error) = tool_error(AppError::InvalidInput("bad query".to_owned()));
        assert_eq!(error.code, "invalid_input");
        assert!(!error.retryable);
    }
}
