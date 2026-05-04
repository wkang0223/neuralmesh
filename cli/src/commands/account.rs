//! `hatch account` subcommands — account registration and identity management.

use crate::client::ClientContext;
use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

#[derive(Subcommand)]
pub enum AccountCmd {
    /// Register this machine and save account_id to config (idempotent)
    Setup,
    /// Show the current account ID and key path
    Info,
}

pub async fn run(cmd: AccountCmd, ctx: &ClientContext) -> Result<()> {
    match cmd {
        AccountCmd::Setup => setup(ctx).await,
        AccountCmd::Info  => info(ctx).await,
    }
}

async fn setup(ctx: &ClientContext) -> Result<()> {
    println!("{}", "Hatch Account Setup".bold().cyan());
    println!("Generating identity and registering with the network...\n");

    let account_id = crate::commands::provider::ensure_account(ctx).await?;

    println!("{} Account registered", "✓".green());
    println!("  Account ID: {}", account_id.yellow());
    println!("  Saved to:   ~/.config/neuralmesh/cli.toml");
    println!();
    println!("  Check balance:  {}", "hatch wallet balance".cyan());
    println!("  Add credits:    {}", "hatch wallet deposit 50".cyan());
    println!("  List GPUs:      {}", "hatch gpu list".cyan());

    Ok(())
}

async fn info(ctx: &ClientContext) -> Result<()> {
    let cfg_dir = dirs::config_dir()
        .unwrap_or_default()
        .join("hatch");

    let key_path = cfg_dir.join("identity.key");
    let cli_cfg_path = cfg_dir.join("cli.toml");

    println!("{}", "Account Info".bold().cyan());
    println!("─────────────────────────────────────────");

    if let Some(id) = &ctx.account_id {
        println!("  Account ID:  {}", id.yellow());
    } else {
        println!("  Account ID:  {} (run `hatch account setup`)", "not configured".red());
    }

    println!(
        "  Identity key: {}",
        if key_path.exists() {
            key_path.display().to_string().green().to_string()
        } else {
            "not found".red().to_string()
        }
    );

    println!(
        "  Config file:  {}",
        if cli_cfg_path.exists() {
            cli_cfg_path.display().to_string().normal().to_string()
        } else {
            "not found".red().to_string()
        }
    );

    println!("  Coordinator:  {}", ctx.coordinator_url.cyan());
    println!("  Ledger:       {}", ctx.ledger_url.cyan());

    Ok(())
}
