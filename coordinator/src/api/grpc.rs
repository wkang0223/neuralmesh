//! gRPC server — handles AgentService and ConsumerService RPCs.

use super::AppState;
use anyhow::Result;
use nm_proto::agent::{
    agent_service_server::{AgentService, AgentServiceServer},
    HeartbeatResponse, JobAccepted, JobCompleteAck, ProviderHeartbeat,
    RegisterProviderRequest, RegisterProviderResponse,
};
use nm_proto::job::{
    consumer_service_server::{ConsumerService, ConsumerServiceServer},
    CancelJobRequest, CancelJobResponse, JobStatus, JobStatusRequest,
    JobSubmitRequest, JobSubmitResponse, ListJobsRequest, ListJobsResponse,
    ListProvidersRequest, ListProvidersResponse,
};
use tonic::{transport::Server, Request, Response, Status};
use tracing::{error, info, warn};
use serde_json;
use uuid::Uuid;

/// Build a tonic Router containing all gRPC services.
/// Can be merged with an axum Router via `axum::Router::merge`.
pub fn build_router(state: AppState) -> axum::Router {
    Server::builder()
        .add_service(AgentServiceServer::new(AgentGrpcService { state: state.clone() }))
        .add_service(ConsumerServiceServer::new(ConsumerGrpcService { state }))
        .into_router()
}

pub async fn serve(state: AppState, addr: String) -> Result<()> {
    let addr_parsed = addr.parse()?;
    info!(addr = %addr, "gRPC server listening");

    Server::builder()
        .add_service(AgentServiceServer::new(AgentGrpcService { state: state.clone() }))
        .add_service(ConsumerServiceServer::new(ConsumerGrpcService { state }))
        .serve(addr_parsed)
        .await?;

    Ok(())
}

// ──────────────────────────────────────────────
// AgentService implementation
// ──────────────────────────────────────────────

struct AgentGrpcService {
    state: AppState,
}

#[tonic::async_trait]
impl AgentService for AgentGrpcService {
    async fn register_provider(
        &self,
        request: Request<RegisterProviderRequest>,
    ) -> Result<Response<RegisterProviderResponse>, Status> {
        let req = request.into_inner();
        let provider_id = req.provider_id.clone();

        info!(provider_id = %provider_id, "Provider registration request");

        // Validate required fields
        let caps = req.capabilities.ok_or_else(|| {
            Status::invalid_argument("capabilities required")
        })?;

        let chip = caps.chip.ok_or_else(|| {
            Status::invalid_argument("chip info required")
        })?;

        // Store provider in DB
        match sqlx::query!(
            r#"
            INSERT INTO providers (
                provider_id, chip_model, unified_memory_gb, gpu_cores,
                metal_version, serial_number, installed_runtimes,
                max_job_ram_gb, bandwidth_mbps, region,
                floor_price_nmc_per_hour, state, nm_version
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'offline', $12)
            ON CONFLICT (provider_id) DO UPDATE SET
                chip_model = EXCLUDED.chip_model,
                unified_memory_gb = EXCLUDED.unified_memory_gb,
                gpu_cores = EXCLUDED.gpu_cores,
                installed_runtimes = EXCLUDED.installed_runtimes,
                floor_price_nmc_per_hour = EXCLUDED.floor_price_nmc_per_hour,
                state = 'offline',
                updated_at = now()
            "#,
            provider_id,
            chip.chip_model,
            chip.unified_memory_gb as i32,
            chip.gpu_cores as i32,
            chip.metal_version,
            chip.serial_number,
            &caps.installed_runtimes,
            caps.max_job_ram_gb as i32,
            caps.bandwidth_mbps as i32,
            caps.region,
            caps.floor_price_nmc_per_hour,
            req.nm_version,
        )
        .execute(&self.state.db)
        .await {
            Ok(_) => {},
            Err(e) => {
                error!(error = %e, "DB error during provider registration");
                return Err(Status::internal("Database error"));
            }
        }

        Ok(Response::new(RegisterProviderResponse {
            status: "ok".into(),
            message: "Provider registered successfully".into(),
            assigned_relay: String::new(),
        }))
    }

