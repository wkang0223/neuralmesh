//! Reverse-auction matching engine + heartbeat watcher for job migration.
//!
//! Two background loops:
//!   1. `run_auction_loop`    — every 30s, match queued jobs to available providers
//!   2. `run_heartbeat_watcher` — every 60s, detect provider timeouts and migrate jobs
//!
//! Matching algorithm:
//!   1. Pull queued jobs ordered by creation time (FIFO)
//!   2. For each job, query available providers meeting hard constraints (RAM, price)
//!   3. Score each provider: price(60%) + trust(40%)
//!   4. Assign job to highest-scoring provider, notify via NATS if available

use anyhow::Result;
use nm_common::score_bid;
use sqlx::PgPool;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

// ── Auction loop ─────────────────────────────────────────────────────────────

/// Run the matching auction loop. Never returns under normal operation.
pub async fn run_auction_loop(db: PgPool, nats: Option<async_nats::Client>) -> Result<()> {
    let mut tick = interval(Duration::from_secs(30));
    info!("Matching engine started (30s auction windows)");

    loop {
        tick.tick().await;
        if let Err(e) = run_auction_cycle(&db, nats.as_ref()).await {
            error!(error = %e, "Auction cycle error");
        }
    }
}

async fn run_auction_cycle(db: &PgPool, nats: Option<&async_nats::Client>) -> Result<()> {
    // Fetch all queued jobs (including re-queued migrating ones)
    let queued_jobs = sqlx::query!(
        r#"
        SELECT job_id, runtime, min_ram_gb, max_price_per_hour,
               preferred_region, consumer_ssh_pubkey, consumer_wg_pubkey,
               bundle_url, bundle_hash, checkpoint_url, restore_attempts
        FROM jobs
        WHERE state = 'queued'
        ORDER BY created_at ASC
        LIMIT 50
        "#
    )
    .fetch_all(db)
    .await?;

    for job in queued_jobs {
        let job_id = job.job_id.as_str();
        let restore = job.checkpoint_url.is_some();

        match find_best_provider(
            db,
            job_id,
            job.min_ram_gb.unwrap_or(0) as u32,
            job.max_price_per_hour.unwrap_or(f64::MAX),
        )
        .await
        {
            Ok(Some(provider_id)) => {
                assign_job(
                    db,
                    nats,
                    job_id,
                    &provider_id,
                    job.max_price_per_hour.unwrap_or(0.1),
                    job.checkpoint_url.as_deref(),
                )
                .await?;

                if restore {
                    info!(
                        job_id,
                        provider_id,
                        restore_attempts = job.restore_attempts.unwrap_or(0) + 1,
                        "Job assigned to new provider for checkpoint restore"
                    );
                }
            }
            Ok(None) => {
                warn!(job_id, "No matching provider available — will retry next cycle");
            }
            Err(e) => {
                error!(job_id, error = %e, "Error finding provider for job");
            }
        }
    }

    Ok(())
}

