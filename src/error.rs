use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("could not initialize the {service} client: {message}")]
    ClientInitialization {
        service: &'static str,
        message: String,
    },

    #[error("{service} request failed: {message}")]
    Upstream {
        service: &'static str,
        message: String,
    },
}

impl AppError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::ClientInitialization { .. } => "client_initialization_failed",
            Self::Upstream { service, .. } => match *service {
                "Gamma API" => "gamma_api_error",
                "CLOB V2 API" => "clob_v2_api_error",
                "Data API" => "data_api_error",
                "CLOB WebSocket" | "CLOB user WebSocket" => "websocket_error",
                "SQLite recorder" => "recording_database_error",
                "SQLite trading audit" => "trading_audit_error",
                "CLOB V2 trading API" => "trading_api_error",
                _ => "upstream_api_error",
            },
        }
    }

    #[must_use]
    pub const fn retryable(&self) -> bool {
        matches!(self, Self::Upstream { .. })
    }
}
