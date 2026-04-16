//! `hatch` — Hatch CLI
//!
//! Usage:
//!   hatch provider install|start|stop|status|config
//!   hatch job submit|list|logs|cancel
//!   hatch gpu list|benchmark
//!   hatch wallet balance|deposit|withdraw|history

mod commands;
mod client;
mod config;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;

const BANNER: &str = r#"
  _   _       _       _
 | | | | __ _| |_ ___| |__
 | |_| |/ _` | __/ __| '_ \
 |  _  | (_| | || (__| | | |
 |_| |_|\__,_|\__\___|_| |_|

 Apple Silicon GPU Marketplace
"#;

#[derive(Parser)]
#[command(
    name = "hatch",
    about = "Hatch CLI — lease or use idle Apple Silicon GPUs",
    version,
    propagate_version = true,
)]
struct Cli {
    /// Coordinator endpoint override
    #[arg(long, global = true, env = "NM_COORDINATOR")]
    coordinator: Option<String>,

    /// Ledger endpoint override
    #[arg(long, global = true, env = "NM_LEDGER")]
    ledger: Option<String>,

    /// Output format: text (default) or json
    #[arg(long, global = true, default_value = "text")]
    output: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage this machine as a GPU provider
    Provider {
        #[command(subcommand)]
        action: commands::provider::ProviderCmd,
    },
    /// Submit and manage compute jobs
    Job {
        #[command(subcommand)]
        action: commands::job::JobCmd,
    },
    /// Browse available GPUs in the network
    Gpu {
        #[command(subcommand)]
        action: commands::gpu::GpuCmd,
    },
    /// Manage your NMC credit wallet
    Wallet {
        #[command(subcommand)]
        action: commands::wallet::WalletCmd,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Init minimal tracing (errors only by default in CLI)
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("NM_LOG")
                .unwrap_or_else(|_| "error".into())
        )
        .without_time()
        .init();

    let cli = Cli::parse();

    // Build API client config
    let cfg = config::CliConfig::load(
        cli.coordinator.as_deref(),
        cli.ledger.as_deref(),
    )?;

    let ctx = client::ClientContext {
        coordinator_url: cfg.coordinator_url.clone(),
        ledger_url: cfg.ledger_url.clone(),
        output_json: cli.output == "json",
        account_id: cfg.account_id.clone(),
    };

    match cli.command {
        Commands::Provider { action } => {
            commands::provider::run(action, &ctx).await?
        }
        Commands::Job { action } => {
            commands::job::run(action, &ctx).await?
        }
        Commands::Gpu { action } => {
            commands::gpu::run(action, &ctx).await?
        }
        Commands::Wallet { action } => {
            commands::wallet::run(action, &ctx).await?
        }
    }

    Ok(())
}
