//! `hatch job` subcommands.

use crate::client::ClientContext;
use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum JobCmd {
    /// Submit a compute job to the network
    Submit {
        /// Script or file to run
        script: PathBuf,

        /// ML runtime to use (mlx, torch-mps, onnx-coreml, llama-cpp, shell)
        #[arg(long, default_value = "mlx")]
        runtime: String,

        /// Minimum unified memory required (GB)
        #[arg(long, default_value = "16")]
        ram: u32,

        /// Maximum job duration (hours)
        #[arg(long, default_value = "1")]
        hours: f64,

        /// Maximum price willing to pay (HC/hr)
        #[arg(long, default_value = "0.50")]
        max_price: f64,

        /// Additional files to include (comma-separated paths)
        #[arg(long)]
        include: Option<String>,

        /// Wait for job to complete before returning
        #[arg(long)]
        wait: bool,
    },
    /// List your jobs
    List {
        /// Filter by status (queued, running, complete, failed)
        #[arg(long)]
        status: Option<String>,

        /// Show last N jobs
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Stream logs from a running job
    Logs {
        /// Job ID
        job_id: String,

        /// Follow log output (poll continuously)
        #[arg(short, long)]
        follow: bool,
    },
    /// Cancel a job
    Cancel {
        /// Job ID
        job_id: String,
    },
}

// ─── API response types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JobResponse {
    id: String,
    state: String,
    runtime: String,
    min_ram_gb: u32,
    max_price_per_hour: f64,
    created_at: String,
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    completed_at: Option<String>,
    #[serde(default)]
    exit_code: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct JobListResponse {
    jobs: Vec<JobResponse>,
    total: u32,
}

#[derive(Debug, Serialize)]
struct JobSubmitRequest {
    account_id: String,
    runtime: String,
    min_ram_gb: u32,
    max_duration_secs: u64,
    max_price_per_hour: f64,
    bundle_hash: String,
    bundle_url: String,
    script_name: String,
}

#[derive(Debug, Deserialize)]
struct JobSubmitResponse {
    job_id: String,
    state: String,
    estimated_wait_secs: u64,
}

#[derive(Debug, Deserialize)]
struct LogsResponse {
    job_id: String,
    output: String,
    #[serde(default)]
    is_complete: bool,
}

// ─── Command handlers ────────────────────────────────────────────────────────

pub async fn run(cmd: JobCmd, ctx: &ClientContext) -> Result<()> {
    match cmd {
        JobCmd::Submit { script, runtime, ram, hours, max_price, include, wait } => {
            submit(ctx, script, runtime, ram, hours, max_price, include, wait).await
        }
        JobCmd::List { status, limit } => list_jobs(ctx, status, limit).await,
        JobCmd::Logs { job_id, follow } => stream_logs(ctx, &job_id, follow).await,
        JobCmd::Cancel { job_id } => cancel_job(ctx, &job_id).await,
    }
}

async fn submit(
    ctx: &ClientContext,
    script: PathBuf,
    runtime: String,
    ram: u32,
    hours: f64,
    max_price: f64,
    include: Option<String>,
    wait: bool,
) -> Result<()> {
    let account_id = ctx.require_account_id()?.to_string();

    // Validate script exists
    if !script.exists() {
        anyhow::bail!("Script not found: {}", script.display());
    }

    // Validate runtime
    let valid_runtimes = ["mlx", "torch-mps", "onnx-coreml", "llama-cpp", "shell"];
    if !valid_runtimes.contains(&runtime.as_str()) {
        anyhow::bail!(
            "Unknown runtime '{}'. Valid options: {}",
            runtime,
            valid_runtimes.join(", ")
        );
    }

    println!("{}", "Packaging job bundle...".bold());

    // Build list of files to pack
    let mut files: Vec<PathBuf> = vec![script.clone()];
    if let Some(extras) = include {
        for path in extras.split(',') {
            let p = PathBuf::from(path.trim());
            if p.exists() {
                files.push(p);
            } else {
                eprintln!("  {} Warning: included file not found: {}", "⚠".yellow(), path.trim());
            }
        }
    }

    // Compute bundle hash from all files
    let bundle_hash = hash_files(&files)?;
    let script_name = script
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("job.py")
        .to_string();

    println!("  Script:   {}", script.display().to_string().yellow());
    println!("  Runtime:  {}", runtime.cyan());
    println!("  RAM:      {} GB minimum", ram);
    println!("  Duration: {} hours max", hours);
    println!("  Price:    ≤ {} HC/hr", max_price);
    println!("  Bundle:   {}", &bundle_hash[..12]);

    // Upload bundle to coordinator artifact store
    println!("\n{}", "Uploading bundle...".bold());
    let bundle_url = upload_bundle(ctx, &files, &bundle_hash).await?;
    println!("  {} Bundle uploaded", "✓".green());

    // Submit job to coordinator
    let req = JobSubmitRequest {
        account_id,
        runtime: runtime.clone(),
        min_ram_gb: ram,
        max_duration_secs: (hours * 3600.0) as u64,
        max_price_per_hour: max_price,
        bundle_hash: bundle_hash.clone(),
        bundle_url,
        script_name,
    };

    let resp = ctx
        .http()
        .post(ctx.coordinator_url("/api/v1/jobs"))
        .json(&req)
        .send()
        .await
        .context("Failed to submit job")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        // Try to extract a human-friendly message from JSON error response
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["message"].as_str().map(|s| s.to_string()))
            .unwrap_or(body);
        anyhow::bail!("Job submission failed (HTTP {}): {}", status.as_u16(), msg);
    }

    let result: JobSubmitResponse = resp.json().await.context("Invalid submit response")?;

    println!("\n{} Job submitted!", "✓".green());
    println!("  Job ID:   {}", result.job_id.cyan());
    println!("  State:    {}", result.state.yellow());
    if result.estimated_wait_secs > 0 {
        println!("  Est. wait: ~{}s (matching providers)", result.estimated_wait_secs);
    }

    if wait {
        println!("\nWaiting for job to complete...");
        wait_for_completion(ctx, &result.job_id).await?;
    } else {
        println!("\nTrack progress:  {} logs {}", "hatch job".cyan(), result.job_id.cyan());
        println!("Cancel:          {} cancel {}", "hatch job".cyan(), result.job_id.cyan());
    }

    Ok(())
}

