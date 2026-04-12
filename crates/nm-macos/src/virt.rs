//! macOS Virtualization.framework integration for job isolation.
//!
//! Each job runs in a lightweight Linux ARM64 VM created via Apple's
//! Virtualization.framework. This gives true OS-level isolation:
//!   - Separate kernel, PID namespace, filesystem
//!   - Rosetta 2 translation for x86_64 workloads (optional)
//!   - virtio-gpu for Metal passthrough (macOS 14+)
//!   - Shared directory mount: /tmp/neuralmesh/<job_id>/ → /job in VM
//!
//! Implementation strategy:
//!   The Rust agent calls a thin Swift helper binary (`nm-vm-helper`) that
//!   uses the Virtualization.framework Objective-C/Swift API directly.
//!   This avoids unsafe FFI into Objective-C from Rust while keeping the
//!   agent fully in Rust.
//!
//! VM lifecycle:
//!   1. create  → allocate VM config, attach disk, set CPU/RAM limits
//!   2. start   → boot Linux, wait for virtio-serial ready signal
//!   3. exec    → send job script via virtio-serial console
//!   4. monitor → read stdout/stderr, sample GPU via powermetrics
//!   5. stop    → send SIGTERM to init, wait for shutdown, delete disk

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tracing::info;

/// Configuration for a single job VM.
#[derive(Debug, Clone)]
pub struct VmConfig {
    pub job_id: String,
    pub cpu_count: u32,
    pub memory_gb: u32,
    /// Directory on the host to share with the VM as /job
    pub job_dir: PathBuf,
    /// Path to base Linux disk image (read-only, copy-on-write overlay per job)
    pub base_image: PathBuf,
    /// Path to nm-vm-helper binary
    pub helper_bin: PathBuf,
}

/// Result from running a job in a VM.
#[derive(Debug)]
pub struct VmJobResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub runtime_secs: u64,
}

impl VmConfig {
    /// Default config for a job given chip RAM available.
    pub fn for_job(job_id: &str, job_dir: PathBuf, ram_limit_gb: u32) -> Self {
        Self {
            job_id: job_id.to_string(),
            cpu_count: num_cpus::get().min(8) as u32,
            memory_gb: ram_limit_gb.min(16).max(2),
            job_dir,
            base_image: PathBuf::from("/var/neuralmesh/base-linux-arm64.img"),
            helper_bin: PathBuf::from("/usr/local/bin/nm-vm-helper"),
        }
    }

    /// Check if Virtualization.framework is available (macOS 12+).
    pub fn is_available() -> bool {
        // Check macOS version >= 12.0
        let ver = Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();

        let parts: Vec<u32> = ver.trim().split('.')
            .filter_map(|s| s.parse().ok())
            .collect();

        // Need macOS 12+
        if parts.first().copied().unwrap_or(0) < 12 {
            return false;
        }

        // Check helper binary exists
        Path::new("/usr/local/bin/nm-vm-helper").exists()
    }
}

/// Run a job inside a Virtualization.framework Linux VM.
/// Falls back to sandbox-exec if VF is unavailable.
pub async fn run_in_vm(cfg: &VmConfig, entry_script: &str) -> Result<VmJobResult> {
    if !VmConfig::is_available() {
        anyhow::bail!("Virtualization.framework not available (need macOS 12+ and nm-vm-helper)");
    }

    let start = std::time::Instant::now();

    info!(
        job_id = %cfg.job_id,
        cpu = cfg.cpu_count,
        ram_gb = cfg.memory_gb,
        "Starting VM for job"
    );

    // nm-vm-helper create <job_id> <cpu> <ram_gb> <job_dir> <base_image>
    let create_status = Command::new(&cfg.helper_bin)
        .args([
            "create",
            &cfg.job_id,
            &cfg.cpu_count.to_string(),
            &cfg.memory_gb.to_string(),
            cfg.job_dir.to_str().unwrap(),
            cfg.base_image.to_str().unwrap(),
        ])
        .status()
        .context("nm-vm-helper create")?;

    if !create_status.success() {
        anyhow::bail!("VM creation failed for job {}", cfg.job_id);
    }

    // nm-vm-helper run <job_id> <entry_script>
    let output = Command::new(&cfg.helper_bin)
        .args(["run", &cfg.job_id, entry_script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("nm-vm-helper run")?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let runtime_secs = start.elapsed().as_secs();

    // nm-vm-helper destroy <job_id>
    let _ = Command::new(&cfg.helper_bin)
        .args(["destroy", &cfg.job_id])
        .status();

    info!(
        job_id = %cfg.job_id,
        exit_code,
        runtime_secs,
        "VM job finished"
    );

    Ok(VmJobResult { exit_code, stdout, stderr, runtime_secs })
}

/// Install the nm-vm-helper Swift binary from the NeuralMesh bundle.
/// Called once during `nm provider install`.
pub fn install_vm_helper() -> Result<()> {
    let helper_path = Path::new("/usr/local/bin/nm-vm-helper");
    if helper_path.exists() {
        info!("nm-vm-helper already installed");
        return Ok(());
    }

    // Download pre-built binary from release artifacts
    let url = "https://releases.neuralmesh.io/tools/nm-vm-helper-arm64";
    info!("Downloading nm-vm-helper from {}", url);

    let status = Command::new("curl")
        .args(["-fsSL", "-o", "/tmp/nm-vm-helper", url])
        .status()
        .context("Download nm-vm-helper")?;

    if !status.success() {
        anyhow::bail!("Failed to download nm-vm-helper");
    }

    Command::new("chmod").args(["+x", "/tmp/nm-vm-helper"]).status()?;
    Command::new("sudo")
        .args(["mv", "/tmp/nm-vm-helper", "/usr/local/bin/nm-vm-helper"])
        .status()
        .context("Install nm-vm-helper")?;

    info!("nm-vm-helper installed at /usr/local/bin/nm-vm-helper");
    Ok(())
}

/// Download the base Linux ARM64 disk image for VMs.
/// This is a minimal Ubuntu 24.04 image with Python 3.12 + MLX pre-installed.
pub fn ensure_base_image() -> Result<PathBuf> {
    let img_path = PathBuf::from("/var/neuralmesh/base-linux-arm64.img");

    if img_path.exists() {
        info!("Base Linux image already present");
        return Ok(img_path);
    }

    std::fs::create_dir_all("/var/neuralmesh")?;

    let url = "https://releases.neuralmesh.io/images/neuralmesh-base-arm64-v1.img.gz";
    info!("Downloading base Linux image (this may take a few minutes)…");

    let status = Command::new("curl")
        .args([
            "-fsSL", "--progress-bar",
            "-o", "/var/neuralmesh/base-linux-arm64.img.gz",
            url,
        ])
        .status()
        .context("Download base image")?;

    if !status.success() {
        anyhow::bail!("Failed to download base Linux image");
    }

    // Decompress
    let status = Command::new("gunzip")
        .arg("/var/neuralmesh/base-linux-arm64.img.gz")
        .status()
        .context("Decompress base image")?;

    if !status.success() {
        anyhow::bail!("Failed to decompress base Linux image");
    }

    info!("Base Linux image ready at {:?}", img_path);
    Ok(img_path)
}
