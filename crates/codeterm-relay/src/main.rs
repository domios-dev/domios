//! CodeTerm Relay - Lightweight mesh node for webhooks and remote UI.
//!
//! A tiny Rust binary (~5-10 MB) with embedded React SPA. Pairs with mesh peers
//! via Ed25519, has restricted capabilities (pane.read, pane.send_text, notify).
//! Lives on a Tailscale-routed VM/NAS/Pi, accepts inbound webhooks, exposes
//! a mobile-friendly web UI.

mod config;
mod routes;
mod webhook;
mod api;
mod error;

use std::sync::Arc;
use parking_lot::Mutex;
use anyhow::Result;
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;
use clap::Parser;
use axum::Router;

use crate::config::Config;
use crate::routes::RouteTable;
use crate::api::AppState;

#[derive(Parser, Debug)]
#[command(name = "codeterm-relay")]
#[command(about = "Lightweight mesh relay for CodeTerm webhooks and remote UI")]
struct Args {
    /// HTTP port for the relay API and web UI (default: 7690)
    #[arg(long, env = "CODETERM_RELAY_PORT", default_value = "7690")]
    port: u16,

    /// Config file path (default: ~/.codeterm-relay/config.toml)
    #[arg(long, env = "CODETERM_RELAY_CONFIG")]
    config: Option<String>,

    /// Routes file path (default: ~/.codeterm-relay/routes.toml)
    #[arg(long, env = "CODETERM_RELAY_ROUTES")]
    routes: Option<String>,

    /// Identity file path (default: ~/.codeterm-relay/identity.json)
    #[arg(long, env = "CODETERM_RELAY_IDENTITY")]
    identity: Option<String>,

    /// Bearer token for API authentication
    #[arg(long, env = "CODETERM_RELAY_BEARER_TOKEN")]
    bearer_token: Option<String>,

    /// Enable verbose logging
    #[arg(short, long, env = "CODETERM_RELAY_VERBOSE")]
    verbose: bool,

    /// Expose via Tailscale Funnel (requires Tailscale installed)
    #[arg(long, env = "CODETERM_RELAY_TAILSCALE_FUNNEL")]
    tailscale_funnel: bool,
}

impl Args {
    fn config_path(&self) -> std::path::PathBuf {
        self.config.clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("codeterm-relay")
                    .join("config.toml")
            })
    }

    fn routes_path(&self) -> std::path::PathBuf {
        self.routes.clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("codeterm-relay")
                    .join("routes.toml")
            })
    }

    fn identity_path(&self) -> std::path::PathBuf {
        self.identity.clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("codeterm-relay")
                    .join("identity.json")
            })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.verbose { Level::DEBUG } else { Level::INFO };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("CodeTerm Relay starting...");
    info!("Args: {:?}", args);

    // Ensure config directory exists
    let config_dir = args.config_path().parent().unwrap();
    std::fs::create_dir_all(config_dir)?;

    // Load or create config
    let config = Config::load_or_create(&args.config_path(), &args)?;
    info!("Loaded config: {} (port {})", config.relay.name, config.relay.port);

    // Load route table
    let route_table = RouteTable::load_or_create(&args.routes_path())?;
    info!("Loaded {} routes", route_table.routes.len());

    // Load or create identity
    let identity = codeterm_mesh_client::types::MeshIdentity::load_or_create(&args.identity_path())?;
    info!("Identity: {} ({})", identity.instance_id, identity.instance_name());

    // Create app state
    let state = AppState::new(config, route_table, identity);

    // Build the router
    let app = api::create_router(state.clone());

    // Start the server
    let addr = format!("0.0.0.0:{}", args.port);
    info!("Starting HTTP server on {}", addr);

    // TODO: Implement Tailscale Funnel if requested
    if args.tailscale_funnel {
        info!("Tailscale Funnel not yet implemented");
    }

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}