async fn list_jobs(ctx: &ClientContext, status: Option<String>, limit: u32) -> Result<()> {
    let account_id = ctx.require_account_id()?;

    let mut url = ctx.coordinator_url(&format!("/api/v1/jobs?account_id={}&limit={}", account_id, limit));
    if let Some(s) = &status {
        url.push_str(&format!("&state={}", s));
    }

    let resp = ctx
        .http()
        .get(&url)
        .send()
        .await
        .context("Failed to list jobs")?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to list jobs: {}", err);
    }

    let result: JobListResponse = resp.json().await.context("Invalid list response")?;

    if result.jobs.is_empty() {
        println!("No jobs found.");
        return Ok(());
    }

    if ctx.output_json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "jobs": result.jobs.iter().map(|j| serde_json::json!({
                "id": j.id, "state": j.state, "runtime": j.runtime,
                "ram_gb": j.min_ram_gb, "created_at": j.created_at,
            })).collect::<Vec<_>>(),
            "total": result.total,
        }))?);
        return Ok(());
    }

    println!("{}", "Your Jobs".bold().cyan());
    println!("─────────────────────────────────────────────────────────────────");
    println!("{:<38} {:<12} {:<12} {:<8} {}",
        "JOB ID", "STATE", "RUNTIME", "RAM GB", "CREATED");
    println!("─────────────────────────────────────────────────────────────────");

    for job in &result.jobs {
        let state_colored = match job.state.as_str() {
            "running"  => job.state.green().to_string(),
            "complete" => job.state.blue().to_string(),
            "failed"   => job.state.red().to_string(),
            "queued" | "matching" => job.state.yellow().to_string(),
            _          => job.state.normal().to_string(),
        };
        println!("{:<38} {:<21} {:<12} {:<8} {}",
            job.id.cyan(),
            state_colored,
            job.runtime,
            job.min_ram_gb,
            &job.created_at[..19],
        );
    }

    if result.total > limit {
        println!("\n  Showing {} of {} jobs. Use --limit to see more.", limit, result.total);
    }

    Ok(())
}

