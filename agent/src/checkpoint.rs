//! DMTCP (Distributed MultiThreaded CheckPointing) integration.
//!
//! When DMTCP is available (Linux/VM environment), jobs are wrapped with
//! `dmtcp_launch` for binary-level checkpointing. On macOS sandbox-exec path,
//! DMTCP is not supported — the coordinator's heartbeat watcher handles
//! re-queuing on provider disconnect instead.
//!
//! Architecture:
//!   Provider A running job
//!     → crash/disconnect detected by coordinator heartbeat watcher
//!     → job re-queued with checkpoint_url (if DMTCP was running)
//!     → Provider B picks up re-queued job
//!     → restore from checkpoint via dmtcp_restart
//!     → job continues from last checkpoint

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tracing::{info, warn};

/// Directory for checkpoint files and metadata.
pub const CHECKPOINT_DIR: &str = "/var/neuralmesh/checkpoints";

/// Checkpoint every 5 minutes.
pub const CHECKPOINT_INTERVAL_SECS: u64 = 300;

/// Returns true if `dmtcp_launch` is available in PATH.
/// On macOS this returns false — DMTCP is Linux-only.
pub fn is_dmtcp_available() -> bool {
    Command::new("which")
        .arg("dmtcp_launch")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Metadata persisted alongside each checkpoint snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMeta {
    pub job_id: String,
    /// Monotonically increasing checkpoint iteration (1, 2, 3, …)
    pub iteration: u32,
    /// Absolute path to the directory holding .dmtcp files
    pub checkpoint_dir: String,
    /// Elapsed job runtime at checkpoint time, in seconds
    pub elapsed_secs: u64,
    /// DMTCP coordinator port — needed by dmtcp_restart on the restore side
    pub coord_port: u16,
    /// File names of .dmtcp checkpoint files in this snapshot
    pub dmtcp_files: Vec<String>,
}

impl CheckpointMeta {
    fn meta_path(job_id: &str) -> PathBuf {
        PathBuf::from(CHECKPOINT_DIR)
            .join(job_id)
            .join("checkpoint.json")
    }

    /// Persist metadata to disk so it survives restarts.
    pub fn save(&self) -> Result<()> {
        let path = Self::meta_path(&self.job_id);
        std::fs::create_dir_all(path.parent().unwrap())?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json).context("Writing checkpoint metadata")?;
        Ok(())
    }

    /// Load checkpoint metadata for `job_id`. Returns error if none exists.
    pub fn load(job_id: &str) -> Result<Self> {
        let path = Self::meta_path(job_id);
        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("Reading checkpoint metadata for job {}", job_id))?;
        serde_json::from_str(&json).context("Parsing checkpoint metadata")
    }

    /// Collect .dmtcp file names from the checkpoint directory.
    fn collect_dmtcp_files(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "dmtcp").unwrap_or(false))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect()
    }
}

/// A live DMTCP-managed job session.
///
/// Use [`DmtcpSession::launch`] to start, then [`DmtcpSession::checkpoint`]
/// to trigger a snapshot, and finally [`DmtcpSession::restore`] on a new
/// provider to continue from where it left off.
pub struct DmtcpSession {
    pub job_id: String,
    pub checkpoint_dir: PathBuf,
    /// DMTCP coordinator port. Each job gets a unique port to avoid conflicts.
    pub coord_port: u16,
    pub iteration: u32,
    started_at: std::time::Instant,
}