    async fn send_heartbeat(
        &self,
        request: Request<ProviderHeartbeat>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let hb = request.into_inner();

        sqlx::query!(
            r#"
            UPDATE providers
            SET state = $1, gpu_util_pct = $2, ram_used_gb = $3,
                active_job_id = NULLIF($4, ''), last_seen = now()
            WHERE provider_id = $5
            "#,
            hb.state,
            hb.gpu_util_pct as f64,
            hb.ram_used_gb as i32,
            hb.active_job_id,
            hb.provider_id,
        )
        .execute(&self.state.db)
        .await
        .map_err(|e| Status::internal(format!("DB: {}", e)))?;

        Ok(Response::new(HeartbeatResponse {
            ok: true,
            message: String::new(),
        }))
    }

    async fn acknowledge_job(
        &self,
        request: Request<JobAccepted>,
    ) -> Result<Response<JobCompleteAck>, Status> {
        let ja = request.into_inner();
        info!(job_id = %ja.job_id, "Job acknowledged by provider");

        sqlx::query!(
            "UPDATE jobs SET state = 'running', wireguard_endpoint = $1, ssh_port = $2 WHERE job_id = $3",
            ja.wireguard_endpoint,
            ja.ssh_port as i32,
            ja.job_id,
        )
        .execute(&self.state.db)
        .await
        .map_err(|e| Status::internal(format!("DB: {}", e)))?;

        Ok(Response::new(JobCompleteAck {
            ok: true,
            credits_earned: 0.0,
            message: "Job running".into(),
        }))
    }

    async fn report_completion(
        &self,
        request: Request<nm_proto::agent::JobComplete>,
    ) -> Result<Response<JobCompleteAck>, Status> {
        let jc = request.into_inner();
        info!(job_id = %jc.job_id, exit_code = jc.exit_code, "Job completion reported");

        // Update job state
        let job_state = if jc.exit_code == 0 { "complete" } else { "failed" };
        sqlx::query!(
            "UPDATE jobs SET state = $1, output_hash = $2, actual_runtime_s = $3, completed_at = now() WHERE job_id = $4",
            job_state,
            jc.output_hash,
            jc.actual_runtime_s as i64,
            jc.job_id,
        )
        .execute(&self.state.db)
        .await
        .map_err(|e| Status::internal(format!("DB: {}", e)))?;

        // ── GPU hour milestone rewards ───────────────────────────────────────
        if job_state == "complete" && !jc.provider_id.is_empty() {
            let hours = jc.actual_runtime_s as f64 / 3600.0;

            // Update provider's total GPU hours contributed
            let updated = sqlx::query!(
                r#"
                UPDATE providers
                SET total_gpu_hours_contributed = COALESCE(total_gpu_hours_contributed, 0.0) + $1
                WHERE provider_id = $2
                RETURNING total_gpu_hours_contributed
                "#,
                hours,
                jc.provider_id,
            )
            .fetch_optional(&self.state.db)
            .await
            .ok()
            .flatten();

            if let Some(row) = updated {
                let total_hours = row.total_gpu_hours_contributed.unwrap_or(0.0);
                let milestones_earned = (total_hours / 8.0) as i32;

                // Count already-claimed milestones
                let claimed = sqlx::query_scalar!(
                    "SELECT COUNT(*) FROM milestone_rewards WHERE provider_id = $1",
                    jc.provider_id,
                )
                .fetch_one(&self.state.db)
                .await
                .unwrap_or(Some(0))
                .unwrap_or(0) as i32;

                // Issue rewards for each unclaimed milestone
                for milestone_num in (claimed + 1)..=(milestones_earned) {
                    let milestone_hours = milestone_num * 8;
                    let reward_hc = 50.0_f64;
                    let account_id = jc.provider_id.clone();

                    let ledger_url = std::env::var("NM_LEDGER_URL")
                        .unwrap_or_else(|_| "http://localhost:8082".into());

                    let client = reqwest::Client::new();
                    let tx_id = match client
                        .post(format!("{}/api/v1/wallet/{}/deposit", ledger_url, account_id))
                        .json(&serde_json::json!({
                            "amount_nmc": reward_hc,
                            "reference": format!("milestone_{}h", milestone_hours),
                        }))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            resp.json::<serde_json::Value>().await
                                .ok()
                                .and_then(|v| v["transaction_id"].as_str().map(String::from))
                        }
                        Ok(resp) => {
                            warn!(
                                status = %resp.status(),
                                milestone_hours,
                                "Ledger reward call failed for milestone"
                            );
                            None
                        }
                        Err(e) => {
                            warn!(error = %e, "Could not reach ledger for milestone reward");
                            None
                        }
                    };

                    // Record milestone — ON CONFLICT DO NOTHING prevents double-paying
                    let _ = sqlx::query!(
                        r#"
                        INSERT INTO milestone_rewards
                            (provider_id, milestone_hours, reward_hc, account_id, tx_id)
                        VALUES ($1, $2, $3, $4, $5)
                        ON CONFLICT (provider_id, milestone_hours) DO NOTHING
                        "#,
                        jc.provider_id,
                        milestone_hours,
                        reward_hc,
                        account_id,
                        tx_id,
                    )
                    .execute(&self.state.db)
                    .await;

                    info!(
                        provider_id = %jc.provider_id,
                        milestone_hours,
                        reward_hc,
                        "GPU hour milestone reward issued"
                    );
                }
            }
        }

        // TODO: trigger escrow release in ledger
        let credits = crate::matching::calculate_credits(&self.state.db, &jc.job_id).await
            .unwrap_or(0.0);

        Ok(Response::new(JobCompleteAck {
            ok: true,
            credits_earned: credits,
            message: format!("Job {}: {} — earned {:.4} NMC", jc.job_id, job_state, credits),
        }))
    }
}

