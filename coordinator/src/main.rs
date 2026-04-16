//! neuralmesh-coordinator — distributed job matchmaking and network coordination.

mod api;
mod db;
mod matching;
mod on_chain;
mod p2p;
mod queue;
mod reputation;

use anyhow::Result;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "neuralmesh-coordinator", version)]
struct Cli {
    #[arg(long, env = "NM_DATABASE_URL")]
    database_url: Option<String>,
    /// Optional — if unset, Redis features are disabled
    #[arg(long, env = "NM_REDIS_URL")]
    redis_url: Option<String>,
    /// Optional — if unset, in-memory job queue is used
    #[arg(long, env = "NM_NATS_URL")]
    nats_url: Option<String>,
    #[arg(long, env = "GRPC_ADDR", default_value = "0.0.0.0:9090")]
    grpc_addr: String,
    #[arg(long, env = "REST_ADDR", default_value = "0.0.0.0:8080")]
    rest_addr: String,
    #[arg(long, default_value = "info")]
    log_level: String,
    /// Ledger service URL for on-chain settlement forwarding (Phase 3, optional)
    #[arg(long, env = "NM_LEDGER_URL")]
    ledger_url: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(&cli.log_level)
        .json()
        .init();

    info!(version = env!("CARGO_PKG_VERSION"), "neuralmesh-coordinator starting");

    let db_url = cli.database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| "postgresql://neuralmesh:neuralmesh@localhost/neuralmesh".into());

    // Database connection pool
    let db = PgPoolOptions::new()
        .max_connections(20)
        .connect(&db_url)
        .await?;
    sqlx::migrate!("src/db/migrations").run(&db).await?;
    info!("Database connected and migrations applied");

    // NATS — optional, falls back to DB-only polling
    let nats = if let Some(ref url) = cli.nats_url {
        match async_nats::connect(url).await {
            Ok(c) => { info!(url = %url, "NATS connected"); Some(c) }
            Err(e) => { warn!(url = %url, error = %e, "NATS unavailable — using DB polling fallback"); None }
        }
    } else {
        info!("NATS not configured — using DB polling fallback");
        None
    };

    // Redis — optional
    let redis = if let Some(ref url) = cli.redis_url {
        match redis::Client::open(url.as_str()) {
            Ok(c) => { info!(url = %url, "Redis client ready"); Some(c) }
            Err(e) => { warn!(url = %url, error = %e, "Redis unavailable — caching disabled"); None }
        }
    } else {
        info!("Redis not configured — caching disabled");
        None
    };

    // Phase 3: on-chain settlement oracle (no-op when NM_ESCROW_ADDRESS unset)
    let oracle = Arc::new(on_chain::SettlementOracle::from_env());

    // Shared app state
    let state = api::AppState {
        db:     db.clone(),
        nats:   nats.clone(),
        redis,
        oracle,
        ledger_url: cli.ledger_url.clone(),
    };

    // Build a single combined axum router: gRPC routes merged with REST routes.
    // tonic 0.12 exposes `Server::into_router()` which returns an axum::Router,
    // so we can simply merge the two routers and serve both on one port.
    // This is required for Railway deployments that expose only a single port.
    let grpc_router = api::grpc::build_router(state.clone());
    let rest_router = api::rest::build_router(state.clone());
    // gRPC routes are matched by path prefix (/neuralmesh.*); REST uses /api/v1/*.
    // Merging puts both in the same axum Router — axum picks by path first,
    // and tonic's routing is path-based so there are no conflicts.
    let app = grpc_router.merge(rest_router);

    let listener = tokio::net::TcpListener::bind(&cli.rest_addr).await?;
    info!(addr = %cli.rest_addr, "Serving REST + gRPC on single port");

    tokio::try_join!(
        async { axum::serve(listener, app).await.map_err(anyhow::Error::from) },
        matching::run_auction_loop(db.clone(), nats),
        matching::run_heartbeat_watcher(db.clone()),
    )?;

    Ok(())
}
