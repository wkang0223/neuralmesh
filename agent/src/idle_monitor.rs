//! Main event loop for the agent: idle detection + job polling + execution.

use crate::coordinator_client::CoordinatorClient;
use crate::job_runner::{JobResult, JobRunner};
use anyhow::Result;
use nm_common::{config::AgentConfig, MacChipInfo, Runtime};
use nm_crypto::NmKeypair;
use nm_macos::idle::{IdleDetector, IdleState};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info, warn};

pub struct IdleMonitor {
    threshold_pct: f32,
    cool_down_minutes: u32,
    chip: MacChipInfo,
    keypair: NmKeypair,
    coordinator: CoordinatorClient,
    cfg: AgentConfig,
    runtimes: Vec<String>,
    start_time: Instant,
}

impl IdleMonitor {
    pub fn new(
        threshold_pct: f32,
        cool_down_minutes: u32,
        chip: MacChipInfo,
        keypair: NmKeypair,
        coordinator: CoordinatorClient,
        cfg: AgentConfig,
        runtimes: Vec<String>,
    ) -> Self {
        Self {
            threshold_pct,
            cool_down_minutes,
            chip,
            keypair,
            coordinator,
            cfg,
            runtimes,
            start_time: Instant::now(),
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut detector = IdleDetector::new(self.threshold_pct, self.cool_down_minutes);
        let mut current_state = IdleState::Busy;
        let heartbeat_interval = Duration::from_secs(30);
        let poll_interval      = Duration::from_secs(30);
        let mut last_heartbeat = Instant::now();

        info!("Idle monitor started — polling every {}s", poll_interval.as_secs());

        loop {
            // ── Idle state machine ────────────────────────────────────────
            if let Some(new_state) = detector.poll() {
                info!(state = ?new_state, "Idle state changed");
                current_state = new_state.clone();

                match &current_state {
                    IdleState::Available => {
                        if let Err(e) = self.announce_available().await {
                            warn!(error = %e, "Failed to announce availability");
                        }
                    }
                    IdleState::Busy => {
                        let _ = self.coordinator.heartbeat(
                            "busy", 0.0, 0, None,
                            self.start_time.elapsed().as_secs()
                        ).await;
                    }
                    _ => {}
                }
            }

            // ── Poll for assigned job (only when available) ───────────────
            if matches!(current_state, IdleState::Available) {
                match self.poll_for_job().await {
                    Ok(Some(job)) => {
                        info!(job_id = %job.job_id, "Job accepted — starting execution");
                        current_state = IdleState::Leased;

                        // Run the job in a separate task so heartbeats continue
                        let job_clone  = job.clone();
                        let coord      = self.coordinator.clone();
                        let provider_id = self.coordinator.provider_id().to_string();
                        let python_prefix = best_python_prefix();

                        tokio::spawn(async move {
                            let rest_base   = coord.rest_base();
                            let job_id_str  = job_clone.job_id.to_string();

                            // Heartbeat task — keeps coordinator informed while job runs.
                            let hb_coord  = coord.clone();
                            let hb_pid    = provider_id.clone();
                            let hb_job_id = job_id_str.clone();
                            let heartbeat_task = tokio::spawn(async move {
                                let mut tick = tokio::time::interval(
                                    tokio::time::Duration::from_secs(30)
                                );
                                let start = std::time::Instant::now();
                                loop {
                                    tick.tick().await;
                                    let base = hb_coord.rest_base();
                                    let url = format!("{}/api/v1/jobs/{}/heartbeat", base, hb_job_id);
                                    let gpu = nm_macos::gpu_detect::sample_gpu_utilization();
                                    let body = serde_json::json!({
                                        "provider_id": hb_pid,
                                        "elapsed_secs": start.elapsed().as_secs(),
                                        "gpu_util_pct": gpu.as_ref().map(|g| g.utilization_pct).unwrap_or(0.0),
                                        "ram_used_gb":  gpu.as_ref().map(|g| g.memory_used_mb / 1024).unwrap_or(0),
                                    });
                                    let _ = reqwest::Client::new()
                                        .post(&url)
                                        .json(&body)
                                        .send()
                                        .await;
                                }
                            });

                            match JobRunner::run(&job_clone, &python_prefix, &rest_base, &provider_id).await {
                                Ok(result) => {
                                    heartbeat_task.abort();
                                    // Push accumulated stdout/stderr to coordinator before reporting.
                                    push_final_log(&rest_base, &result.job_id.to_string()).await;
                                    report_completion(&coord, &provider_id, result).await;
                                }
                                Err(e) => {
                                    heartbeat_task.abort();
                                    error!(error = %e, "Job execution failed");
                                    push_final_log(&rest_base, &job_id_str).await;
                                    report_failure(
                                        &coord,
                                        &provider_id,
                                        &job_id_str,
                                        &format!("{}", e),
                                        true, // provider fault
                                    ).await;
                                }
                            }
                        });
                    }
                    Ok(None) => {} // No job yet
                    Err(e) => warn!(error = %e, "Job poll failed"),
                }
            }

            // ── Heartbeat ─────────────────────────────────────────────────
            if last_heartbeat.elapsed() >= heartbeat_interval {
                let state_str = state_to_str(&current_state);
                let gpu_stats = nm_macos::gpu_detect::sample_gpu_utilization();
                let gpu_util  = gpu_stats.as_ref().map(|s| s.utilization_pct).unwrap_or(0.0);
                let ram_used  = gpu_stats.as_ref()
                    .map(|s| (s.memory_used_mb / 1024) as u32)
                    .unwrap_or(0);

                if let Err(e) = self.coordinator.heartbeat(
                    state_str, gpu_util, ram_used, None,
                    self.start_time.elapsed().as_secs(),
                ).await {
                    warn!(error = %e, "Heartbeat failed");
                }
                last_heartbeat = Instant::now();
            }

            sleep(poll_interval).await;
        }
    }

    /// Download checkpoint metadata from the coordinator so the job runner
    /// can restore the DMTCP snapshot on this provider.
    async fn fetch_checkpoint(&self, job_id: &str, checkpoint_url: &str) -> Result<()> {
        use crate::checkpoint::{CheckpointMeta, CHECKPOINT_DIR};

        // Download the checkpoint metadata JSON
        let meta_url = format!("{}/checkpoint.json", checkpoint_url.trim_end_matches('/'));
        let resp = reqwest::get(&meta_url).await?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to fetch checkpoint metadata: HTTP {}", resp.status());
        }

        let meta: CheckpointMeta = resp.json().await?;
        info!(
            job_id,
            iteration = meta.iteration,
            files = meta.dmtcp_files.len(),
            "Checkpoint metadata fetched"
        );

        // Create local checkpoint directory
        let ckpt_dir = std::path::PathBuf::from(CHECKPOINT_DIR).join(job_id);
        std::fs::create_dir_all(&ckpt_dir)?;

        // Download each .dmtcp file
        for fname in &meta.dmtcp_files {
            let file_url = format!("{}/{}", checkpoint_url.trim_end_matches('/'), fname);
            let file_resp = reqwest::get(&file_url).await?;
            if !file_resp.status().is_success() {
                warn!(job_id, file = %fname, "Could not download checkpoint file — skipping");
                continue;
            }
            let bytes = file_resp.bytes().await?;
            let dest = ckpt_dir.join(fname);
            std::fs::write(&dest, &bytes)?;
            info!(job_id, file = %fname, bytes = bytes.len(), "Checkpoint file downloaded");
        }

        // Save the metadata locally so DmtcpSession::restore() can find it
        meta.save()?;
        info!(job_id, "Checkpoint fully fetched and ready for restore");
        Ok(())
    }

