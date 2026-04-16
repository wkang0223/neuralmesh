pub mod grpc;
pub mod rest;

use sqlx::PgPool;
use std::sync::Arc;
use crate::on_chain::SettlementOracle;

#[derive(Clone)]
pub struct AppState {
    pub db:     PgPool,
    pub nats:   Option<async_nats::Client>,
    pub redis:  Option<redis::Client>,
    /// Phase 3: on-chain settlement oracle.
    /// Disabled (no-op) when `NM_ESCROW_ADDRESS` env var is absent.
    pub oracle: Arc<SettlementOracle>,
    /// Optional ledger service URL for milestone rewards (NM_LEDGER_URL).
    pub ledger_url: Option<String>,
}
