//! REST API for the dashboard and external integrations.

use super::AppState;
use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tower_http::cors::CorsLayer;
use tracing::info;
use uuid::Uuid;

pub async fn serve(state: AppState, addr: String) -> Result<()> {
    let app = Router::new()
        .route("/health",                    get(health))
        .route("/api/v1/providers",          get(list_providers))
        .route("/api/v1/providers/:id",      get(get_provider))
        .route("/api/v1/jobs",               get(list_jobs_rest))
        .route("/api/v1/stats",              get(network_stats))
        // ── Device-locked account endpoints ──────────────────────────────
        .route("/api/v1/account/register",      post(register_account))
        .route("/api/v1/account/:id",           get(get_account))
        .route("/api/v1/account/:id/verify",    post(verify_device))
        .route("/api/v1/account/:id/reregister", post(reregister_device))
        // ── Provider job lifecycle ────────────────────────────────────────
        .route("/api/v1/provider/:id/job",      get(get_assigned_job))
        .route("/api/v1/jobs/:id/complete",     post(complete_job))
        .route("/api/v1/jobs/:id/fail",         post(fail_job))
        .route("/api/v1/jobs/:id/checkpoint",   post(save_checkpoint))
        .route("/api/v1/jobs/:id/heartbeat",    post(job_heartbeat))
        // ── Ledger (Stripe webhook credits) ──────────────────────────────
        .route("/api/v1/ledger/stripe-credit",  post(stripe_credit))
        .layer(CorsLayer::permissive())
        .with_state(state);

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

async fn list_jobs_rest(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let rows = sqlx::query!(
        "SELECT job_id, consumer_id, state, runtime, created_at FROM jobs ORDER BY created_at DESC LIMIT 50"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let jobs: Vec<_> = rows.into_iter().map(|r| serde_json::json!({
        "job_id":      r.job_id,
        "consumer_id": r.consumer_id,
        "state":       r.state,
        "runtime":     r.runtime,
        "created_at":  r.created_at.map(|t| t.to_string()),
    })).collect();

    Ok(Json(serde_json::json!({ "jobs": jobs })))
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
               max_price_per_hour, bundle_hash, bundle_url,
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

    info!(
        job_id = %job_id,
        provider_id = %body.provider_id,
        gross_nmc,
        provider_nmc,
        platform_nmc,
        "Job completed — credits transferred"
    );

    Ok(Json(serde_json::json!({
        "ok": true,
        "gross_nmc":    gross_nmc,
        "provider_nmc": provider_nmc,
        "platform_nmc": platform_nmc,
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
