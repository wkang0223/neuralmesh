//! REST API for the dashboard and external integrations.

use super::AppState;
use anyhow::Result;
use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, StatusCode},
    response::{Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use uuid::Uuid;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health",                    get(health))
        .route("/api/v1/providers",          get(list_providers))
        .route("/api/v1/providers/:id",      get(get_provider))
        .route("/api/v1/stats",              get(network_stats))
        // ── Jobs (consumer-facing) ────────────────────────────────────────
        .route("/api/v1/jobs",               get(list_jobs_rest).post(submit_job))
        .route("/api/v1/jobs/:id",           get(get_job).delete(cancel_job))
        .route("/api/v1/jobs/:id/logs",      get(get_job_logs).post(append_job_log))
        // ── Device-locked account endpoints ──────────────────────────────
        .route("/api/v1/account/register",      post(register_account))
        .route("/api/v1/account/:id",           get(get_account))
        .route("/api/v1/account/:id/verify",    post(verify_device))
        .route("/api/v1/account/:id/reregister", post(reregister_device))
        // ── Ledger (balance, transactions) ────────────────────────────────
        .route("/api/v1/balance/:id",           get(get_balance))
        .route("/api/v1/transactions",          get(list_transactions))
        // ── KYC ──────────────────────────────────────────────────────────
        .route("/api/v1/kyc/:id",              get(get_kyc))
        .route("/api/v1/kyc/submit",           post(submit_kyc))
        // ── Provider job lifecycle ────────────────────────────────────────
        .route("/api/v1/provider/:id/job",      get(get_assigned_job))
        .route("/api/v1/jobs/:id/complete",     post(complete_job))
        .route("/api/v1/jobs/:id/fail",         post(fail_job))
        .route("/api/v1/jobs/:id/checkpoint",   get(get_checkpoint).post(save_checkpoint))
        .route("/api/v1/jobs/:id/heartbeat",    post(job_heartbeat))
        .route("/api/v1/artifacts",             post(upload_artifact))
        .route("/api/v1/artifacts/:hash",       get(download_artifact))
        // ── Withdraw ──────────────────────────────────────────────────────
        .route("/api/v1/withdraw",              post(withdraw))
        // ── Ledger (Stripe webhook credits) ──────────────────────────────
        .route("/api/v1/ledger/stripe-credit",  post(stripe_credit))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub async fn serve(state: AppState, addr: String) -> Result<()> {
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "REST API server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "neuralmesh-coordinator" }))
}

#[derive(Deserialize)]
struct ProviderQuery {
    runtime: Option<String>,
    min_ram: Option<i32>,
    region: Option<String>,
    limit: Option<i64>,
}

async fn list_providers(
    State(state): State<AppState>,
    Query(q): Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let rows = sqlx::query!(
        r#"
        SELECT provider_id, chip_model, unified_memory_gb, gpu_cores,
               floor_price_nmc_per_hour, region, trust_score,
               jobs_completed, state, last_seen, installed_runtimes, max_job_ram_gb
        FROM providers
        WHERE state = 'available'
          AND ($1::int IS NULL OR max_job_ram_gb >= $1)
          AND ($2::text IS NULL OR region = $2)
        ORDER BY floor_price_nmc_per_hour ASC
        LIMIT $3
        "#,
        q.min_ram,
        q.region,
        q.limit.unwrap_or(50),
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let providers: Vec<_> = rows.into_iter().map(|r| serde_json::json!({
        "id":                       r.provider_id,
        "chip_model":               r.chip_model,
        "unified_memory_gb":        r.unified_memory_gb,
        "gpu_cores":                r.gpu_cores,
        "floor_price_nmc_per_hour": r.floor_price_nmc_per_hour,
        "region":                   r.region,
        "trust_score":              r.trust_score,
        "jobs_completed":           r.jobs_completed,
        "state":                    r.state,
        "installed_runtimes":       r.installed_runtimes.unwrap_or_default(),
        "max_job_ram_gb":           r.max_job_ram_gb,
        "last_seen":                r.last_seen.map(|t| t.to_string()),
    })).collect();

    Ok(Json(serde_json::json!({ "providers": providers, "total": providers.len() })))
}

async fn get_provider(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let row = sqlx::query!(
        "SELECT * FROM providers WHERE provider_id = $1",
        provider_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(serde_json::json!({
        "provider_id": row.provider_id,
        "chip_model": row.chip_model,
        "unified_memory_gb": row.unified_memory_gb,
        "state": row.state,
        "floor_price_nmc_per_hour": row.floor_price_nmc_per_hour,
        "jobs_completed": row.jobs_completed,
        "trust_score": row.trust_score,
    })))
}

// ═══════════════════════════════════════════════════════════════════════════
// Jobs — consumer-facing endpoints
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct JobQuery {
    account_id: Option<String>,
    state:      Option<String>,
    limit:      Option<i64>,
}

/// GET /api/v1/jobs — list jobs, optionally filtered by account_id / state
async fn list_jobs_rest(
    State(state): State<AppState>,
    Query(q): Query<JobQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let rows = sqlx::query!(
        r#"
        SELECT job_id, consumer_id, provider_id, state, runtime,
               min_ram_gb, max_price_per_hour, price_per_hour,
               bundle_hash, bundle_url,
               actual_runtime_s, checkpoint_url,
               started_at, completed_at, created_at
        FROM jobs
        WHERE ($1::TEXT IS NULL OR consumer_id = $1)
          AND ($2::TEXT IS NULL OR state = $2)
        ORDER BY created_at DESC
        LIMIT $3
        "#,
        q.account_id as Option<String>,
        q.state      as Option<String>,
        q.limit.unwrap_or(50),
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let jobs: Vec<_> = rows.into_iter().map(|r| {
        // Derive actual_cost_nmc from runtime (92% of gross)
        let actual_cost_nmc = match (r.price_per_hour, r.actual_runtime_s) {
            (Some(p), Some(s)) => Some(p * (s as f64 / 3600.0)),
            _ => None,
        };
        // Convert integer runtime enum back to string name for CLI
        let runtime_str = match r.runtime {
            Some(0) => "mlx",
            Some(1) => "torch-mps",
            Some(2) => "onnx-coreml",
            Some(3) => "llama-cpp",
            Some(4) => "shell",
            _       => "shell",
        };
        serde_json::json!({
            "id":                 r.job_id,
            "job_id":             r.job_id,
            "account_id":         r.consumer_id,
            "consumer_id":        r.consumer_id,
            "provider_id":        r.provider_id,
            "state":              r.state,
            "runtime":            runtime_str,
            "min_ram_gb":         r.min_ram_gb,
            "max_price_per_hour": r.max_price_per_hour,
            "bundle_hash":        r.bundle_hash,
            "actual_cost_nmc":    actual_cost_nmc,
            "has_checkpoint":     r.checkpoint_url.is_some(),
            "started_at":         r.started_at.map(|t| t.to_string()),
            "completed_at":       r.completed_at.map(|t| t.to_string()),
            "created_at":         r.created_at.map(|t| t.to_string()),
        })
    }).collect();

    Ok(Json(serde_json::json!({ "jobs": jobs, "total": jobs.len() })))
}

/// GET /api/v1/jobs/:id — single job detail
async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let r = sqlx::query!(
        r#"
        SELECT job_id, consumer_id, provider_id, state, runtime,
               min_ram_gb, max_price_per_hour, price_per_hour,
               bundle_hash, bundle_url, output_hash,
               actual_runtime_s, restore_attempts, failure_reason,
               checkpoint_url, checkpoint_iter,
               started_at, completed_at, created_at
        FROM jobs
        WHERE job_id = $1
        "#,
        job_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let actual_cost_nmc = match (r.price_per_hour, r.actual_runtime_s) {
        (Some(p), Some(s)) => Some(p * (s as f64 / 3600.0)),
        _ => None,
    };
    let runtime_str = match r.runtime {
        Some(0) => "mlx",
        Some(1) => "torch-mps",
        Some(2) => "onnx-coreml",
        Some(3) => "llama-cpp",
        Some(4) => "shell",
        _       => "shell",
    };

    Ok(Json(serde_json::json!({
        "id":                 r.job_id,
        "job_id":             r.job_id,
        "account_id":         r.consumer_id,
        "consumer_id":        r.consumer_id,
        "provider_id":        r.provider_id,
        "state":              r.state,
        "runtime":            runtime_str,
        "min_ram_gb":         r.min_ram_gb,
        "max_price_per_hour": r.max_price_per_hour,
        "price_per_hour":     r.price_per_hour,
        "bundle_hash":        r.bundle_hash,
        "bundle_url":         r.bundle_url,
        "output_hash":        r.output_hash,
        "actual_cost_nmc":    actual_cost_nmc,
        "actual_runtime_s":   r.actual_runtime_s,
        "restore_attempts":   r.restore_attempts,
        "failure_reason":     r.failure_reason,
        "has_checkpoint":     r.checkpoint_url.is_some(),
        "checkpoint_iter":    r.checkpoint_iter,
        "started_at":         r.started_at.map(|t| t.to_string()),
        "completed_at":       r.completed_at.map(|t| t.to_string()),
        "created_at":         r.created_at.map(|t| t.to_string()),
    })))
}

/// POST /api/v1/jobs — submit a new job
#[derive(Deserialize)]
struct SubmitJobBody {
    account_id:        String,
    runtime:           String,   // "mlx" | "torch-mps" | "onnx-coreml" | "shell"
    min_ram_gb:        i32,
    max_duration_secs: i32,
    max_price_per_hour: f64,
    bundle_hash:       Option<String>,
    bundle_url:        Option<String>,
    /// Entry-point filename inside the bundle (e.g. "inference.py").
    /// Agent uses this to find the right file instead of heuristic scanning.
    script_name:       Option<String>,
    /// Caller's preferred region — optional
    preferred_region:  Option<String>,
    /// Environment variables passed to the job (coordinator validates keys)
    env_vars:          Option<serde_json::Value>,
}

async fn submit_job(
    State(state): State<AppState>,
    Json(body): Json<SubmitJobBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    use tracing::warn;

    // Validate account exists and has enough balance
    let credit = sqlx::query!(
        "SELECT available_nmc FROM credit_accounts WHERE account_id = $1",
        body.account_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "db_error"})),
    ))?;

    let available = credit.map(|c| c.available_nmc.unwrap_or(0.0)).unwrap_or(0.0);
    let max_cost = body.max_price_per_hour * (body.max_duration_secs as f64 / 3600.0);

    if available < max_cost {
        return Err((
            StatusCode::PAYMENT_REQUIRED,
            Json(serde_json::json!({
                "ok": false,
                "error": "insufficient_balance",
                "message": format!(
                    "Need {:.4} HC escrow for max job cost, only {:.4} available. \
                     Run `hatch wallet deposit` to add credits.",
                    max_cost, available
                ),
            })),
        ));
    }

    // Map runtime string to integer enum (matches nm_common::Runtime)
    let runtime_int: i32 = match body.runtime.as_str() {
        "mlx"        => 0,
        "torch-mps"  => 1,
        "onnx-coreml"=> 2,
        "llama-cpp"  => 3,
        "shell"      => 4,
        _            => 4,
    };

    let job_id = Uuid::new_v4().to_string();

    // Insert job as queued
    sqlx::query!(
        r#"
        INSERT INTO jobs (
            job_id, consumer_id, runtime, min_ram_gb, max_duration_s,
            max_price_per_hour, bundle_hash, bundle_url, script_name, preferred_region, state
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'queued')
        "#,
        job_id,
        body.account_id,
        runtime_int,
        body.min_ram_gb,
        body.max_duration_secs,
        body.max_price_per_hour,
        body.bundle_hash,
        body.bundle_url,
        body.script_name,
        body.preferred_region,
    )
    .execute(&state.db)
    .await
    .map_err(|e| {
        warn!(error = %e, "Failed to insert job");
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "db_error"})))
    })?;

    // Lock escrow (max possible cost)
    sqlx::query!(
        r#"
        UPDATE credit_accounts
        SET available_nmc = available_nmc - $1,
            escrowed_nmc  = escrowed_nmc + $1,
            updated_at    = now()
        WHERE account_id = $2
        "#,
        max_cost,
        body.account_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "db_error"}))))?;

    // Record escrow
    let escrow_id = Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO escrows (escrow_id, job_id, consumer_id, locked_nmc)
        VALUES ($1, $2, $3, $4)
        "#,
        escrow_id,
        job_id,
        body.account_id,
        max_cost,
    )
    .execute(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "db_error"}))))?;

    let tx_id = Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO transactions (tx_id, account_id, tx_type, amount_nmc, balance_after, reference, description)
        VALUES ($1, $2, 'escrow_lock', $3,
            (SELECT available_nmc FROM credit_accounts WHERE account_id = $2),
            $4, 'Job escrow locked')
        "#,
        tx_id,
        body.account_id,
        -max_cost,   // negative = money moved out of available balance
        job_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "db_error"}))))?;

    info!(
        job_id = %job_id,
        account_id = %body.account_id,
        runtime = %body.runtime,
        max_cost,
        "Job submitted"
    );

    Ok(Json(serde_json::json!({
        "ok":                  true,
        "job_id":              job_id,
        "state":               "queued",
        "estimated_wait_secs": 30,  // next auction cycle
        "locked_nmc":          max_cost,
    })))
}

