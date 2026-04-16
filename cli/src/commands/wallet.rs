//! `nm wallet` subcommands.

use crate::client::ClientContext;
use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use serde::{Deserialize, Serialize};

#[derive(Subcommand)]
pub enum WalletCmd {
    /// Show NMC credit balance
    Balance,
    /// Deposit NMC credits (generates payment link)
    Deposit {
        /// Amount of NMC credits to deposit
        amount: f64,
        /// Payment method: stripe (default), crypto
        #[arg(long, default_value = "stripe")]
        method: String,
    },
    /// Withdraw NMC credits to crypto address
    Withdraw {
        /// Destination address (ETH/USDC on Arbitrum)
        address: String,
        /// Amount of NMC to withdraw
        amount: f64,
    },
    /// Show transaction history
    History {
        /// Show last N transactions
        #[arg(long, default_value = "20")]
        limit: u32,
        /// Filter by type: deposit, withdrawal, job_payment, job_earning, escrow_lock, escrow_release
        #[arg(long)]
        kind: Option<String>,
    },
}

// ─── API response types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BalanceResponse {
    account_id: String,
    available_nmc: f64,
    escrowed_nmc: f64,
    total_earned_nmc: f64,
    total_spent_nmc: f64,
}

#[derive(Debug, Deserialize)]
struct DepositResponse {
    payment_url: Option<String>,
    deposit_address: Option<String>,
    amount_nmc: f64,
    #[serde(default)]
    reference_id: String,
}

#[derive(Debug, Deserialize)]
struct WithdrawResponse {
    tx_id: String,
    amount_nmc: f64,
    destination: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct Transaction {
    tx_id: String,
    #[serde(rename = "tx_type")]
    kind: String,
    amount_nmc: f64,
    #[serde(default)]
    balance_after: f64,
    description: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct TransactionListResponse {
    transactions: Vec<Transaction>,
    #[serde(default)]
    total: u32,
}

#[derive(Debug, Serialize)]
struct WithdrawRequest {
    dest_address: String,
    amount_nmc: f64,
}

// ─── Command handlers ────────────────────────────────────────────────────────

pub async fn run(cmd: WalletCmd, ctx: &ClientContext) -> Result<()> {
    match cmd {
        WalletCmd::Balance => show_balance(ctx).await,
        WalletCmd::Deposit { amount, method } => deposit(ctx, amount, &method).await,
        WalletCmd::Withdraw { address, amount } => withdraw(ctx, &address, amount).await,
        WalletCmd::History { limit, kind } => show_history(ctx, limit, kind).await,
    }
}

async fn show_balance(ctx: &ClientContext) -> Result<()> {
    let account_id = ctx.require_account_id()?;

    let resp = ctx
        .http()
        .get(ctx.ledger_url(&format!("/api/v1/wallet/{}/balance", account_id)))
        .send()
        .await
        .context("Failed to fetch balance")?;

    if resp.status().as_u16() == 404 {
        println!("Account not found. Run {} to set up.", "`nm provider install`".cyan());
        return Ok(());
    }
    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to fetch balance: {}", err);
    }

    let bal: BalanceResponse = resp.json().await.context("Invalid balance response")?;

    if ctx.output_json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "account_id": bal.account_id,
            "available_nmc": bal.available_nmc,
            "escrowed_nmc": bal.escrowed_nmc,
            "total_earned_nmc": bal.total_earned_nmc,
            "total_spent_nmc": bal.total_spent_nmc,
        }))?);
        return Ok(());
    }

    println!("{}", "NMC Wallet".bold().cyan());
    println!("─────────────────────────────────────────");
    println!("  Account:      {}", bal.account_id.yellow());
    println!("  Available:    {} NMC", format!("{:.4}", bal.available_nmc).green().bold());
    if bal.escrowed_nmc > 0.0 {
        println!("  In escrow:    {} NMC  (reserved for active jobs)", format!("{:.4}", bal.escrowed_nmc).yellow());
    }
    println!("─────────────────────────────────────────");
    println!("  Total earned: {} NMC", format!("{:.4}", bal.total_earned_nmc).blue());
    println!("  Total spent:  {} NMC", format!("{:.4}", bal.total_spent_nmc).normal());
    println!();
    println!("  Add credits:    {}", "nm wallet deposit 50".cyan());
    println!("  Cash out:       {}", "nm wallet withdraw <eth-address> <amount>".cyan());

    Ok(())
}

async fn deposit(ctx: &ClientContext, amount: f64, method: &str) -> Result<()> {
    let account_id = ctx.require_account_id()?;

    if amount <= 0.0 {
        anyhow::bail!("Deposit amount must be greater than 0");
    }

    let resp = ctx
        .http()
        .post(ctx.ledger_url(&format!("/api/v1/wallet/{}/deposit", account_id)))
        .json(&serde_json::json!({
            "amount_nmc": amount,
            "reference":  method,
        }))
        .send()
        .await
        .context("Failed to initiate deposit")?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("Deposit failed: {}", err);
    }

    let dep: DepositResponse = resp.json().await.context("Invalid deposit response")?;

    println!("{}", "Deposit NMC Credits".bold().cyan());
    println!("─────────────────────────────────────────");
    println!("  Amount:     {} NMC", format!("{:.2}", dep.amount_nmc).green().bold());

    match method {
        "stripe" => {
            if let Some(url) = dep.payment_url {
                println!("  Method:     Credit / Debit card");
                println!();
                println!("  Open this URL to complete payment:");
                println!("  {}", url.cyan().underline());
                println!();
                println!("  Credits will appear in your wallet within ~30 seconds of payment.");
            } else {
                println!("  Credits added to your account.");
            }
        }
        "crypto" => {
            if let Some(addr) = dep.deposit_address {
                println!("  Method:     Crypto (USDC on Arbitrum)");
                println!();
                println!("  Send USDC to this Arbitrum address:");
                println!("  {}", addr.yellow().bold());
                println!();
                println!("  Reference:  {}", dep.reference_id.cyan());
                println!("  Credits will appear after 1 block confirmation (~2s on Arbitrum).");
            }
        }
        other => {
            println!("  Method: {}", other);
            println!("  Reference: {}", dep.reference_id.cyan());
        }
    }

    Ok(())
}

