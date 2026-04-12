//! neuralmesh-coordinator — distributed job matchmaking and network coordination.

mod api;
mod db;
mod matching;
mod p2p;
mod queue;
mod reputation;

use anyhow::Result;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
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
    #[arg(long, default_value = "0.0.0.0:9090")]
    grpc_addr: String,
    #[arg(long, default_value = "0.0.0.0:8080")]
    rest_addr: String,
    #[arg(long, default_value = "info")]
    log_level: String,
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

    // Shared app state
    let state = api::AppState {
        db: db.clone(),
        nats: nats.clone(),
        redis,
    };

    // Start background services in parallel
    tokio::try_join!(
        api::grpc::serve(state.clone(), cli.grpc_addr.clone()),
        api::rest::serve(state.clone(), cli.rest_addr.clone()),
        matching::run_auction_loop(db.clone(), nats),
        matching::run_heartbeat_watcher(db.clone()),
    )?;

    Ok(())
}