async fn stream_logs(ctx: &ClientContext, job_id: &str, follow: bool) -> Result<()> {
    let url = ctx.coordinator_url(&format!("/api/v1/jobs/{}/logs", job_id));

    if follow {
        println!("Streaming logs for job {} (Ctrl+C to stop)...", job_id.cyan());
        println!("─────────────────────────────────────────");
        let mut offset = 0usize;
        loop {
            let resp = ctx
                .http()
                .get(&format!("{}?offset={}", url, offset))
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    if let Ok(logs) = r.json::<LogsResponse>().await {
                        if !logs.output.is_empty() {
                            print!("{}", logs.output);
                            offset += logs.output.len();
                        }
                        if logs.is_complete {
                            println!("\n─────────────────────────────────────────");
                            println!("{} Job complete", "✓".green());
                            break;
                        }
                    }
                }
                Ok(r) if r.status().as_u16() == 404 => {
                    anyhow::bail!("Job not found: {}", job_id);
                }
                _ => {}
            }

            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    } else {
        let resp = ctx
            .http()
            .get(&url)
            .send()
            .await
            .context("Failed to fetch logs")?;

        if resp.status().as_u16() == 404 {
            anyhow::bail!("Job not found: {}", job_id);
        }
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("Failed to fetch logs: {}", err);
        }

        let logs: LogsResponse = resp.json().await.context("Invalid logs response")?;
        if logs.output.is_empty() {
            println!("(no output yet)");
        } else {
            print!("{}", logs.output);
        }
    }

    Ok(())
}

async fn cancel_job(ctx: &ClientContext, job_id: &str) -> Result<()> {
    let resp = ctx
        .http()
        .delete(ctx.coordinator_url(&format!("/api/v1/jobs/{}", job_id)))
        .send()
        .await
        .context("Failed to cancel job")?;

    if resp.status().as_u16() == 404 {
        anyhow::bail!("Job not found: {}", job_id);
    }
    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to cancel job: {}", err);
    }

    println!("{} Job {} cancelled", "✓".green(), job_id.cyan());
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hash_files(files: &[PathBuf]) -> Result<String> {
    use std::collections::BTreeMap;
    // Deterministic hash: sort by filename, SHA-256 each, combine
    let mut map = BTreeMap::new();
    for path in files {
        let content = std::fs::read(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        // Simple deterministic hash using std (no sha2 dep in CLI yet — use file sizes + XOR for Phase 1)
        let mut h: u64 = 0xcbf29ce484222325; // FNV offset basis
        for byte in &content {
            h ^= *byte as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        map.insert(path.file_name().unwrap().to_string_lossy().to_string(), h);
    }
    // Combine all per-file hashes
    let mut combined: u64 = 0;
    for (_, h) in &map {
        combined ^= h;
        combined = combined.wrapping_mul(0x100000001b3);
    }
    Ok(format!("{:016x}{:016x}", combined, combined.wrapping_mul(0xdeadbeef)))
}

async fn upload_bundle(ctx: &ClientContext, files: &[PathBuf], bundle_hash: &str) -> Result<String> {
    // Build multipart form with all files
    let mut form = reqwest::multipart::Form::new()
        .text("bundle_hash", bundle_hash.to_string());

    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let content = std::fs::read(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let part = reqwest::multipart::Part::bytes(content).file_name(name.clone());
        form = form.part(name, part);
    }

    let resp = ctx
        .http()
        .post(ctx.coordinator_url("/api/v1/artifacts"))
        .multipart(form)
        .send()
        .await
        .context("Failed to upload bundle")?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("Bundle upload failed: {}", err);
    }

    #[derive(Deserialize)]
    struct UploadResp { url: String }
    let r: UploadResp = resp.json().await.context("Invalid upload response")?;
    Ok(r.url)
}

async fn wait_for_completion(ctx: &ClientContext, job_id: &str) -> Result<()> {
    let url = ctx.coordinator_url(&format!("/api/v1/jobs/{}", job_id));
    let terminal_states = ["complete", "failed", "cancelled"];

    loop {
        let resp = ctx.http().get(&url).send().await;
        if let Ok(r) = resp {
            if let Ok(job) = r.json::<JobResponse>().await {
                let state = job.state.as_str();
                if terminal_states.contains(&state) {
                    println!("\n{} Job {}: {}",
                        if state == "complete" { "✓".green() } else { "✗".red() },
                        job_id.cyan(),
                        match state {
                            "complete" => state.green().to_string(),
                            _ => state.red().to_string(),
                        }
                    );
                    if let Some(code) = job.exit_code {
                        println!("  Exit code: {}", code);
                    }
                    return Ok(());
                }
                print!("\r  State: {:<15}", job.state.yellow());
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