// ──────────────────────────────────────────────
// ConsumerService implementation
// ──────────────────────────────────────────────

struct ConsumerGrpcService {
    state: AppState,
}

#[tonic::async_trait]
impl ConsumerService for ConsumerGrpcService {
    async fn submit_job(
        &self,
        request: Request<JobSubmitRequest>,
    ) -> Result<Response<JobSubmitResponse>, Status> {
        let req = request.into_inner();
        let reqs = req.requirements.ok_or_else(|| Status::invalid_argument("requirements required"))?;

        let job_id = Uuid::new_v4().to_string();
        info!(job_id = %job_id, consumer_id = %req.consumer_id, "Job submitted");

        // Insert job into DB
        sqlx::query!(
            r#"
            INSERT INTO jobs (
                job_id, consumer_id, runtime, min_ram_gb, max_duration_s,
                max_price_per_hour, bundle_hash, bundle_url,
                consumer_ssh_pubkey, consumer_wg_pubkey, state
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'queued')
            "#,
            job_id,
            req.consumer_id,
            reqs.runtime as i32,
            reqs.min_ram_gb as i32,
            reqs.max_duration_s as i32,
            reqs.max_price_per_h,
            req.bundle_hash,
            req.bundle_upload_url,
            req.consumer_ssh_pubkey,
            req.consumer_wg_pubkey,
        )
        .execute(&self.state.db)
        .await
        .map_err(|e| Status::internal(format!("DB: {}", e)))?;

        // Publish to NATS for matching engine (optional — DB polling picks it up if absent)
        if let Some(ref nc) = self.state.nats {
            let payload = serde_json::json!({ "job_id": job_id }).to_string();
            let _ = nc.publish("nm.jobs.new", payload.into_bytes().into()).await;
        }

        Ok(Response::new(JobSubmitResponse {
            job_id,
            state: 1, // QUEUED
            message: "Job queued for matching".into(),
        }))
    }