    async fn announce_available(&self) -> Result<()> {
        self.coordinator.heartbeat(
            "available", 0.0, 0, None,
            self.start_time.elapsed().as_secs(),
        ).await?;
        info!("Announced AVAILABLE to coordinator");
        Ok(())
    }

    /// Poll the coordinator REST API for an assigned job.
    async fn poll_for_job(&self) -> Result<Option<JobSpec>> {
        let provider_id = self.coordinator.provider_id();
        let base = self.coordinator.rest_base();
        let url  = format!("{}/api/v1/provider/{}/job", base, provider_id);

        let resp = reqwest::get(&url).await?;
        if !resp.status().is_success() {
            return Ok(None);
        }

        let body: serde_json::Value = resp.json().await?;
        if body["job"].is_null() {
            return Ok(None);
        }

        let job = body["job"].clone();
        let runtime_str = job["runtime"].as_str().unwrap_or("shell");
        let runtime = nm_common::Runtime::from_str(runtime_str)
            .unwrap_or(Runtime::Shell);

        let checkpoint_url = job["checkpoint_url"].as_str().map(|s| s.to_string());
        let restore_attempts = job["restore_attempts"].as_u64().unwrap_or(0) as u32;

        // If this is a checkpoint-restore job, try to download the checkpoint first
        if let Some(ref ckpt_url) = checkpoint_url {
            if let Err(e) = self.fetch_checkpoint(
                job["job_id"].as_str().unwrap_or_default(),
                ckpt_url,
            ).await {
                warn!(
                    job_id = job["job_id"].as_str().unwrap_or("?"),
                    error = %e,
                    "Could not fetch checkpoint — will attempt fresh start"
                );
            }
        }

        Ok(Some(JobSpec {
            job_id: uuid::Uuid::parse_str(
                job["job_id"].as_str().unwrap_or_default()
            ).unwrap_or_default(),
            consumer_id: job["consumer_id"].as_str().unwrap_or_default().to_string(),
            runtime,
            min_ram_gb: job["min_ram_gb"].as_i64().unwrap_or(8) as u32,
            max_duration_secs: job["max_duration_s"].as_i64().unwrap_or(3600) as u32,
            max_price_per_hour: job["max_price_per_hour"].as_f64().unwrap_or(0.1),
            bundle_hash: job["bundle_hash"].as_str().unwrap_or_default().to_string(),
            bundle_url: job["bundle_url"].as_str().unwrap_or_default().to_string(),
            consumer_ssh_pubkey: job["consumer_ssh_pubkey"].as_str().unwrap_or_default().to_string(),
            consumer_wg_pubkey: job["consumer_wg_pubkey"].as_str().unwrap_or_default().to_string(),
            preferred_region: job["preferred_region"].as_str().map(|s| s.to_string()),
            env_vars: std::collections::HashMap::new(),
            checkpoint_url,
            restore_attempts,
        }))
    }
}

