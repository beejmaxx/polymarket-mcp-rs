use polymarket_client_sdk_v2::{
    clob::{
        Client as ClobClient, Config as ClobConfig,
        types::{
            TimeRange,
            request::{OrderBookSummaryRequest, PriceHistoryRequest},
            response::{OrderBookSummaryResponse, PriceHistoryResponse},
        },
    },
    data::{
        Client as DataClient,
        types::{
            SortDirection,
            request::{
                ActivityRequest, HoldersRequest, PositionsRequest, TradesRequest, ValueRequest,
            },
            response::{Activity, MetaHolder, Position, Trade, Value},
        },
    },
    gamma::{
        Client as GammaClient,
        types::{
            request::{
                EventByIdRequest, EventBySlugRequest, EventsRequest, MarketByIdRequest,
                MarketBySlugRequest, MarketsRequest, SearchRequest,
            },
            response::{Event, Market, SearchResults},
        },
    },
    types::{Address, B256, U256},
};

use crate::error::AppError;

pub const GAMMA_ENDPOINT: &str = "https://gamma-api.polymarket.com";
pub const CLOB_V2_ENDPOINT: &str = "https://clob.polymarket.com";
pub const DATA_ENDPOINT: &str = "https://data-api.polymarket.com";

#[derive(Clone, Debug)]
pub struct PolymarketClient {
    gamma: GammaClient,
    clob: ClobClient,
    data: DataClient,
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
        let data =
            DataClient::new(DATA_ENDPOINT).map_err(|error| AppError::ClientInitialization {
                service: "Data API",
                message: error.to_string(),
            })?;

        Ok(Self { gamma, clob, data })
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

    pub async fn market_by_slug(&self, slug: &str) -> Result<Market, AppError> {
        let request = MarketBySlugRequest::builder().slug(slug).build();
        self.gamma
            .market_by_slug(&request)
            .await
            .map_err(gamma_error)
    }

    pub async fn market_by_condition(&self, condition_id: B256) -> Result<Market, AppError> {
        let request = MarketsRequest::builder()
            .condition_ids(vec![condition_id])
            .limit(1)
            .build();
        self.gamma
            .markets(&request)
            .await
            .map_err(gamma_error)?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Upstream {
                service: "Gamma API",
                message: format!("no market found for condition ID {condition_id}"),
            })
    }

    pub async fn events(&self, request: &EventsRequest) -> Result<Vec<Event>, AppError> {
        self.gamma.events(request).await.map_err(gamma_error)
    }

    pub async fn event_by_id(&self, event_id: &str) -> Result<Event, AppError> {
        let request = EventByIdRequest::builder().id(event_id).build();
        self.gamma.event_by_id(&request).await.map_err(gamma_error)
    }

    pub async fn event_by_slug(&self, slug: &str) -> Result<Event, AppError> {
        let request = EventBySlugRequest::builder().slug(slug).build();
        self.gamma
            .event_by_slug(&request)
            .await
            .map_err(gamma_error)
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

    pub async fn order_book(&self, token_id: U256) -> Result<OrderBookSummaryResponse, AppError> {
        let request = OrderBookSummaryRequest::builder()
            .token_id(token_id)
            .build();
        self.clob.order_book(&request).await.map_err(clob_error)
    }

    pub async fn price_history(
        &self,
        token_id: U256,
        time_range: TimeRange,
        fidelity: Option<u32>,
    ) -> Result<PriceHistoryResponse, AppError> {
        let request = PriceHistoryRequest::builder()
            .market(token_id)
            .time_range(time_range)
            .maybe_fidelity(fidelity)
            .build();
        self.clob.price_history(&request).await.map_err(clob_error)
    }

    pub async fn holders(
        &self,
        condition_id: B256,
        limit: i32,
        min_balance: i32,
    ) -> Result<Vec<MetaHolder>, AppError> {
        let request = HoldersRequest::builder()
            .markets(vec![condition_id])
            .limit(limit)
            .map_err(|error| AppError::InvalidInput(error.to_string()))?
            .min_balance(min_balance)
            .map_err(|error| AppError::InvalidInput(error.to_string()))?
            .build();
        self.data
            .holders(&request)
            .await
            .map_err(|error| AppError::Upstream {
                service: "Data API",
                message: error.to_string(),
            })
    }

    pub async fn positions(
        &self,
        wallet: Address,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<Position>, AppError> {
        let request = PositionsRequest::builder()
            .user(wallet)
            .limit(limit)
            .map_err(validation_error)?
            .offset(offset)
            .map_err(validation_error)?
            .sort_direction(SortDirection::Desc)
            .build();
        self.data.positions(&request).await.map_err(data_error)
    }

    pub async fn wallet_value(&self, wallet: Address) -> Result<Vec<Value>, AppError> {
        let request = ValueRequest::builder().user(wallet).build();
        self.data.value(&request).await.map_err(data_error)
    }

    pub async fn wallet_trades(
        &self,
        wallet: Address,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<Trade>, AppError> {
        let request = TradesRequest::builder()
            .user(wallet)
            .limit(limit)
            .map_err(validation_error)?
            .offset(offset)
            .map_err(validation_error)?
            .build();
        self.data.trades(&request).await.map_err(data_error)
    }

    pub async fn wallet_activity(
        &self,
        wallet: Address,
        limit: i32,
        offset: i32,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<Vec<Activity>, AppError> {
        let request = ActivityRequest::builder()
            .user(wallet)
            .limit(limit)
            .map_err(validation_error)?
            .offset(offset)
            .map_err(validation_error)?
            .maybe_start(start)
            .maybe_end(end)
            .sort_direction(SortDirection::Desc)
            .build();
        self.data.activity(&request).await.map_err(data_error)
    }
}

fn validation_error(error: impl ToString) -> AppError {
    AppError::InvalidInput(error.to_string())
}

fn data_error(error: polymarket_client_sdk_v2::error::Error) -> AppError {
    AppError::Upstream {
        service: "Data API",
        message: error.to_string(),
    }
}

fn gamma_error(error: polymarket_client_sdk_v2::error::Error) -> AppError {
    AppError::Upstream {
        service: "Gamma API",
        message: error.to_string(),
    }
}

fn clob_error(error: polymarket_client_sdk_v2::error::Error) -> AppError {
    AppError::Upstream {
        service: "CLOB V2 API",
        message: error.to_string(),
    }
}

impl Default for PolymarketClient {
    fn default() -> Self {
        Self::new().expect("hard-coded Polymarket endpoints must be valid URLs")
    }
}
