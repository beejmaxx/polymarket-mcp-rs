use std::collections::HashMap;

use polymarket_client_sdk_v2::{
    clob::types::response::OrderBookSummaryResponse,
    gamma::types::response::{Event, Market},
    types::Decimal,
};

use crate::{
    error::AppError,
    polymarket::{CLOB_V2_ENDPOINT, GAMMA_ENDPOINT, PolymarketClient},
    types::{
        MarketDetail, MarketSummary, OrderBookSnapshot, OutcomeDetail, OutcomeQuote,
        SearchMarketsOutput, ServerStatus,
    },
};

const DEFAULT_SEARCH_LIMIT: u8 = 10;
const MAX_SEARCH_LIMIT: u8 = 25;

#[derive(Clone, Debug)]
pub struct App {
    client: PolymarketClient,
}

impl App {
    pub fn new() -> Result<Self, AppError> {
        Ok(Self {
            client: PolymarketClient::new()?,
        })
    }

    #[must_use]
    pub fn status(&self) -> ServerStatus {
        ServerStatus {
            name: env!("CARGO_PKG_NAME").to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            mode: "read-only".to_owned(),
            clob_protocol: "v2".to_owned(),
            gamma_endpoint: GAMMA_ENDPOINT.to_owned(),
            clob_endpoint: CLOB_V2_ENDPOINT.to_owned(),
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
            .take(usize::from(limit))
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

        Ok(SearchMarketsOutput {
            query,
            count: markets.len(),
            markets,
        })
    }

    pub async fn get_market(&self, market_id: String) -> Result<MarketDetail, AppError> {
        let market_id = validate_nonempty("market_id", market_id)?;
        let market = self.client.market(&market_id).await?;
        let token_ids = market.clob_token_ids.clone().unwrap_or_default();
        let books = if market.enable_order_book.unwrap_or(false) && !market.closed.unwrap_or(false)
        {
            self.client.order_books(&token_ids).await?
        } else {
            Vec::new()
        };

        Ok(market_detail(&market, books))
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
}
