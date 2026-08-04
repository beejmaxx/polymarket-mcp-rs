use std::{error::Error, io};

use clap::{Parser, Subcommand};
use polymarket_mcp_rs::{App, PolymarketServer, ToolProfile, run_doctor};
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "polymarket-mcp-rs",
    version,
    about = "Polymarket market intelligence and optional trading over MCP"
)]
struct Cli {
    /// Tool surface to expose: core, research, trading, or all.
    #[arg(long, global = true, value_name = "PROFILE")]
    tool_profile: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Serve MCP over stdin/stdout (the default when no command is given).
    Serve,
    /// Validate local configuration and production Polymarket connectivity.
    Doctor {
        /// Skip production HTTP and WebSocket checks.
        #[arg(long)]
        offline: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print the tools exposed by the selected profile.
    Tools {
        /// Emit full tool definitions as JSON instead of one name per line.
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let profile = resolve_profile(cli.tool_profile.as_deref())?;

    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => serve(profile).await,
        Command::Doctor { offline, json } => {
            let report = run_doctor(profile, !offline).await;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.human());
            }
            if report.ok {
                Ok(())
            } else {
                Err(io::Error::other("diagnostic checks failed").into())
            }
        }
        Command::Tools { json } => list_tools(profile, json).await,
    }
}

fn resolve_profile(cli_value: Option<&str>) -> Result<ToolProfile, io::Error> {
    let value = cli_value
        .map(str::to_owned)
        .or_else(|| std::env::var("POLYMARKET_TOOL_PROFILE").ok())
        .unwrap_or_else(|| ToolProfile::default().to_string());
    value
        .parse()
        .map_err(|message: String| io::Error::new(io::ErrorKind::InvalidInput, message))
}

async fn list_tools(profile: ToolProfile, json: bool) -> Result<(), Box<dyn Error>> {
    let app = App::new()?;
    let server = PolymarketServer::with_profile(app.clone(), profile);
    let tools = server.tools();
    if json {
        println!("{}", serde_json::to_string_pretty(&tools)?);
    } else {
        for tool in tools {
            println!("{}", tool.name);
        }
    }
    app.shutdown().await?;
    Ok(())
}

async fn serve(profile: ToolProfile) -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let app = App::new()?;
    let server = PolymarketServer::with_profile(app.clone(), profile);
    tracing::info!(%profile, "starting Polymarket MCP server over stdio");

    let mut signal_task = tokio::spawn(shutdown_signal());
    let service = tokio::select! {
        biased;
        result = &mut signal_task => {
            result?;
            tracing::info!("received process shutdown signal during MCP initialization");
            let (recordings, watches) = app.shutdown().await?;
            tracing::info!(recordings, watches, "Polymarket MCP server stopped cleanly");
            return Ok(());
        },
        result = server.serve(stdio()) => match result {
            Ok(service) => service,
            Err(error) => {
                let _ = app.shutdown().await;
                return Err(error.into());
            }
        },
    };
    let service_error = tokio::select! {
        biased;
        result = &mut signal_task => {
            result?;
            tracing::info!("received process shutdown signal");
            None
        },
        result = service.waiting() => {
            signal_task.abort();
            result.err()
        },
    };
    let shutdown_result = app.shutdown().await;
    if let Some(error) = service_error {
        return Err(error.into());
    }
    let (recordings, watches) = shutdown_result?;
    tracing::info!(recordings, watches, "Polymarket MCP server stopped cleanly");

    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
