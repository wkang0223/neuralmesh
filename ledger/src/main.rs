//! neuralmesh-ledger — off-chain credit accounting (Phase 1) + on-chain
//! settlement oracle (Phase 3).
//!
//! Endpoints:
//!   GET  /api/v1/wallet/:id/balance
//!   POST /api/v1/wallet/:id/deposit
//!   POST /api/v1/wallet/:id/withdraw
//!   GET  /api/v1/wallet/:id/transactions
//!   POST /api/v1/escrow/lock
//!   POST /api/v1/escrow/release
//!   POST /api/v1/on_chain/release_escrow   ← Phase 3
//!   POST /api/v1/on_chain/cancel_escrow    ← Phase 3

mod off_chain;
mod on_chain;

use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    Router,
    routing::{get, post},
};
use clap::Parser;
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "neuralmesh-ledger", version)]
struct Cli {
    #[arg(long, env = "NM_DATABASE_URL")]
    database_url: Option<String>,
    #[arg(long, env = "REST_ADDR", default_value = "0.0.0.0:8082")]
    listen_addr: String,
    #[arg(long, default_value = "info")]
    log_level: String,
}

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt().with_env_filter(&cli.log_level).json().init();
    info!(version = env!("CARGO_PKG_VERSION"), "neuralmesh-ledger starting");

    let db_url = cli.database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| "postgresql://neuralmesh:neuralmesh@localhost/neuralmesh".into());

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&db_url)
        .await?;

    // Phase 3: on-chain oracle (no-op when NM_ESCROW_ADDRESS is absent)
    let oracle = Arc::new(on_chain::ArbitrumOracle::from_env());

    let state = off_chain::LedgerState { db, oracle };

    let app = Router::new()
        .route("/health",                            get(health))
        // Off-chain credit/escrow
        .route("/api/v1/wallet/:id/balance",         get(off_chain::handlers::get_balance))
        .route("/api/v1/wallet/:id/deposit",         post(off_chain::handlers::deposit))
        .route("/api/v1/wallet/:id/withdraw",        post(off_chain::handlers::withdraw))
        .route("/api/v1/wallet/:id/transactions",    get(off_chain::handlers::list_transactions))
        .route("/api/v1/escrow/lock",                post(off_chain::handlers::lock_escrow))
        .route("/api/v1/escrow/release",             post(off_chain::handlers::release_escrow))
        // Phase 3: on-chain settlement (called internally by coordinator)
        .route("/api/v1/on_chain/release_escrow",    post(on_chain_release))
        .route("/api/v1/on_chain/cancel_escrow",     post(on_chain_cancel))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&cli.listen_addr).await?;
    info!(addr = %cli.listen_addr, "Ledger REST API listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "neuralmesh-ledger" }))
}

// ── Phase 3: On-chain settlement endpoints ────────────────────────────────────

/// POST /api/v1/on_chain/release_escrow
/// Submits `Escrow.releaseEscrow(jobId, actualCost)` on Arbitrum.
/// Protected by `INTERNAL_SECRET` bearer token.
#[derive(Deserialize)]
struct OnChainReleaseBody {
    job_id:          String,
    actual_cost_nmc: f64,
    secret:          String,
}

async fn on_chain_release(
    State(state): State<off_chain::LedgerState>,
    Json(body):   Json<OnChainReleaseBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !verify_secret(&body.secret) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if body.actual_cost_nmc < 0.0 {
        return Err(StatusCode::BAD_REQUEST);
    }

    match state.oracle.release_escrow(&body.job_id, body.actual_cost_nmc).await {
        Ok(Some(tx_hash)) => {
            info!(job_id = %body.job_id, tx_hash = %tx_hash, "Escrow released on-chain");
            Ok(Json(serde_json::json!({
                "ok":      true,
                "tx_hash": tx_hash,
                "job_id":  body.job_id,
            })))
        }
        Ok(None) => {
            warn!(job_id = %body.job_id, "on_chain_release: oracle disabled");
            Ok(Json(serde_json::json!({
                "ok":      true,
                "tx_hash": null,
                "message": "on-chain oracle disabled — off-chain settlement only",
            })))
        }
        Err(e) => {
            tracing::error!(job_id = %body.job_id, error = %e, "on_chain_release failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// POST /api/v1/on_chain/cancel_escrow
/// Submits `Escrow.cancelEscrow(jobId)` (full consumer refund) on Arbitrum.
#[derive(Deserialize)]
struct OnChainCancelBody {
    job_id: String,
    secret: String,
}

async fn on_chain_cancel(
    State(state): State<off_chain::LedgerState>,
    Json(body):   Json<OnChainCancelBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !verify_secret(&body.secret) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    match state.oracle.cancel_escrow(&body.job_id).await {
        Ok(Some(tx_hash)) => {
            info!(job_id = %body.job_id, tx_hash = %tx_hash, "Escrow cancelled on-chain");
            Ok(Json(serde_json::json!({
                "ok":      true,
                "tx_hash": tx_hash,
            })))
        }
        Ok(None) => Ok(Json(serde_json::json!({ "ok": true, "tx_hash": null }))),
        Err(e) => {
            tracing::error!(error = %e, "on_chain_cancel failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn verify_secret(provided: &str) -> bool {
    let expected = std::env::var("INTERNAL_SECRET").unwrap_or_default();
    !expected.is_empty() && provided == expected
}