    async fn get_job_status(
        &self,
        request: Request<JobStatusRequest>,
    ) -> Result<Response<JobStatus>, Status> {
        let req = request.into_inner();

        let row = sqlx::query!(
            r#"
            SELECT job_id, state, provider_id, price_per_hour,
                   EXTRACT(EPOCH FROM (now() - started_at))::bigint AS elapsed_s,
                   wireguard_endpoint, ssh_port
            FROM jobs WHERE job_id = $1 AND consumer_id = $2
            "#,
            req.job_id, req.consumer_id,
        )
        .fetch_optional(&self.state.db)
        .await
        .map_err(|e| Status::internal(format!("DB: {}", e)))?
        .ok_or_else(|| Status::not_found("Job not found"))?;

        Ok(Response::new(JobStatus {
            job_id: row.job_id,
            state: 0, // map from string
            provider_id: row.provider_id.unwrap_or_default(),
            provider_chip: String::new(),
            provider_ram_gb: 0,
            price_per_hour: row.price_per_hour.unwrap_or(0.0),
            elapsed_s: row.elapsed_s.unwrap_or(0) as u64,
            gpu_util_pct: 0.0,
            ram_used_gb: 0,
            cost_so_far_nmc: 0.0,
            wireguard_endpoint: row.wireguard_endpoint.unwrap_or_default(),
            ssh_port: row.ssh_port.unwrap_or(2222) as u32,
            started_at: None,
            estimated_end: None,
        }))
    }

    async fn list_jobs(
        &self,
        request: Request<ListJobsRequest>,
    ) -> Result<Response<ListJobsResponse>, Status> {
        Ok(Response::new(ListJobsResponse { jobs: vec![], total: 0 }))
    }

    async fn cancel_job(
        &self,
        request: Request<CancelJobRequest>,
    ) -> Result<Response<CancelJobResponse>, Status> {
        let req = request.into_inner();
        sqlx::query!(
            "UPDATE jobs SET state = 'cancelled' WHERE job_id = $1 AND consumer_id = $2",
            req.job_id, req.consumer_id,
        )
        .execute(&self.state.db)
        .await
        .map_err(|e| Status::internal(format!("DB: {}", e)))?;

        Ok(Response::new(CancelJobResponse { ok: true, message: "Cancelled".into() }))
    }

    async fn list_providers(
        &self,
        request: Request<ListProvidersRequest>,
    ) -> Result<Response<ListProvidersResponse>, Status> {
        let req = request.into_inner();

        let rows = sqlx::query!(
            r#"
            SELECT provider_id, chip_model, unified_memory_gb, gpu_cores,
                   metal_version, installed_runtimes, max_job_ram_gb,
                   floor_price_nmc_per_hour, region, trust_score,
                   jobs_completed, success_rate
            FROM providers
            WHERE state = 'available'
              AND max_job_ram_gb >= $1
              AND ($2 = '' OR region = $2)
              AND (floor_price_nmc_per_hour <= $3 OR $3 = 0)
            ORDER BY floor_price_nmc_per_hour ASC
            LIMIT $4 OFFSET $5
            "#,
            req.min_ram_gb as i32,
            req.region,
            req.max_price,
            req.limit.max(20) as i64,
            req.offset as i64,
        )
        .fetch_all(&self.state.db)
        .await
        .map_err(|e| Status::internal(format!("DB: {}", e)))?;

        let providers: Vec<nm_proto::job::ProviderListing> = rows.into_iter().map(|r| {
            nm_proto::job::ProviderListing {
                provider_id:    r.provider_id,
                chip_model:     r.chip_model.unwrap_or_default(),
                unified_ram_gb: r.unified_memory_gb.unwrap_or(0) as u32,
                gpu_cores:      r.gpu_cores.unwrap_or(0) as u32,
                metal_version:  r.metal_version.unwrap_or_default(),
                runtimes:       r.installed_runtimes.unwrap_or_default(),
                max_job_ram_gb: r.max_job_ram_gb.unwrap_or(0) as u32,
                price_per_hour: r.floor_price_nmc_per_hour.unwrap_or(0.0),
                region:         r.region.unwrap_or_default(),
                trust_score:    r.trust_score.unwrap_or(3.0) as f32,
                jobs_completed: r.jobs_completed.unwrap_or(0) as u32,
                success_rate:   r.success_rate.unwrap_or(1.0) as f32,
                latency_ms:     0,
            }
        }).collect();

        Ok(Response::new(ListProvidersResponse {
            total: providers.len() as u32,
            providers,
        }))
    }
}
