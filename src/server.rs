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
        GetMarketInput, MarketDetail, SearchMarketsInput, SearchMarketsOutput, ServerStatus,
        ToolError,
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
            .get_market(input.market_id)
            .await
            .map(Json)
            .map_err(tool_error)
    }
}

#[tool_handler]
impl ServerHandler for PolymarketServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Read-only access to current Polymarket discovery and CLOB V2 market data. Use search_markets first, then pass a returned market_id to get_market.",
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
    fn exposes_only_the_phase_zero_tools() {
        let mut names = PolymarketServer::tool_router()
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<Vec<_>>();
        names.sort();

        assert_eq!(names, ["get_market", "search_markets", "server_status"]);
    }

    #[test]
    fn tool_errors_are_structured() {
        let Json(error) = tool_error(AppError::InvalidInput("bad query".to_owned()));
        assert_eq!(error.code, "invalid_input");
        assert!(!error.retryable);
    }
}
