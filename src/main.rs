use std::error::Error;

use polymarket_mcp_rs::{App, PolymarketServer};
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let server = PolymarketServer::new(App::new()?);
    tracing::info!("starting Polymarket MCP server over stdio");

    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