// ── Job spec (flattened from protobuf for REST polling) ───────────────────

#[derive(Debug, Clone)]
pub struct JobSpec {
    pub job_id: uuid::Uuid,
    pub consumer_id: String,
    pub runtime: nm_common::Runtime,
    pub min_ram_gb: u32,
    pub max_duration_secs: u32,
    pub max_price_per_hour: f64,
    pub bundle_hash: String,
    pub bundle_url: String,
    pub consumer_ssh_pubkey: String,
    pub consumer_wg_pubkey: String,
    pub preferred_region: Option<String>,
    pub env_vars: std::collections::HashMap<String, String>,
    /// If set, this is a checkpoint-restore job — skip bundle download and
    /// restore from the DMTCP checkpoint at this URL.
    pub checkpoint_url: Option<String>,
    /// Checkpoint iteration — how many times this job has been migrated.
    pub restore_attempts: u32,
}

// ── Completion reporting ──────────────────────────────────────────────────

async fn report_completion(coord: &CoordinatorClient, provider_id: &str, result: JobResult) {
    let base = coord.rest_base();
    let url  = format!("{}/api/v1/jobs/{}/complete", base, result.job_id);

    let body = serde_json::json!({
        "provider_id":      provider_id,
        "exit_code":        result.exit_code,
        "output_hash":      result.output_hash,
        "actual_runtime_s": result.actual_runtime_s,
        "avg_gpu_util_pct": result.avg_gpu_util_pct,
        "peak_ram_gb":      result.peak_ram_gb,
    });

    match reqwest::Client::new().post(&url).json(&body).send().await {
        Ok(r) if r.status().is_success() => {
            info!(job_id = %result.job_id, "Completion reported, credits credited");
        }
        Ok(r) => error!(job_id = %result.job_id, status = %r.status(), "Completion report rejected"),
        Err(e) => error!(job_id = %result.job_id, error = %e, "Completion report failed"),
    }
}

