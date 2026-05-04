//! REST handlers for off-chain credit accounting.

use super::LedgerState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use tracing::info;

// ──────────────────────────────────────────────
// Balance
// ──────────────────────────────────────────────

pub async fn get_balance(
    State(state): State<LedgerState>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Upsert account if not exists
    sqlx::query!(
        "INSERT INTO credit_accounts (account_id) VALUES ($1) ON CONFLICT DO NOTHING",
        account_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let row = sqlx::query!(
        "SELECT available_nmc, escrowed_nmc, total_earned_nmc, total_spent_nmc FROM credit_accounts WHERE account_id = $1",
        account_id,
    )
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(serde_json::json!({
        "account_id":       account_id,
        "available_nmc":    row.available_nmc,
        "escrowed_nmc":     row.escrowed_nmc,
        "total_nmc":        row.available_nmc.unwrap_or(0.0) + row.escrowed_nmc.unwrap_or(0.0),
        "total_earned_nmc": row.total_earned_nmc,
        "total_spent_nmc":  row.total_spent_nmc,
    })))
}

// ──────────────────────────────────────────────
// Deposit
// ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DepositBody {
    pub amount_nmc: f64,
    pub reference: Option<String>,
}

pub async fn deposit(
    State(state): State<LedgerState>,
    Path(account_id): Path<String>,
    Json(body): Json<DepositBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if body.amount_nmc <= 0.0 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let tx_id = Uuid::new_v4().to_string();

    // Update balance
    sqlx::query!(
        r#"
        INSERT INTO credit_accounts (account_id, available_nmc)
        VALUES ($1, $2)
        ON CONFLICT (account_id) DO UPDATE
        SET available_nmc = credit_accounts.available_nmc + $2,
            updated_at    = now()
        "#,
        account_id, body.amount_nmc,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Record transaction (with balance_after snapshot)
    sqlx::query!(
        r#"INSERT INTO transactions (tx_id, account_id, tx_type, amount_nmc, balance_after, reference, description)
           VALUES ($1, $2, 'deposit', $3,
               (SELECT available_nmc FROM credit_accounts WHERE account_id = $2),
               $4, 'Manual deposit')"#,
        tx_id, account_id, body.amount_nmc, body.reference,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(account_id, amount = body.amount_nmc, tx_id = %tx_id, "Deposit recorded");

    Ok(Json(serde_json::json!({
        "ok": true,
        "transaction_id": tx_id,
        "amount_nmc": body.amount_nmc,
    })))
}

// ──────────────────────────────────────────────
// Withdraw
// ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct WithdrawBody {
    pub amount_nmc: f64,
    pub dest_address: String,
}

pub async fn withdraw(
    State(state): State<LedgerState>,
    Path(account_id): Path<String>,
    Json(body): Json<WithdrawBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if body.amount_nmc <= 0.0 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let tx_id = Uuid::new_v4().to_string();

    // Atomic debit (fails if insufficient balance)
    let result = sqlx::query!(
        r#"
        UPDATE credit_accounts
        SET available_nmc = available_nmc - $1,
            total_spent_nmc = total_spent_nmc + $1,
            updated_at = now()
        WHERE account_id = $2
          AND available_nmc >= $1
        "#,
        body.amount_nmc, account_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if result.rows_affected() == 0 {
        return Ok(Json(serde_json::json!({
            "ok": false,
            "message": "Insufficient balance"
        })));
    }

    sqlx::query!(
        r#"INSERT INTO transactions (tx_id, account_id, tx_type, amount_nmc, balance_after, reference, description)
           VALUES ($1, $2, 'withdraw', $3,
               (SELECT available_nmc FROM credit_accounts WHERE account_id = $2),
               $4, 'Withdrawal request')"#,
        tx_id, account_id, -body.amount_nmc, body.dest_address,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(account_id, amount = body.amount_nmc, dest = %body.dest_address, "Withdrawal initiated");

    Ok(Json(serde_json::json!({
        "ok": true,
        "transaction_id": tx_id,
        "message": "Withdrawal queued for processing",
    })))
}

// ──────────────────────────────────────────────
// Escrow
// ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct EscrowLockBody {
    pub job_id: String,
    pub consumer_id: String,
    pub provider_id: Option<String>,
    pub amount_nmc: f64,
}

pub async fn lock_escrow(
    State(state): State<LedgerState>,
    Json(body): Json<EscrowLockBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let escrow_id = Uuid::new_v4().to_string();

    // Debit consumer available → escrowed
    let result = sqlx::query!(
        r#"
        UPDATE credit_accounts
        SET available_nmc = available_nmc - $1,
            escrowed_nmc  = escrowed_nmc  + $1,
            updated_at = now()
        WHERE account_id = $2
          AND available_nmc >= $1
        "#,
        body.amount_nmc, body.consumer_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if result.rows_affected() == 0 {
        return Ok(Json(serde_json::json!({ "ok": false, "message": "Insufficient balance for escrow" })));
    }

    // Create escrow record
    sqlx::query!(
        "INSERT INTO escrows (escrow_id, job_id, consumer_id, provider_id, locked_nmc) VALUES ($1, $2, $3, $4, $5)",
        escrow_id, body.job_id, body.consumer_id, body.provider_id, body.amount_nmc,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(escrow_id = %escrow_id, job_id = %body.job_id, amount = body.amount_nmc, "Escrow locked");

    Ok(Json(serde_json::json!({ "ok": true, "escrow_id": escrow_id })))
}

#[derive(Deserialize)]
pub struct EscrowReleaseBody {
    pub escrow_id: String,
    pub job_id: String,
    pub actual_runtime_s: u64,
    pub actual_price_per_hour: f64,
}

pub async fn release_escrow(
    State(state): State<LedgerState>,
    Json(body): Json<EscrowReleaseBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let escrow = sqlx::query!(
        "SELECT consumer_id, provider_id, locked_nmc FROM escrows WHERE escrow_id = $1 AND state = 'locked'",
        body.escrow_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let actual_hours = body.actual_runtime_s as f64 / 3600.0;
    let locked = escrow.locked_nmc;
    let actual_cost  = (body.actual_price_per_hour * actual_hours).min(locked);
    let fee          = actual_cost * 0.08; // 8% platform fee
    let provider_gets = actual_cost - fee; // provider receives 92%
    let consumer_refund = locked - actual_cost;

    let consumer_id = escrow.consumer_id;
    let provider_id = escrow.provider_id.unwrap_or_default();

    // Release escrow: debit escrowed from consumer, credit provider
    sqlx::query!(
        r#"
        UPDATE credit_accounts
        SET escrowed_nmc  = escrowed_nmc  - $1,
            available_nmc = available_nmc + $2,  -- refund
            updated_at = now()
        WHERE account_id = $3
        "#,
        escrow.locked_nmc, consumer_refund, consumer_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    sqlx::query!(
        r#"
        INSERT INTO credit_accounts (account_id, available_nmc, total_earned_nmc)
        VALUES ($1, $2, $2)
        ON CONFLICT (account_id) DO UPDATE
        SET available_nmc   = credit_accounts.available_nmc + $2,
            total_earned_nmc = credit_accounts.total_earned_nmc + $2,
            updated_at = now()
        "#,
        provider_id, provider_gets,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Mark escrow settled
    sqlx::query!(
        "UPDATE escrows SET state = 'released', settled_at = now() WHERE escrow_id = $1",
        body.escrow_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(
        escrow_id = %body.escrow_id,
        provider_gets, consumer_refund, fee,
        "Escrow released"
    );

    Ok(Json(serde_json::json!({
        "ok": true,
        "provider_credited":  provider_gets,
        "consumer_refunded":  consumer_refund,
        "platform_fee":       fee,
    })))
}

pub async fn list_transactions(
    State(state): State<LedgerState>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let rows = sqlx::query!(
        "SELECT tx_id, tx_type, amount_nmc, balance_after, reference, description, created_at FROM transactions WHERE account_id = $1 ORDER BY created_at DESC LIMIT 100",
        account_id,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let txs: Vec<_> = rows.into_iter().map(|r| serde_json::json!({
        "tx_id":        r.tx_id,
        "tx_type":      r.tx_type,
        "amount_nmc":   r.amount_nmc,
        "balance_after": r.balance_after.unwrap_or(0.0),
        "reference":    r.reference,
        "description":  r.description,
        "created_at":   r.created_at.map(|t| t.to_string()),
    })).collect();

    Ok(Json(serde_json::json!({ "transactions": txs })))
}