/// DELETE /api/v1/jobs/:id — cancel a queued or running job
async fn cancel_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let job = sqlx::query!(
        "SELECT consumer_id, state FROM jobs WHERE job_id = $1",
        job_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    match job.state.as_deref() {
        Some("complete") | Some("failed") | Some("cancelled") => {
            return Ok(Json(serde_json::json!({
                "ok": false,
                "error": "already_terminal",
                "state": job.state,
            })));
        }
        _ => {}
    }

    // Mark cancelled
    sqlx::query!(
        "UPDATE jobs SET state = 'cancelled', completed_at = now() WHERE job_id = $1",
        job_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Refund escrow
    sqlx::query!(
        r#"
        UPDATE credit_accounts
        SET available_nmc = available_nmc + (
              SELECT locked_nmc FROM escrows WHERE job_id = $1 AND state = 'locked' LIMIT 1
            ),
            escrowed_nmc = GREATEST(0, escrowed_nmc - (
              SELECT COALESCE(locked_nmc, 0) FROM escrows WHERE job_id = $1 AND state = 'locked' LIMIT 1
            )),
            updated_at = now()
        WHERE account_id = $2
        "#,
        job_id,
        job.consumer_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    sqlx::query!(
        "UPDATE escrows SET state = 'released', settled_at = now() WHERE job_id = $1 AND state = 'locked'",
        job_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Free provider if one was assigned
    let _ = sqlx::query!(
        "UPDATE providers SET state = 'available', active_job_id = NULL WHERE active_job_id = $1",
        job_id,
    )
    .execute(&state.db)
    .await;

    info!(job_id = %job_id, "Job cancelled");
    Ok(Json(serde_json::json!({ "ok": true, "job_id": job_id, "state": "cancelled" })))
}

/// GET /api/v1/jobs/:id/logs
/// Returns accumulated job output stored in the DB (pushed by the agent).
/// Supports `?offset=N` for incremental polling.
#[derive(Deserialize)]
struct LogsQuery {
    offset: Option<usize>,
}

async fn get_job_logs(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Query(q): Query<LogsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let offset = q.offset.unwrap_or(0);

    let job = sqlx::query!(
        "SELECT state, output_log FROM jobs WHERE job_id = $1",
        job_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let is_complete = matches!(
        job.state.as_deref(),
        Some("complete") | Some("failed") | Some("cancelled")
    );

    let full_log = job.output_log.unwrap_or_default();
    let output = if full_log.len() > offset {
        full_log[offset..].to_string()
    } else {
        String::new()
    };

    Ok(Json(serde_json::json!({
        "job_id":      job_id,
        "output":      output,
        "is_complete": is_complete,
        "offset":      offset + output.len(),
    })))
}

/// POST /api/v1/jobs/:id/logs
/// Called by the agent after job completion to push accumulated stdout/stderr.
/// The body is `{ "output": "<log text>" }`.
#[derive(Deserialize)]
struct LogAppendBody {
    output: String,
}

async fn append_job_log(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(body): Json<LogAppendBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if body.output.is_empty() {
        return Ok(Json(serde_json::json!({ "ok": true, "appended": 0 })));
    }

    sqlx::query!(
        r#"
        UPDATE jobs
        SET output_log = COALESCE(output_log, '') || $1
        WHERE job_id = $2
        "#,
        body.output,
        job_id,
    )
    .execute(&state.db)
    .await
    .map_err(|e| {
        warn!(job_id, error = %e, "Failed to append job log");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(serde_json::json!({
        "ok":      true,
        "appended": body.output.len(),
    })))
}

// ═══════════════════════════════════════════════════════════════════════════
// Ledger — balance & transactions (consolidated on coordinator for Phase 1)
// ═══════════════════════════════════════════════════════════════════════════

/// GET /api/v1/balance/:id
async fn get_balance(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let bal = sqlx::query!(
        r#"
        SELECT available_nmc, escrowed_nmc, total_earned_nmc, total_spent_nmc
        FROM credit_accounts
        WHERE account_id = $1
        "#,
        account_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match bal {
        Some(b) => Ok(Json(serde_json::json!({
            "account_id":       account_id,
            "available_nmc":    b.available_nmc,
            "escrowed_nmc":     b.escrowed_nmc,
            "total_earned_nmc": b.total_earned_nmc,
            "total_spent_nmc":  b.total_spent_nmc,
        }))),
        None => Ok(Json(serde_json::json!({
            "account_id":       account_id,
            "available_nmc":    0.0,
            "escrowed_nmc":     0.0,
            "total_earned_nmc": 0.0,
            "total_spent_nmc":  0.0,
        }))),
    }
}

/// GET /api/v1/transactions?account_id=...&limit=...
#[derive(Deserialize)]
struct TxQuery {
    account_id: String,
    limit:      Option<i64>,
}

async fn list_transactions(
    State(state): State<AppState>,
    Query(q): Query<TxQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let rows = sqlx::query!(
        r#"
        SELECT tx_id, tx_type, amount_nmc, reference, description, created_at
        FROM transactions
        WHERE account_id = $1
        ORDER BY created_at DESC
        LIMIT $2
        "#,
        q.account_id,
        q.limit.unwrap_or(50),
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let txs: Vec<_> = rows.into_iter().map(|r| serde_json::json!({
        "id":          r.tx_id,
        "kind":        r.tx_type,
        "amount_nmc":  r.amount_nmc,
        "description": r.description,
        "reference":   r.reference,
        "created_at":  r.created_at.map(|t| t.to_string()),
    })).collect();

    Ok(Json(serde_json::json!({ "transactions": txs, "total": txs.len() })))
}

// ═══════════════════════════════════════════════════════════════════════════
// KYC
// ═══════════════════════════════════════════════════════════════════════════

/// GET /api/v1/kyc/:id — fetch KYC record for an account
async fn get_kyc(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Actual schema: account_id, full_name, id_type, country_code, kyc_level,
    //                annual_limit_myr, submitted_at, verified_at, archived
    let kyc = sqlx::query!(
        r#"
        SELECT account_id, full_name, id_type, country_code,
               kyc_level, annual_limit_myr::double precision AS annual_limit_myr,
               submitted_at, verified_at, archived
        FROM kyc_records
        WHERE account_id = $1
        "#,
        account_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match kyc {
        Some(k) => {
            // Derive status from verified_at / archived
            let status = if k.archived {
                "rejected"
            } else if k.verified_at.is_some() {
                "approved"
            } else {
                "pending"
            };
            Ok(Json(serde_json::json!({
                "account_id":       k.account_id,
                "status":           status,
                "compliance_level": k.kyc_level,
                "full_name":        k.full_name,
                "id_type":          k.id_type,
                "country":          k.country_code,
                "annual_limit_myr": k.annual_limit_myr,
                "submitted_at":     k.submitted_at.to_string(),
                "approved_at":      k.verified_at.map(|t: time::OffsetDateTime| t.to_string()),
            })))
        }
        None => Ok(Json(serde_json::json!({
            "account_id":       account_id,
            "status":           "not_submitted",
            "compliance_level": 0,
        }))),
    }
}

/// POST /api/v1/kyc/submit
#[derive(Deserialize)]
struct KycSubmitBody {
    account_id: String,
    full_name:  String,
    id_type:    String,  // "mykad" | "passport" | "nric" | "other"
    id_number:  String,  // hashed before storing
    country:    String,  // 2-char ISO code
}

async fn submit_kyc(
    State(state): State<AppState>,
    Json(body): Json<KycSubmitBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sha2::{Digest as _, Sha256};

    if body.full_name.is_empty() || body.id_number.is_empty() || body.country.len() < 2 {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Hash the ID number before storing (PII protection)
    let id_hash = hex::encode(Sha256::digest(body.id_number.as_bytes()));

    // Truncate country to 2 chars
    let country_code = &body.country[..2.min(body.country.len())];

    sqlx::query!(
        r#"
        INSERT INTO kyc_records (
            account_id, full_name, id_type, id_number_hash,
            country_code, kyc_level, annual_limit_myr,
            acknowledged_terms, acknowledged_at, submitted_at
        ) VALUES ($1, $2, $3, $4, $5, 1, 5000.0, true, now(), now())
        ON CONFLICT (account_id) DO UPDATE
            SET full_name       = $2,
                id_type         = $3,
                id_number_hash  = $4,
                country_code    = $5,
                submitted_at    = now(),
                archived        = false
        "#,
        body.account_id,
        body.full_name,
        body.id_type,
        id_hash,
        country_code,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(
        account_id = %body.account_id,
        id_type = %body.id_type,
        country = %country_code,
        "KYC submitted"
    );

    Ok(Json(serde_json::json!({
        "ok":               true,
        "status":           "pending",
        "compliance_level": 1,
        "annual_limit_myr": 5000.0,
        "message":          "KYC submitted — will be reviewed within 1 business day",
    })))
}

// ─── Device-locked account registration ──────────────────────────────────────

/// Body sent by the browser/CLI when creating a new account.
#[derive(Deserialize)]
struct RegisterAccountBody {
    /// ECDSA P-256 public key in SPKI format, hex-encoded (Web Crypto exportKey "spki")
    ecdsa_pubkey_hex: String,
    /// SHA-256(device fingerprint signals), hex-encoded
    device_fingerprint_hash: String,
    /// Optional human label, e.g. "Alice's MacBook Pro"
    device_label: Option<String>,
    /// "macos" | "linux" | "windows" | "browser"
    platform: Option<String>,
    /// BLAKE3(IOPlatformUUID + serial) — only sent by the CLI agent, empty for browser
    hardware_serial_hash: Option<String>,
}

/// Derive account_id = hex(SHA-256(pubkey_hex || fingerprint_hash))[..24]
fn derive_account_id(pubkey_hex: &str, fingerprint_hash: &str) -> String {
    let mut h = Sha256::new();
    h.update(pubkey_hex.as_bytes());
    h.update(b"||");
    h.update(fingerprint_hash.as_bytes());
    let result = h.finalize();
    hex::encode(&result[..12]) // 24 hex chars = 96 bits — collision-resistant for our scale
}

async fn register_account(
    State(state): State<AppState>,
    Json(body): Json<RegisterAccountBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Validate pubkey format (must be non-empty hex)
    if body.ecdsa_pubkey_hex.is_empty() || body.device_fingerprint_hash.len() != 64 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let account_id = derive_account_id(&body.ecdsa_pubkey_hex, &body.device_fingerprint_hash);
    let platform = body.platform.unwrap_or_else(|| "browser".to_string());

    // Upsert — idempotent: same pubkey+fingerprint always yields the same account_id
    let result = sqlx::query!(
        r#"
        INSERT INTO accounts (
            account_id, ecdsa_pubkey_hex, device_fingerprint_hash,
            device_label, platform, hardware_serial_hash
        ) VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (account_id) DO UPDATE
            SET last_seen    = now(),
                device_label = COALESCE($4, accounts.device_label)
        RETURNING account_id, created_at, last_seen
        "#,
        account_id,
        body.ecdsa_pubkey_hex,
        body.device_fingerprint_hash,
        body.device_label,
        platform,
        body.hardware_serial_hash,
    )
    .fetch_one(&state.db)
    .await;

    match result {
        Ok(row) => {
            // Also ensure a credit_accounts entry exists for this account
            let _ = sqlx::query!(
                "INSERT INTO credit_accounts (account_id) VALUES ($1) ON CONFLICT DO NOTHING",
                account_id,
            )
            .execute(&state.db)
            .await;

            info!(account_id = %account_id, platform = %platform, "Account registered/refreshed");
            Ok(Json(serde_json::json!({
                "ok": true,
                "account_id": row.account_id,
                "created_at": row.created_at.map(|t| t.to_string()),
                "last_seen":  row.last_seen.map(|t| t.to_string()),
            })))
        }
        Err(e) => {
            // Unique constraint on ecdsa_pubkey_hex — different fingerprint on same key
            if e.to_string().contains("unique") {
                return Ok(Json(serde_json::json!({
                    "ok": false,
                    "error": "device_mismatch",
                    "message": "This key is already registered to a different device fingerprint."
                })));
            }
            tracing::error!(error = %e, "Account registration failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_account(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let acc = sqlx::query!(
        "SELECT account_id, device_label, platform, role, active, created_at, last_seen FROM accounts WHERE account_id = $1",
        account_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let bal = sqlx::query!(
        "SELECT available_nmc, escrowed_nmc, total_earned_nmc, total_spent_nmc FROM credit_accounts WHERE account_id = $1",
        account_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "account_id":    acc.account_id,
        "device_label":  acc.device_label,
        "platform":      acc.platform,
        "role":          acc.role,
        "active":        acc.active,
        "created_at":    acc.created_at.map(|t| t.to_string()),
        "last_seen":     acc.last_seen.map(|t| t.to_string()),
        "balance": bal.map(|b| serde_json::json!({
            "available_nmc":    b.available_nmc,
            "escrowed_nmc":     b.escrowed_nmc,
            "total_earned_nmc": b.total_earned_nmc,
            "total_spent_nmc":  b.total_spent_nmc,
        })),
    })))
}

/// Verify that the requesting device matches the stored fingerprint for an account.
#[derive(Deserialize)]
struct VerifyDeviceBody {
    device_fingerprint_hash: String,
    ecdsa_pubkey_hex: String,
}

async fn verify_device(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Json(body): Json<VerifyDeviceBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let acc = sqlx::query!(
        "SELECT ecdsa_pubkey_hex, device_fingerprint_hash FROM accounts WHERE account_id = $1 AND active = TRUE",
        account_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let pubkey_match = acc.ecdsa_pubkey_hex == body.ecdsa_pubkey_hex;
    let fingerprint_match = acc.device_fingerprint_hash == body.device_fingerprint_hash;

    if pubkey_match && fingerprint_match {
        // Update last_seen
        let _ = sqlx::query!(
            "UPDATE accounts SET last_seen = now() WHERE account_id = $1",
            account_id
        ).execute(&state.db).await;

        Ok(Json(serde_json::json!({ "ok": true, "verified": true })))
    } else {
        Ok(Json(serde_json::json!({
            "ok": true,
            "verified": false,
            "reason": if !pubkey_match { "key_mismatch" } else { "fingerprint_mismatch" }
        })))
    }
}

// ─────────────────────────────────────────────────────────────────────────────

async fn network_stats(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let stats = sqlx::query!(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE state = 'available') AS available_providers,
            COUNT(*) FILTER (WHERE state = 'leased')    AS active_providers,
            SUM(unified_memory_gb) FILTER (WHERE state = 'available') AS total_available_ram_gb
        FROM providers
        "#
    )
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let job_stats = sqlx::query!(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE state = 'running')  AS running_jobs,
            COUNT(*) FILTER (WHERE state = 'complete') AS completed_jobs
        FROM jobs
        "#
    )
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "available_providers":  stats.available_providers,
        "active_providers":     stats.active_providers,
        "total_available_ram_gb": stats.total_available_ram_gb,
        "running_jobs":         job_stats.running_jobs,
        "completed_jobs":       job_stats.completed_jobs,
    })))
}

// ─── Re-register device (update fingerprint for existing account) ─────────────

/// Update the device fingerprint for an existing account.
/// Used when the browser/OS is updated and stable signals shift.
#[derive(Deserialize)]
struct ReregisterBody {
    ecdsa_pubkey_hex: String,
    new_device_fingerprint_hash: String,
    old_device_fingerprint_hash: String,
    device_label: Option<String>,
}

async fn reregister_device(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Json(body): Json<ReregisterBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if body.new_device_fingerprint_hash.len() != 64 || body.old_device_fingerprint_hash.len() != 64 {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Verify the pubkey matches what's stored — prevents hijacking
    let acc = sqlx::query!(
        "SELECT ecdsa_pubkey_hex FROM accounts WHERE account_id = $1 AND active = TRUE",
        account_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    if acc.ecdsa_pubkey_hex != body.ecdsa_pubkey_hex {
        return Ok(Json(serde_json::json!({
            "ok": false,
            "error": "key_mismatch",
            "message": "Public key does not match the registered account."
        })));
    }

    // Update the fingerprint and last_seen
    let row = sqlx::query!(
        r#"
        UPDATE accounts
        SET device_fingerprint_hash = $1,
            last_seen               = now(),
            device_label            = COALESCE($2, device_label)
        WHERE account_id = $3
        RETURNING account_id, last_seen
        "#,
        body.new_device_fingerprint_hash,
        body.device_label,
        account_id,
    )
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(account_id = %account_id, "Device fingerprint updated");
    Ok(Json(serde_json::json!({
        "ok": true,
        "account_id": row.account_id,
        "last_seen":  row.last_seen.map(|t| t.to_string()),
    })))
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 2 — Job lifecycle endpoints (called by the provider agent)
// ═══════════════════════════════════════════════════════════════════════════

/// GET /api/v1/provider/:id/job
/// Agent polls this every 30 s to discover if a job has been assigned to it.
async fn get_assigned_job(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let job = sqlx::query!(
        r#"
        SELECT job_id, consumer_id, runtime, min_ram_gb, max_duration_s,
               max_price_per_hour, bundle_hash, bundle_url, script_name,
               consumer_ssh_pubkey, consumer_wg_pubkey, preferred_region
        FROM jobs
        WHERE provider_id = $1 AND state = 'assigned'
        ORDER BY started_at ASC
        LIMIT 1
        "#,
        provider_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match job {
        None => Ok(Json(serde_json::json!({ "job": null }))),
        Some(j) => {
            // Transition job → running
            let _ = sqlx::query!(
                "UPDATE jobs SET state = 'running' WHERE job_id = $1",
                j.job_id
            )
            .execute(&state.db)
            .await;

            Ok(Json(serde_json::json!({
                "job": {
                    "job_id":             j.job_id,
                    "consumer_id":        j.consumer_id,
                    "runtime":            j.runtime,
                    "min_ram_gb":         j.min_ram_gb,
                    "max_duration_s":     j.max_duration_s,
                    "max_price_per_hour": j.max_price_per_hour,
                    "bundle_hash":        j.bundle_hash,
                    "bundle_url":         j.bundle_url,
                    "script_name":        j.script_name,
                    "consumer_ssh_pubkey":j.consumer_ssh_pubkey,
                    "consumer_wg_pubkey": j.consumer_wg_pubkey,
                    "preferred_region":   j.preferred_region,
                }
            })))
        }
    }
}

/// POST /api/v1/jobs/:id/complete
/// Agent reports a successfully completed job. Triggers credit flow.
#[derive(Deserialize)]
struct CompleteJobBody {
    provider_id: String,
    exit_code: i32,
    output_hash: String,
    actual_runtime_s: i64,
    avg_gpu_util_pct: f64,
    peak_ram_gb: i32,
}

const PLATFORM_FEE: f64 = 0.08; // 8% platform fee

async fn complete_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(body): Json<CompleteJobBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Fetch job for price calculation
    let job = sqlx::query!(
        "SELECT price_per_hour, consumer_id FROM jobs WHERE job_id = $1 AND provider_id = $2",
        job_id, body.provider_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let hours = body.actual_runtime_s as f64 / 3600.0;
    let gross_nmc = job.price_per_hour.unwrap_or(0.05) * hours;
    let provider_nmc = gross_nmc * (1.0 - PLATFORM_FEE); // 92% to provider
    let platform_nmc = gross_nmc * PLATFORM_FEE;          // 8%  to platform

    // Mark job complete
    sqlx::query!(
        r#"
        UPDATE jobs
        SET state           = 'complete',
            output_hash     = $1,
            actual_runtime_s = $2,
            completed_at    = now()
        WHERE job_id = $3
        "#,
        body.output_hash, body.actual_runtime_s, job_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Credit provider
    sqlx::query!(
        r#"
        INSERT INTO credit_accounts (account_id, available_nmc, total_earned_nmc)
        VALUES ($1, $2, $2)
        ON CONFLICT (account_id) DO UPDATE
        SET available_nmc    = credit_accounts.available_nmc    + $2,
            total_earned_nmc = credit_accounts.total_earned_nmc + $2,
            updated_at       = now()
        "#,
        body.provider_id, provider_nmc,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Release consumer escrow → deduct actual cost
    sqlx::query!(
        r#"
        UPDATE credit_accounts
        SET escrowed_nmc = GREATEST(0.0, escrowed_nmc - $1),
            total_spent_nmc = total_spent_nmc + $1,
            updated_at = now()
        WHERE account_id = $2
        "#,
        gross_nmc, job.consumer_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Free provider
    sqlx::query!(
        "UPDATE providers SET state = 'available', active_job_id = NULL,
         jobs_completed = jobs_completed + 1 WHERE provider_id = $1",
        body.provider_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Log transaction
    let tx_id = Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO transactions (tx_id, account_id, tx_type, amount_nmc, reference, description)
        VALUES ($1, $2, 'earn', $3, $4, 'Job completion credit')
        "#,
        tx_id, body.provider_id, provider_nmc, job_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Update trust score (reputation)
    update_trust_score(&state.db, &body.provider_id, true).await;

    // ── Phase 3: on-chain escrow release ─────────────────────────────────────
    // Spawned in background — coordinator does not block on chain confirmation.
    // The oracle is a no-op when NM_LEDGER_URL + INTERNAL_SECRET are absent.
    if state.oracle.enabled {
        let oracle   = std::sync::Arc::clone(&state.oracle);
        let jid      = job_id.clone();
        let cost_nmc = gross_nmc;
        tokio::spawn(async move {
            match oracle.release_escrow(&jid, cost_nmc).await {
                Ok(Some(tx)) => info!(job_id = %jid, tx_hash = %tx, "On-chain escrow released"),
                Ok(None)     => { /* ledger disabled */ }
                Err(e)       => warn!(
                    job_id = %jid,
                    error  = %e,
                    "On-chain release failed — off-chain credit already settled"
                ),
            }
        });
    }

    info!(
        job_id = %job_id,
        provider_id = %body.provider_id,
        gross_nmc,
        provider_nmc,
        platform_nmc,
        on_chain = state.oracle.enabled,
        "Job completed — credits transferred"
    );

    Ok(Json(serde_json::json!({
        "ok":            true,
        "gross_nmc":     gross_nmc,
        "provider_nmc":  provider_nmc,
        "platform_nmc":  platform_nmc,
        "on_chain":      state.oracle.enabled,
    })))
}

/// POST /api/v1/jobs/:id/fail
/// Agent reports a failed job (crash, OOM, timeout).
#[derive(Deserialize)]
struct FailJobBody {
    provider_id: String,
    reason: String,
    slash: bool, // true = provider fault (lowers trust score)
}

async fn fail_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(body): Json<FailJobBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    sqlx::query!(
        "UPDATE jobs SET state = 'failed', completed_at = now() WHERE job_id = $1",
        job_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Release consumer escrow back
    sqlx::query!(
        r#"
        UPDATE credit_accounts
        SET available_nmc = available_nmc + escrowed_nmc,
            escrowed_nmc  = 0.0,
            updated_at    = now()
        WHERE account_id = (SELECT consumer_id FROM jobs WHERE job_id = $1)
        "#,
        job_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Free provider slot
    sqlx::query!(
        "UPDATE providers SET state = 'available', active_job_id = NULL WHERE provider_id = $1",
        body.provider_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Slash trust score if provider fault
    if body.slash {
        update_trust_score(&state.db, &body.provider_id, false).await;
    }

    // Phase 3: cancel on-chain escrow (full consumer refund)
    if state.oracle.enabled {
        let oracle = std::sync::Arc::clone(&state.oracle);
        let jid    = job_id.clone();
        tokio::spawn(async move {
            match oracle.cancel_escrow(&jid).await {
                Ok(Some(tx)) => info!(job_id = %jid, tx_hash = %tx, "On-chain escrow cancelled"),
                Ok(None)     => {}
                Err(e)       => warn!(job_id = %jid, error = %e, "On-chain cancel failed"),
            }
        });
    }

    info!(job_id = %job_id, reason = %body.reason, slash = body.slash, "Job failed");

    Ok(Json(serde_json::json!({ "ok": true, "job_id": job_id })))
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 2 — Reputation system
// ═══════════════════════════════════════════════════════════════════════════

/// Update provider trust score (0.0–5.0) after a job completes or fails.
/// Success:  +0.05 (capped at 5.0)
/// Failure:  -0.30 (floor at 0.0)
/// Also recalculates success_rate from jobs_completed / total_jobs.
async fn update_trust_score(db: &sqlx::PgPool, provider_id: &str, success: bool) {
    let delta: f64 = if success { 0.05 } else { -0.30 };
    let _ = sqlx::query!(
        r#"
        UPDATE providers SET
            trust_score  = GREATEST(0.0, LEAST(5.0, trust_score + $1)),
            success_rate = CASE
                WHEN jobs_completed > 0
                THEN (success_rate * (jobs_completed - 1) + $2) / jobs_completed
                ELSE $2
            END,
            updated_at   = now()
        WHERE provider_id = $3
        "#,
        delta,
        if success { 1.0_f64 } else { 0.0_f64 },
        provider_id,
    )
    .execute(db)
    .await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Stripe webhook ledger credit (called internally by Next.js webhook handler)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct StripeCreditBody {
    account_id: String,
    amount_nmc: f64,
    stripe_session_id: String,
    amount_myr: f64,
}

async fn stripe_credit(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<StripeCreditBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Validate internal secret via Authorization: Bearer <token>
    let expected = std::env::var("INTERNAL_SECRET").unwrap_or_default();
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("");
    if token != expected {
        return Err(StatusCode::UNAUTHORIZED);
    }

    if body.amount_nmc <= 0.0 {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Idempotency: check if this Stripe session was already credited
    let already = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM deposit_records WHERE stripe_session_id = $1",
        body.stripe_session_id,
    )
    .fetch_one(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .unwrap_or(0);

    if already > 0 {
        return Ok(Json(serde_json::json!({ "ok": true, "duplicate": true })));
    }

    // Credit NMC balance
    sqlx::query!(
        r#"
        INSERT INTO credit_accounts (account_id, available_nmc, updated_at)
        VALUES ($1, $2, now())
        ON CONFLICT (account_id) DO UPDATE
        SET available_nmc = credit_accounts.available_nmc + $2,
            updated_at    = now()
        "#,
        body.account_id, body.amount_nmc,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Record deposit for KYC annual limit tracking
    sqlx::query!(
        "INSERT INTO deposit_records (stripe_session_id, account_id, amount_myr) VALUES ($1, $2, $3::double precision)",
        body.stripe_session_id, body.account_id, body.amount_myr,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Log transaction
    let tx_id = Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO transactions (tx_id, account_id, tx_type, amount_nmc, reference, description)
        VALUES ($1, $2, 'deposit', $3, $4, 'Stripe top-up')
        "#,
        tx_id, body.account_id, body.amount_nmc, body.stripe_session_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(
        account_id = %body.account_id,
        amount_nmc = body.amount_nmc,
        session_id = %body.stripe_session_id,
        "Stripe credit applied"
    );

    Ok(Json(serde_json::json!({ "ok": true, "credited_nmc": body.amount_nmc })))
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 2 — DMTCP checkpoint tracking
// ═══════════════════════════════════════════════════════════════════════════

/// POST /api/v1/jobs/:id/checkpoint
/// Provider reports that a DMTCP checkpoint was saved.
/// The coordinator records the checkpoint and updates the job's checkpoint_url
/// so that if the provider disconnects, the job can be re-queued for restore.
#[derive(Deserialize)]
struct CheckpointBody {
    provider_id: String,
    iteration: i32,
    elapsed_secs: i64,
    /// Absolute local path on the provider — useful for same-provider restore
    checkpoint_dir: String,
    /// Names of .dmtcp files (stored so a restore agent knows what to request)
    dmtcp_files: Vec<String>,
}

async fn save_checkpoint(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(body): Json<CheckpointBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tracing::warn;

    // Upsert checkpoint record
    let ckpt_id = Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO job_checkpoints
            (checkpoint_id, job_id, provider_id, iteration,
             checkpoint_dir, dmtcp_files, elapsed_secs)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        ckpt_id,
        job_id,
        body.provider_id,
        body.iteration,
        body.checkpoint_dir,
        &body.dmtcp_files,
        body.elapsed_secs,
    )
    .execute(&state.db)
    .await
    .map_err(|e| {
        warn!(job_id, error = %e, "Failed to insert checkpoint record");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Store the checkpoint URL on the job itself — Phase 1: use provider's local path
    // Phase 2: this would be an S3/R2 URL after upload.
    let checkpoint_url = format!(
        "file://{}",
        body.checkpoint_dir.trim_end_matches('/')
    );

    sqlx::query!(
        r#"
        UPDATE jobs
        SET checkpoint_url  = $1,
            checkpoint_iter = $2,
            last_heartbeat  = now()
        WHERE job_id = $3
        "#,
        checkpoint_url,
        body.iteration,
        job_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(
        job_id = %job_id,
        provider_id = %body.provider_id,
        iteration = body.iteration,
        elapsed_secs = body.elapsed_secs,
        "Checkpoint recorded"
    );

    Ok(Json(serde_json::json!({
        "ok": true,
        "checkpoint_id": ckpt_id,
        "checkpoint_url": checkpoint_url,
    })))
}

/// POST /api/v1/jobs/:id/heartbeat
/// GET /api/v1/jobs/:id/checkpoint
/// Returns the latest checkpoint metadata stored for this job.
/// Called by the agent on the *restore* side to fetch checkpoint info
/// without needing access to the original provider's local filesystem.
async fn get_checkpoint(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let row = sqlx::query!(
        r#"
        SELECT checkpoint_id, provider_id, iteration, checkpoint_dir,
               dmtcp_files, elapsed_secs
        FROM job_checkpoints
        WHERE job_id = $1
        ORDER BY iteration DESC
        LIMIT 1
        "#,
        job_id,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(serde_json::json!({
        "job_id":         job_id,
        "iteration":      row.iteration,
        "checkpoint_dir": row.checkpoint_dir,
        "dmtcp_files":    row.dmtcp_files,
        "elapsed_secs":   row.elapsed_secs,
        "coord_port":     0,   // coord_port not stored in DB; restore uses a fresh coordinator
    })))
}

/// Provider sends periodic job progress heartbeat (separate from provider heartbeat).
/// Updates last_heartbeat on the job row so the heartbeat watcher knows the
/// job is still alive.
#[derive(Deserialize)]
struct JobHeartbeatBody {
    provider_id: String,
    elapsed_secs: i64,
    gpu_util_pct: f32,
    ram_used_gb: u32,
}

async fn job_heartbeat(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(body): Json<JobHeartbeatBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    sqlx::query!(
        r#"
        UPDATE jobs
        SET state          = 'running',
            last_heartbeat = now()
        WHERE job_id    = $1
          AND provider_id = $2
        "#,
        job_id,
        body.provider_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Also refresh the provider's last_seen
    sqlx::query!(
        "UPDATE providers SET last_seen = now(), gpu_util_pct = $1, ram_used_gb = $2 WHERE provider_id = $3",
        body.gpu_util_pct as f64,
        body.ram_used_gb as i32,
        body.provider_id,
    )
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ═══════════════════════════════════════════════════════════════════════════
// Withdraw — queue an off-chain NMC withdrawal to an external wallet.
// Phase 1: queued for manual batch processing (no live on-chain bridge yet).
// Phase 3: will trigger NMCToken.burn() + cross-chain bridge automatically.
// ═══════════════════════════════════════════════════════════════════════════

/// POST /api/v1/withdraw
#[derive(Deserialize)]
struct WithdrawBody {
    account_id:          String,
    destination_address: String,
    amount_nmc:          f64,
    chain:               String,  // "arbitrum" | "solana"
}

async fn withdraw(
    State(state): State<AppState>,
    Json(body):   Json<WithdrawBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if body.amount_nmc <= 0.0 {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Validate destination address format
    let is_eth = body.destination_address.starts_with("0x")
        && body.destination_address.len() == 42;
    let is_sol = body.destination_address.len() >= 32
        && body.destination_address.len() <= 44
        && !body.destination_address.starts_with("0x");

    if !is_eth && !is_sol {
        return Ok(Json(serde_json::json!({
            "ok":    false,
            "error": "invalid_address",
            "message": "Provide a valid Ethereum (0x…) or Solana address",
        })));
    }

    // Check balance — use runtime query() to avoid needing a sqlx offline cache entry
    let bal: f64 = sqlx::query_scalar(
        "SELECT COALESCE(available_nmc, 0.0) FROM credit_accounts WHERE account_id = $1"
    )
    .bind(&body.account_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .unwrap_or(0.0_f64);

    if bal < body.amount_nmc {
        return Ok(Json(serde_json::json!({
            "ok":    false,
            "error": "insufficient_balance",
            "message": format!("Available: {:.4} NMC, requested: {:.4}", bal, body.amount_nmc),
        })));
    }

    // Deduct from balance — runtime query, no offline cache needed
    sqlx::query(
        "UPDATE credit_accounts SET available_nmc = available_nmc - $1, updated_at = now() WHERE account_id = $2"
    )
    .bind(body.amount_nmc)
    .bind(&body.account_id)
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Record the withdrawal transaction
    let tx_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO transactions (tx_id, account_id, tx_type, amount_nmc, reference, description) VALUES ($1, $2, 'withdrawal', $3, $4, 'Withdrawal request queued')"
    )
    .bind(&tx_id)
    .bind(&body.account_id)
    .bind(-body.amount_nmc)
    .bind(&body.destination_address)
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(
        account_id  = %body.account_id,
        amount_nmc  = body.amount_nmc,
        destination = %body.destination_address,
        chain       = %body.chain,
        tx_id       = %tx_id,
        "Withdrawal queued"
    );

    Ok(Json(serde_json::json!({
        "ok":     true,
        "tx_id":  tx_id,
        "message": "Withdrawal queued — processed within 24 hours to your on-chain address",
    })))
}

// ─── Artifact store ───────────────────────────────────────────────────────────
//
// POST /api/v1/artifacts
//   Accepts: multipart/form-data
//     Field "bundle_hash" — hex hash (dedup key)
//     File fields          — the script file(s) to pack
//   Packs files into a tar.gz in-memory and stores in the artifacts table.
//   Returns: { "url": "<public_url>/api/v1/artifacts/{hash}" }
//
// GET /api/v1/artifacts/:hash
//   Streams the bundle.tar.gz from the artifacts table.
//
// NOTE: Artifacts are stored in PostgreSQL (not the container filesystem) so
// they survive Railway redeployments.

async fn upload_artifact(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut bundle_hash = String::new();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        warn!(error = %e, "Multipart parse error");
        StatusCode::BAD_REQUEST
    })? {
        let name = field.name().unwrap_or("").to_string();
        if name == "bundle_hash" {
            bundle_hash = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?;
        } else {
            let file_name = field
                .file_name()
                .unwrap_or(&name)
                .to_string();
            let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
            files.push((file_name, data.to_vec()));
        }
    }

    if bundle_hash.is_empty() || files.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Sanitise hash — only hex digits allowed
    if !bundle_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Build tar.gz in-memory using a temp directory + tar command
    let tmp_dir = tempfile::tempdir().map_err(|e| {
        warn!(error = %e, "Failed to create tmpdir");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    for (name, data) in &files {
        let safe_name = std::path::Path::new(name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        std::fs::write(tmp_dir.path().join(safe_name), data).map_err(|e| {
            warn!(error = %e, "Failed to write tmp file");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    let bundle_path = tmp_dir.path().join("bundle.tar.gz");
    let file_args: Vec<String> = files
        .iter()
        .map(|(n, _)| {
            std::path::Path::new(n)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(n)
                .to_string()
        })
        .collect();

    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(&bundle_path)
        .args(&file_args)
        .current_dir(tmp_dir.path())
        .status()
        .map_err(|e| { warn!(error = %e, "tar spawn failed"); StatusCode::INTERNAL_SERVER_ERROR })?;

    if !status.success() {
        warn!(hash = %bundle_hash, "tar failed");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let bundle_gz = std::fs::read(&bundle_path).map_err(|e| {
        warn!(error = %e, "Failed to read tar.gz");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Upsert into artifacts table (idempotent — same hash = same content)
    sqlx::query!(
        "INSERT INTO artifacts (hash, bundle_gz) VALUES ($1, $2) ON CONFLICT (hash) DO NOTHING",
        bundle_hash,
        bundle_gz,
    )
    .execute(&state.db)
    .await
    .map_err(|e| {
        warn!(error = %e, "DB artifact insert failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let base = if state.public_url.is_empty() {
        String::new()
    } else {
        state.public_url.trim_end_matches('/').to_string()
    };

    let url = format!("{}/api/v1/artifacts/{}", base, bundle_hash);
    info!(hash = %bundle_hash, files = files.len(), size_bytes = bundle_gz.len(), "Artifact stored in DB");
    Ok(Json(serde_json::json!({ "url": url, "bundle_hash": bundle_hash })))
}

async fn download_artifact(
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let row = sqlx::query!(
        "SELECT bundle_gz FROM artifacts WHERE hash = $1",
        hash,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        warn!(error = %e, hash = %hash, "DB artifact fetch failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    let data = row.bundle_gz;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/gzip")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"bundle-{}.tar.gz\"", &hash[..8.min(hash.len())]),
        )
        .body(Body::from(data))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?)
}