async fn report_failure(
    coord: &CoordinatorClient,
    provider_id: &str,
    job_id: &str,
    reason: &str,
    slash: bool,
) {
    let base = coord.rest_base();
    let url  = format!("{}/api/v1/jobs/{}/fail", base, job_id);
    let body = serde_json::json!({
        "provider_id": provider_id,
        "reason":      reason,
        "slash":       slash,
    });
    let _ = reqwest::Client::new().post(&url).json(&body).send().await;
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn state_to_str(state: &IdleState) -> &'static str {
    match state {
        IdleState::Busy        => "busy",
        IdleState::CoolingDown => "cooling_down",
        IdleState::Available   => "available",
        IdleState::Leased      => "leased",
        IdleState::Paused      => "paused",
    }
}

/// Find the best Python interpreter that has ML packages installed.
///
/// Search order:
///   1. `$HATCH_VENV/bin/python` — set by installer in launchd plist; MLX
///      lives here on Homebrew-managed (PEP 668) macOS 14+ systems.
///   2. System pythons (python3.12 … python3) — legacy / developer installs.
fn best_python_prefix() -> String {
    // 1. Installer venv takes priority.
    if let Ok(venv) = std::env::var("HATCH_VENV") {
        let py = format!("{}/bin/python", venv);
        if std::process::Command::new(&py)
            .args(["-c", "import mlx"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            if let Ok(out) = std::process::Command::new(&py)
                .args(["-c", "import sys; print(sys.prefix)"])
                .output()
            {
                return String::from_utf8_lossy(&out.stdout).trim().to_string();
            }
        }
    }

    // 2. Fall back to system pythons.
    for py in &["python3.12", "python3.13", "python3.11", "python3"] {
        if std::process::Command::new(py)
            .args(["-c", "import mlx"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            if let Ok(out) = std::process::Command::new(py)
                .args(["-c", "import sys; print(sys.prefix)"])
                .output()
            {
                return String::from_utf8_lossy(&out.stdout).trim().to_string();
            }
        }
    }
    "/opt/homebrew".to_string()
}

/// Read the job's stdout/stderr log from disk and POST it to the coordinator
/// so consumers can retrieve it via GET /api/v1/jobs/:id/logs.
/// Called once after job completion (success or failure).
async fn push_final_log(rest_base: &str, job_id: &str) {
    let log_path = format!("/tmp/neuralmesh/{}/nm-output.log", job_id);
    let output = match std::fs::read_to_string(&log_path) {
        Ok(s) if !s.is_empty() => s,
        _ => return, // nothing to push
    };

    let url  = format!("{}/api/v1/jobs/{}/logs", rest_base, job_id);
    let body = serde_json::json!({ "output": output });

    match reqwest::Client::new().post(&url).json(&body).send().await {
        Ok(r) if r.status().is_success() => {
            info!(job_id, bytes = output.len(), "Job log pushed to coordinator");
        }
        Ok(r) => {
            warn!(job_id, status = %r.status(), "Coordinator rejected job log push");
        }
        Err(e) => {
            warn!(job_id, error = %e, "Failed to push job log to coordinator");
        }
    }
}