impl DmtcpSession {
    /// Launch a new DMTCP session wrapping `cmd args`.
    ///
    /// Returns `(session_handle, child_process)`. The caller is responsible
    /// for awaiting the child and aborting the checkpoint ticker.
    pub fn launch(
        job_id: &str,
        cmd: &str,
        args: &[&str],
        work_dir: &Path,
        env_vars: &[(String, String)],
    ) -> Result<(Self, Child)> {
        // Assign a deterministic-ish port from the job ID's hash to avoid conflicts
        let port_hash = job_id.bytes().fold(0u16, |acc, b| acc.wrapping_add(b as u16));
        let coord_port = 7780 + (port_hash % 200);

        let ckpt_dir = PathBuf::from(CHECKPOINT_DIR).join(job_id);
        std::fs::create_dir_all(&ckpt_dir)?;

        let mut cmd_builder = Command::new("dmtcp_launch");
        cmd_builder
            .args([
                "--new-coordinator",
                "--coord-port", &coord_port.to_string(),
                "--ckptdir", ckpt_dir.to_str().unwrap(),
                "--no-gzip",         // faster I/O, trade space for speed
                "--interval", "0",   // disable auto-interval; we trigger manually
                "--",
                cmd,
            ])
            .args(args)
            .current_dir(work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("NM_JOB_ID", job_id)
            .env("DMTCP_CKPT_DIR", ckpt_dir.to_str().unwrap());

        for (k, v) in env_vars {
            cmd_builder.env(k, v);
        }

        let child = cmd_builder.spawn().context("dmtcp_launch spawn")?;
        let pid = child.id();

        info!(
            job_id,
            coord_port,
            pid,
            ckpt_dir = %ckpt_dir.display(),
            "DMTCP session launched"
        );

        Ok((
            Self {
                job_id: job_id.to_string(),
                checkpoint_dir: ckpt_dir,
                coord_port,
                iteration: 0,
                started_at: std::time::Instant::now(),
            },
            child,
        ))
    }

    /// Trigger an immediate checkpoint and save metadata.
    ///
    /// This issues `dmtcp_command --checkpoint` to the DMTCP coordinator.
    /// Blocks until the checkpoint is written to disk.
    pub fn checkpoint(&mut self) -> Result<()> {
        self.iteration += 1;
        info!(
            job_id = %self.job_id,
            iteration = self.iteration,
            "Triggering DMTCP checkpoint"
        );

        let status = Command::new("dmtcp_command")
            .args([
                "--coord-port", &self.coord_port.to_string(),
                "--checkpoint",
                "--wait-for-success",
            ])
            .status()
            .context("dmtcp_command --checkpoint")?;

        if !status.success() {
            anyhow::bail!("dmtcp_command --checkpoint returned non-zero");
        }

        let dmtcp_files = CheckpointMeta::collect_dmtcp_files(&self.checkpoint_dir);
        let meta = CheckpointMeta {
            job_id:          self.job_id.clone(),
            iteration:       self.iteration,
            checkpoint_dir:  self.checkpoint_dir.to_string_lossy().to_string(),
            elapsed_secs:    self.started_at.elapsed().as_secs(),
            coord_port:      self.coord_port,
            dmtcp_files,
        };
        meta.save()?;

        info!(
            job_id = %self.job_id,
            iteration = self.iteration,
            files = meta.dmtcp_files.len(),
            "Checkpoint saved"
        );
        Ok(())
    }

    /// Restore a previously checkpointed job on this (or another) provider.
    ///
    /// Finds .dmtcp files from the checkpoint metadata and launches
    /// `dmtcp_restart` to resume the process state.
    pub fn restore(meta: &CheckpointMeta, work_dir: &Path) -> Result<Child> {
        let ckpt_dir = PathBuf::from(&meta.checkpoint_dir);

        if meta.dmtcp_files.is_empty() {
            anyhow::bail!(
                "No .dmtcp files recorded in checkpoint metadata for job {}",
                meta.job_id
            );
        }

        let file_paths: Vec<PathBuf> = meta
            .dmtcp_files
            .iter()
            .map(|f| ckpt_dir.join(f))
            .collect();

        // Verify at least one file exists
        let found: Vec<_> = file_paths.iter().filter(|p| p.exists()).collect();
        if found.is_empty() {
            anyhow::bail!(
                "Checkpoint files not found on disk for job {}. \
                 Expected them in {}",
                meta.job_id,
                ckpt_dir.display()
            );
        }

        info!(
            job_id = %meta.job_id,
            iteration = meta.iteration,
            files = found.len(),
            elapsed_secs = meta.elapsed_secs,
            "Restoring from DMTCP checkpoint"
        );

        let mut cmd = Command::new("dmtcp_restart");
        cmd.arg("--new-coordinator")
            .args(["--coord-port", &meta.coord_port.to_string()])
            .current_dir(work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for f in &found {
            cmd.arg(f);
        }

        let child = cmd.spawn().context("dmtcp_restart spawn")?;
        info!(
            job_id = %meta.job_id,
            pid = child.id(),
            "DMTCP restore process started"
        );
        Ok(child)
    }

    /// Remove checkpoint files for `job_id` once the job is fully complete.
    pub fn cleanup(job_id: &str) {
        let dir = PathBuf::from(CHECKPOINT_DIR).join(job_id);
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                warn!(job_id, error = %e, "Failed to clean checkpoint dir");
            } else {
                info!(job_id, "Checkpoint directory removed");
            }
        }
    }
}

/// Spawn a background task that triggers a DMTCP checkpoint every
/// `interval_secs` seconds. Returns a `JoinHandle` — call `abort()` when
/// the job completes normally.
pub fn spawn_checkpoint_ticker(
    job_id: String,
    coord_port: u16,
    checkpoint_dir: PathBuf,
    interval_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
        tick.tick().await; // skip the immediate first tick
        let mut iteration: u32 = 0;
        let started_at = std::time::Instant::now();

        loop {
            tick.tick().await;
            iteration += 1;
            info!(job_id, iteration, "Triggering periodic DMTCP checkpoint");

            let port_str = coord_port.to_string();
            let status = tokio::task::spawn_blocking(move || {
                Command::new("dmtcp_command")
                    .args(["--coord-port", &port_str, "--checkpoint", "--wait-for-success"])
                    .status()
            })
            .await;

            match status {
                Ok(Ok(s)) if s.success() => {
                    // Gather .dmtcp files
                    let dmtcp_files = CheckpointMeta::collect_dmtcp_files(&checkpoint_dir);
                    let meta = CheckpointMeta {
                        job_id:         job_id.clone(),
                        iteration,
                        checkpoint_dir: checkpoint_dir.to_string_lossy().to_string(),
                        elapsed_secs:   started_at.elapsed().as_secs(),
                        coord_port,
                        dmtcp_files,
                    };
                    if let Err(e) = meta.save() {
                        warn!(job_id, error = %e, "Could not save checkpoint metadata");
                    } else {
                        info!(job_id, iteration, "Periodic checkpoint saved");
                    }
                }
                other => {
                    warn!(
                        job_id,
                        iteration,
                        error = ?other,
                        "Checkpoint command failed — will retry next interval"
                    );
                }
            }
        }
    })
}

/// Report the latest checkpoint URL to the coordinator so it can re-queue
/// the job with restore capability if this provider disconnects.
pub async fn report_checkpoint_to_coordinator(
    rest_base: &str,
    provider_id: &str,
    job_id: &str,
    meta: &CheckpointMeta,
) -> Result<()> {
    let url = format!("{}/api/v1/jobs/{}/checkpoint", rest_base, job_id);
    let body = serde_json::json!({
        "provider_id":    provider_id,
        "iteration":      meta.iteration,
        "elapsed_secs":   meta.elapsed_secs,
        "checkpoint_dir": meta.checkpoint_dir,
        "dmtcp_files":    meta.dmtcp_files,
    });

    reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Reporting checkpoint to coordinator")?;

    info!(job_id, iteration = meta.iteration, "Checkpoint reported to coordinator");
    Ok(())
}