async fn find_best_provider(
    db: &PgPool,
    job_id: &str,
    min_ram_gb: u32,
    max_price: f64,
) -> Result<Option<String>> {
    let candidates = sqlx::query!(
        r#"
        SELECT provider_id, floor_price_nmc_per_hour, trust_score, jobs_completed
        FROM providers
        WHERE state = 'available'
          AND max_job_ram_gb >= $1
          AND floor_price_nmc_per_hour <= $2
        ORDER BY floor_price_nmc_per_hour ASC
        LIMIT 20
        "#,
        min_ram_gb as i32,
        max_price,
    )
    .fetch_all(db)
    .await?;

    if candidates.is_empty() {
        return Ok(None);
    }

    let best = candidates.into_iter().max_by(|a, b| {
        let score_a = simple_score(
            a.floor_price_nmc_per_hour.unwrap_or(0.1),
            a.trust_score.unwrap_or(3.0),
            max_price,
        );
        let score_b = simple_score(
            b.floor_price_nmc_per_hour.unwrap_or(0.1),
            b.trust_score.unwrap_or(3.0),
            max_price,
        );
        score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(best.map(|r| r.provider_id))
}

fn simple_score(price: f64, trust: f64, max_price: f64) -> f64 {
    let price_norm = if max_price > 0.0 {
        1.0 - (price / max_price).min(1.0)
    } else {
        0.5
    };
    let trust_norm = trust / 5.0;
    0.6 * price_norm + 0.4 * trust_norm
}

async fn assign_job(
    db: &PgPool,
    nats: Option<&async_nats::Client>,
    job_id: &str,
    provider_id: &str,
    price_per_hour: f64,
    checkpoint_url: Option<&str>,
) -> Result<()> {
    sqlx::query!(
        r#"UPDATE jobs
           SET state = 'assigned',
               provider_id = $1,
               price_per_hour = $2,
               started_at = COALESCE(started_at, now()),
               restore_attempts = restore_attempts + CASE WHEN $3::TEXT IS NOT NULL THEN 1 ELSE 0 END
           WHERE job_id = $4"#,
        provider_id,
        price_per_hour,
        checkpoint_url as Option<&str>,
        job_id,
    )
    .execute(db)
    .await?;

    sqlx::query!(
        "UPDATE providers SET state = 'leased', active_job_id = $1 WHERE provider_id = $2",
        job_id,
        provider_id,
    )
    .execute(db)
    .await?;

    // Notify provider via NATS (optional)
    if let Some(nc) = nats {
        let payload = serde_json::json!({
            "type": "job_assigned",
            "job_id": job_id,
            "provider_id": provider_id,
            "checkpoint_url": checkpoint_url,
        })
        .to_string();
        let _ = nc
            .publish(
                format!("nm.provider.{}", provider_id),
                payload.into_bytes().into(),
            )
            .await;
    }

    info!(job_id, provider_id, price_per_hour, "Job assigned to provider");
    Ok(())
}

/// Calculate how much a provider earned for a completed job.
pub async fn calculate_credits(db: &PgPool, job_id: &str) -> Result<f64> {
    let row = sqlx::query!(
        "SELECT price_per_hour, actual_runtime_s FROM jobs WHERE job_id = $1",
        job_id,
    )
    .fetch_optional(db)
    .await?;

    if let Some(r) = row {
        let hours = r.actual_runtime_s.unwrap_or(0) as f64 / 3600.0;
        let gross = r.price_per_hour.unwrap_or(0.0) * hours;
        let fee = gross * 0.08; // 8% platform fee
        Ok(gross - fee)
    } else {
        Ok(0.0)
    }
}

// ── Heartbeat watcher ─────────────────────────────────────────────────────────

/// Provider heartbeat timeout — if a provider hasn't sent a heartbeat within
/// this many seconds while holding a running job, we consider it disconnected.
const PROVIDER_TIMEOUT_SECS: i64 = 120;

/// Maximum times we try to restore a job from checkpoint before failing it.
const MAX_RESTORE_ATTEMPTS: i32 = 3;

/// Run the heartbeat watcher. Never returns under normal operation.
/// Checks every 60 seconds for providers that have gone silent.
pub async fn run_heartbeat_watcher(db: PgPool) -> Result<()> {
    let mut tick = interval(Duration::from_secs(60));
    info!("Heartbeat watcher started — timeout threshold {}s", PROVIDER_TIMEOUT_SECS);

    loop {
        tick.tick().await;
        if let Err(e) = check_stale_providers(&db).await {
            error!(error = %e, "Heartbeat watcher error");
        }
    }
}

async fn check_stale_providers(db: &PgPool) -> Result<()> {
    // Find providers that have been in 'leased' state but haven't reported
    // a heartbeat (last_seen) in PROVIDER_TIMEOUT_SECS seconds.
    let stale = sqlx::query!(
        r#"
        SELECT provider_id, active_job_id
        FROM providers
        WHERE state = 'leased'
          AND last_seen < now() - ($1 || ' seconds')::INTERVAL
        "#,
        PROVIDER_TIMEOUT_SECS.to_string(),
    )
    .fetch_all(db)
    .await?;

    for provider in stale {
        let pid = &provider.provider_id;
        warn!(provider_id = %pid, "Provider heartbeat timeout — marking offline and migrating job");

        // Mark provider offline
        sqlx::query!(
            "UPDATE providers SET state = 'offline', active_job_id = NULL WHERE provider_id = $1",
            pid,
        )
        .execute(db)
        .await?;

        if let Some(job_id) = provider.active_job_id {
            migrate_or_fail_job(db, &job_id, pid).await?;
        }
    }

    Ok(())
}

async fn migrate_or_fail_job(db: &PgPool, job_id: &str, provider_id: &str) -> Result<()> {
    // Look up the job's current checkpoint status and restore history
    let job = sqlx::query!(
        r#"
        SELECT state, checkpoint_url, restore_attempts
        FROM jobs
        WHERE job_id = $1
        "#,
        job_id,
    )
    .fetch_optional(db)
    .await?;

    let Some(job) = job else {
        warn!(job_id, "Job not found when trying to migrate");
        return Ok(());
    };

    // Only act on jobs that are still in progress
    if !matches!(job.state.as_deref(), Some("running") | Some("assigned")) {
        return Ok(());
    }

    let restore_attempts = job.restore_attempts.unwrap_or(0);
    let has_checkpoint = job.checkpoint_url.is_some();

    if has_checkpoint && restore_attempts < MAX_RESTORE_ATTEMPTS {
        // Re-queue the job — checkpoint is available, try a new provider
        sqlx::query!(
            r#"UPDATE jobs
               SET state = 'queued',
                   provider_id = NULL,
                   failure_reason = 'provider_disconnect_checkpoint_restore'
               WHERE job_id = $1"#,
            job_id,
        )
        .execute(db)
        .await?;

        info!(
            job_id,
            provider_id,
            restore_attempts,
            "Job re-queued for checkpoint restore on new provider"
        );
    } else {
        // No checkpoint or too many restore attempts — fail the job
        let reason = if has_checkpoint {
            format!("Max restore attempts ({}) exceeded", MAX_RESTORE_ATTEMPTS)
        } else {
            "Provider disconnected with no checkpoint".to_string()
        };

        sqlx::query!(
            r#"UPDATE jobs
               SET state = 'failed',
                   completed_at = now(),
                   failure_reason = $1
               WHERE job_id = $2"#,
            reason,
            job_id,
        )
        .execute(db)
        .await?;

        // Refund consumer escrow (best-effort)
        if let Err(e) = refund_consumer_escrow(db, job_id).await {
            warn!(job_id, error = %e, "Failed to refund escrow on job failure (non-fatal)");
        }

        // Apply a small trust penalty to the vanishing provider
        let _ = sqlx::query!(
            r#"UPDATE providers
               SET trust_score = GREATEST(0.0, LEAST(5.0, trust_score - 0.20))
               WHERE provider_id = $1"#,
            provider_id,
        )
        .execute(db)
        .await;

        warn!(job_id, provider_id, reason, "Job failed due to provider disconnect");
    }

    Ok(())
}

async fn refund_consumer_escrow(db: &PgPool, job_id: &str) -> Result<()> {
    // Find the locked escrow for this job
    let escrow = sqlx::query!(
        "SELECT escrow_id, consumer_id, locked_nmc FROM escrows WHERE job_id = $1 AND state = 'locked'",
        job_id,
    )
    .fetch_optional(db)
    .await?;

    let Some(escrow) = escrow else {
        return Ok(()); // no escrow to refund
    };

    // Release escrow
    sqlx::query!(
        "UPDATE escrows SET state = 'released', settled_at = now() WHERE escrow_id = $1",
        escrow.escrow_id,
    )
    .execute(db)
    .await?;

    // Credit consumer's available balance
    sqlx::query!(
        r#"INSERT INTO credit_accounts (account_id, available_nmc)
           VALUES ($1, $2)
           ON CONFLICT (account_id) DO UPDATE
             SET available_nmc = credit_accounts.available_nmc + $2,
                 escrowed_nmc  = GREATEST(0, credit_accounts.escrowed_nmc - $2),
                 updated_at    = now()"#,
        escrow.consumer_id,
        escrow.locked_nmc,
    )
    .execute(db)
    .await?;

    // Record refund transaction
    let tx_id = Uuid::new_v4().to_string();
    sqlx::query!(
        r#"INSERT INTO transactions (tx_id, account_id, tx_type, amount_nmc, reference, description)
           VALUES ($1, $2, 'escrow_release', $3, $4, 'Provider disconnect refund')"#,
        tx_id,
        escrow.consumer_id,
        escrow.locked_nmc,
        job_id,
    )
    .execute(db)
    .await?;

    info!(
        job_id,
        consumer_id = %escrow.consumer_id,
        refund_nmc = escrow.locked_nmc,
        "Consumer escrow refunded after provider disconnect"
    );
    Ok(())
}
