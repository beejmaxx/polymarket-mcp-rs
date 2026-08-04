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
    #[schemars(description = "Gamma market ID returned by search_markets")]
    pub market_id: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ServerStatus {
    pub name: String,
    pub version: String,
    pub mode: String,
    pub clob_protocol: String,
    pub gamma_endpoint: String,
    pub clob_endpoint: String,
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

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ToolError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}
