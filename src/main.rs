use std::{
    error::Error,
    io::{self, Read as _, Write as _},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

use clap::{Parser, Subcommand, ValueEnum};
use polymarket_mcp_rs::{
    App, PolymarketServer, ToolProfile,
    http::{HttpConfig, serve_http},
    run_doctor,
};
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "polymarket-mcp-rs",
    version,
    about = "Polymarket market intelligence and optional trading over MCP"
)]
struct Cli {
    /// Tool surface to expose: chatgpt, core, research, trading, or all.
    #[arg(long, global = true, value_name = "PROFILE")]
    tool_profile: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Serve MCP over stdio or Streamable HTTP (stdio is the default).
    Serve {
        /// MCP transport. HTTP only accepts the credential-blind chatgpt or core profile.
        #[arg(long, value_enum, default_value_t = Transport::Stdio)]
        transport: Transport,
        /// HTTP listen address; ignored for stdio.
        #[arg(long, default_value = "127.0.0.1:8080")]
        bind: String,
    },
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
    /// Probe the local HTTP health endpoint (used by the container image).
    #[command(hide = true)]
    Healthcheck,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum Transport {
    Stdio,
    Http,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Serve {
        transport: Transport::Stdio,
        bind: "127.0.0.1:8080".to_owned(),
    });
    let default_profile = match &command {
        Command::Serve {
            transport: Transport::Http,
            ..
        } => ToolProfile::Chatgpt,
        _ => ToolProfile::default(),
    };
    let profile = resolve_profile(cli.tool_profile.as_deref(), default_profile)?;

    match command {
        Command::Serve {
            transport: Transport::Stdio,
            ..
        } => serve_stdio(profile).await,
        Command::Serve {
            transport: Transport::Http,
            bind,
        } => {
            init_logging();
            let bind = std::env::var("POLYMARKET_HTTP_BIND").unwrap_or(bind);
            serve_http(profile, HttpConfig::from_environment(&bind)?).await?;
            Ok(())
        }
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
        Command::Healthcheck => container_healthcheck(),
    }
}

fn container_healthcheck() -> Result<(), Box<dyn Error>> {
    let address = std::env::var("POLYMARKET_HEALTHCHECK_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
        .parse::<SocketAddr>()?;
    let timeout = Duration::from_secs(3);
    let mut stream = TcpStream::connect_timeout(&address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut response = [0_u8; 128];
    let length = stream.read(&mut response)?;
    let status = &response[..length];
    if status.starts_with(b"HTTP/1.1 200") || status.starts_with(b"HTTP/1.0 200") {
        Ok(())
    } else {
        Err(io::Error::other("HTTP health endpoint did not return 200").into())
    }
}

fn resolve_profile(
    cli_value: Option<&str>,
    default_profile: ToolProfile,
) -> Result<ToolProfile, io::Error> {
    let value = cli_value
        .map(str::to_owned)
        .or_else(|| std::env::var("POLYMARKET_TOOL_PROFILE").ok())
        .unwrap_or_else(|| default_profile.to_string());
    value
        .parse()
        .map_err(|message: String| io::Error::new(io::ErrorKind::InvalidInput, message))
}

async fn list_tools(profile: ToolProfile, json: bool) -> Result<(), Box<dyn Error>> {
    let app = App::new_ephemeral()?;
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

async fn serve_stdio(profile: ToolProfile) -> Result<(), Box<dyn Error>> {
    init_logging();

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

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init()
        .ok();
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