async fn withdraw(ctx: &ClientContext, address: &str, amount: f64) -> Result<()> {
    let account_id = ctx.require_account_id()?.to_string();

    if amount <= 0.0 {
        anyhow::bail!("Withdrawal amount must be greater than 0");
    }

    // Basic Ethereum address validation
    if !address.starts_with("0x") || address.len() != 42 {
        anyhow::bail!("Invalid Ethereum address. Expected 0x... (42 chars)");
    }

    println!("Withdrawing {} NMC to {}...", format!("{:.4}", amount).green(), address.yellow());

    let req = WithdrawRequest {
        dest_address: address.to_string(),
        amount_nmc: amount,
    };

    let resp = ctx
        .http()
        .post(ctx.ledger_url(&format!("/api/v1/wallet/{}/withdraw", account_id)))
        .json(&req)
        .send()
        .await
        .context("Failed to initiate withdrawal")?;

    if resp.status().as_u16() == 400 {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("Withdrawal failed: {}", err);
    }
    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("Withdrawal failed: {}", err);
    }

    let w: WithdrawResponse = resp.json().await.context("Invalid withdrawal response")?;

    println!("{} Withdrawal submitted", "✓".green());
    println!("─────────────────────────────────────────");
    println!("  TX ID:    {}", w.tx_id.cyan());
    println!("  Amount:   {} NMC", format!("{:.4}", w.amount_nmc).green());
    println!("  To:       {}", w.destination.yellow());
    println!("  Status:   {}", w.status.yellow());
    println!();
    println!("  Funds arrive as USDC on Arbitrum within ~5 minutes.");

    Ok(())
}

async fn show_history(ctx: &ClientContext, limit: u32, kind: Option<String>) -> Result<()> {
    let account_id = ctx.require_account_id()?;

    let mut params = vec![
        format!("account_id={}", account_id),
        format!("limit={}", limit),
    ];
    if let Some(k) = &kind {
        params.push(format!("kind={}", k));
    }

    let url = ctx.ledger_url(&format!("/api/v1/wallet/{}/transactions?{}", account_id, params.join("&")));

    let resp = ctx
        .http()
        .get(&url)
        .send()
        .await
        .context("Failed to fetch transaction history")?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to fetch history: {}", err);
    }

    let result: TransactionListResponse = resp.json().await.context("Invalid transaction response")?;

    if result.transactions.is_empty() {
        println!("No transactions found.");
        return Ok(());
    }

    if ctx.output_json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "transactions": result.transactions.iter().map(|t| serde_json::json!({
                "tx_id": t.tx_id,
                "kind": t.kind,
                "amount_nmc": t.amount_nmc,
                "balance_after": t.balance_after,
                "description": t.description,
                "created_at": t.created_at,
            })).collect::<Vec<_>>(),
            "total": result.total,
        }))?);
        return Ok(());
    }

    println!("{}", "Transaction History".bold().cyan());
    println!("─────────────────────────────────────────────────────────────────────────────────────");
    println!("{:<22} {:<20} {:>10} {:>12} {}",
        "DATE", "TYPE", "AMOUNT", "BALANCE", "DESCRIPTION");
    println!("─────────────────────────────────────────────────────────────────────────────────────");

    for tx in &result.transactions {
        let amount_str = if tx.amount_nmc >= 0.0 {
            format!("+{:.4}", tx.amount_nmc).green().to_string()
        } else {
            format!("{:.4}", tx.amount_nmc).red().to_string()
        };

        let kind_colored = match tx.kind.as_str() {
            "deposit"        => tx.kind.green().to_string(),
            "withdrawal"     => tx.kind.yellow().to_string(),
            "job_payment"    => tx.kind.red().to_string(),
            "job_earning"    => tx.kind.cyan().to_string(),
            "escrow_lock"    => tx.kind.yellow().to_string(),
            "escrow_release" => tx.kind.blue().to_string(),
            _                => tx.kind.normal().to_string(),
        };

        println!("{:<22} {:<29} {:>10} {:>12} {}",
            &tx.created_at[..19],
            kind_colored,
            amount_str,
            format!("{:.4}", tx.balance_after).bold(),
            tx.description.chars().take(35).collect::<String>(),
        );
    }

    println!("─────────────────────────────────────────────────────────────────────────────────────");
    if result.total > limit {
        println!("  Showing {} of {} transactions. Use --limit to see more.", limit, result.total);
    }

    Ok(())
}
