use polymarket_client_sdk_v2::{
    clob::{
        Client as ClobClient, Config as ClobConfig,
        types::{request::OrderBookSummaryRequest, response::OrderBookSummaryResponse},
    },
    gamma::{
        Client as GammaClient,
        types::{
            request::{MarketByIdRequest, SearchRequest},
            response::{Market, SearchResults},
        },
    },
};

use crate::error::AppError;

pub const GAMMA_ENDPOINT: &str = "https://gamma-api.polymarket.com";
pub const CLOB_V2_ENDPOINT: &str = "https://clob.polymarket.com";

#[derive(Clone, Debug)]
pub struct PolymarketClient {
    gamma: GammaClient,
    clob: ClobClient,
}

impl PolymarketClient {
    pub fn new() -> Result<Self, AppError> {
        let gamma =
            GammaClient::new(GAMMA_ENDPOINT).map_err(|error| AppError::ClientInitialization {
                service: "Gamma API",
                message: error.to_string(),
            })?;
        let clob = ClobClient::new(CLOB_V2_ENDPOINT, ClobConfig::default()).map_err(|error| {
            AppError::ClientInitialization {
                service: "CLOB V2 API",
                message: error.to_string(),
            }
        })?;

        Ok(Self { gamma, clob })
    }

    pub async fn search(&self, query: &str, limit: i32) -> Result<SearchResults, AppError> {
        let request = SearchRequest::builder()
            .q(query)
            .events_status("active".to_owned())
            .keep_closed_markets(0)
            .limit_per_type(limit)
            .page(1)
            .search_profiles(false)
            .search_tags(false)
            .build();

        self.gamma
            .search(&request)
            .await
            .map_err(|error| AppError::Upstream {
                service: "Gamma API",
                message: error.to_string(),
            })
    }

    pub async fn market(&self, market_id: &str) -> Result<Market, AppError> {
        let request = MarketByIdRequest::builder().id(market_id).build();

        self.gamma
            .market_by_id(&request)
            .await
            .map_err(|error| AppError::Upstream {
                service: "Gamma API",
                message: error.to_string(),
            })
    }

    pub async fn order_books(
        &self,
        token_ids: &[polymarket_client_sdk_v2::types::U256],
    ) -> Result<Vec<OrderBookSummaryResponse>, AppError> {
        if token_ids.is_empty() {
            return Ok(Vec::new());
        }

        let requests = token_ids
            .iter()
            .cloned()
            .map(|token_id| {
                OrderBookSummaryRequest::builder()
                    .token_id(token_id)
                    .build()
            })
            .collect::<Vec<_>>();

        self.clob
            .order_books(&requests)
            .await
            .map_err(|error| AppError::Upstream {
                service: "CLOB V2 API",
                message: error.to_string(),
            })
    }
}

impl Default for PolymarketClient {
    fn default() -> Self {
        Self::new().expect("hard-coded Polymarket endpoints must be valid URLs")
    }
}
