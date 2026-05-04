//! `hatch provider` subcommands.

use crate::client::ClientContext;
use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::process::Command;

#[derive(Subcommand)]
pub enum ProviderCmd {
    /// Install ML runtimes and configure this Mac as a provider
    Install {
        /// Runtimes to install (comma-separated: mlx,torch-mps,onnx-coreml,llama-cpp)
        #[arg(long, default_value = "mlx,torch-mps,onnx-coreml")]
        runtimes: String,
    },
    /// Start the hatch-agent daemon
    Start,
    /// Stop the hatch-agent daemon
    Stop,
    /// Show provider status (GPU state, active jobs, earnings)
    Status,
    /// Configure provider settings
    Config {
        #[arg(long)] idle_threshold: Option<f32>,
        #[arg(long)] idle_minutes: Option<u32>,
        #[arg(long)] floor_price: Option<f64>,
        #[arg(long)] max_job_ram: Option<u32>,
    },
}

pub async fn run(cmd: ProviderCmd, ctx: &ClientContext) -> Result<()> {
    match cmd {
        ProviderCmd::Install { runtimes } => install(runtimes, ctx).await,
        ProviderCmd::Start  => start_daemon().await,
        ProviderCmd::Stop   => stop_daemon().await,
        ProviderCmd::Status => show_status(ctx).await,
        ProviderCmd::Config { idle_threshold, idle_minutes, floor_price, max_job_ram } => {
            configure(idle_threshold, idle_minutes, floor_price, max_job_ram).await
        }
    }
}

async fn install(runtimes: String, ctx: &ClientContext) -> Result<()> {
    println!("{}", "Hatch Provider Setup".bold().cyan());
    println!("Installing ML runtimes: {}", runtimes.yellow());
    println!();

    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner()
        .template("{spinner:.green} {msg}")
        .unwrap());

    // Detect chip
    pb.set_message("Detecting Apple Silicon chip...");
    let chip_out = Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]).output()?;
    let chip = String::from_utf8_lossy(&chip_out.stdout).trim().to_string();
    pb.finish_with_message(format!("Detected: {}", chip.green()));

    // Create neuralmesh_worker user
    println!("\n{}", "Creating isolated worker user...".bold());
    let user_exists = Command::new("id").arg("neuralmesh_worker").status()?.success();
    if !user_exists {
        let status = Command::new("sudo")
            .args(["dscl", ".", "-create", "/Users/neuralmesh_worker"])
            .status()?;
        if status.success() {
            Command::new("sudo")
                .args(["dscl", ".", "-create", "/Users/neuralmesh_worker", "UserShell", "/usr/bin/false"])
                .status()?;
            println!("  {} Created neuralmesh_worker user", "✓".green());
        }
    } else {
        println!("  {} neuralmesh_worker user already exists", "✓".green());
    }

    // Install each runtime
    for runtime in runtimes.split(',') {
        let runtime = runtime.trim();
        println!("\n{} {}...", "Installing".bold(), runtime.yellow());

        match runtime {
            "mlx" => {
                install_pip_package("mlx")?;
                install_pip_package("mlx-lm")?;
                println!("  {} MLX installed", "✓".green());
            }
            "torch-mps" => {
                install_pip_package("torch torchvision torchaudio")?;
                println!("  {} PyTorch (MPS) installed", "✓".green());
            }
            "onnx-coreml" => {
                install_pip_package("onnxruntime")?;
                println!("  {} ONNX Runtime (CoreML EP) installed", "✓".green());
            }
            "llama-cpp" => {
                println!("  Installing llama-cpp-python with Metal support...");
                let pip = venv_pip();
                let status = Command::new(&pip)
                    .args(["install", "llama-cpp-python"])
                    .env("CMAKE_ARGS", "-DGGML_METAL=on")
                    .env("FORCE_CMAKE", "1")
                    .status()?;
                if status.success() {
                    println!("  {} llama-cpp-python (Metal) installed", "✓".green());
                } else {
                    println!("  {} llama-cpp-python install failed — skipping", "⚠".yellow());
                }
            }
            _ => println!("  {} Unknown runtime: {} — skipping", "⚠".yellow(), runtime),
        }
    }

    // Create /tmp/neuralmesh directory
    std::fs::create_dir_all("/tmp/neuralmesh")?;
    println!("\n{} Working directory created: /tmp/neuralmesh", "✓".green());

    // ── Account registration ─────────────────────────────────────────────────
    println!("\n{}", "Setting up Hatch account...".bold());
    match ensure_account(ctx).await {
        Ok(account_id) => {
            println!("  {} Account ready: {}", "✓".green(), account_id.cyan());
        }
        Err(e) => {
            // Non-fatal — provider can still start, account can be registered later
            println!(
                "  {} Could not register account ({})\n  Run {} manually if needed.",
                "⚠".yellow(),
                e,
                "`hatch account setup`".cyan()
            );
        }
    }

    println!("\n{}", "Setup complete!".bold().green());
    println!("Run {} to start offering your GPU to the network.", "`hatch provider start`".cyan());

    Ok(())
}

/// Returns path to the pip binary inside the Hatch venv (or system pip3 fallback).
fn venv_pip() -> String {
    // Honour the env var set by the installer / launchd plist
    if let Ok(venv) = std::env::var("HATCH_VENV") {
        let p = format!("{}/bin/pip", venv);
        if std::path::Path::new(&p).exists() {
            return p;
        }
    }
    // Default venv location
    let default = format!("{}/.hatch-venv/bin/pip", std::env::var("HOME").unwrap_or_default());
    if std::path::Path::new(&default).exists() {
        return default;
    }
    "pip3".to_string()
}

fn install_pip_package(packages: &str) -> Result<()> {
    let pip = venv_pip();
    let args: Vec<&str> = std::iter::once("install")
        .chain(packages.split_whitespace())
        .collect();
    let status = Command::new(&pip).args(&args).status()?;
    if !status.success() {
        anyhow::bail!("{} install {} failed", pip, packages);
    }
    Ok(())
}

async fn start_daemon() -> Result<()> {
    let plist = "/Library/LaunchDaemons/io.hatch.agent.plist";

    println!("Starting hatch-agent...");
    let status = Command::new("sudo")
        .args(["launchctl", "load", "-w", plist])
        .status()?;

    if status.success() {
        println!("{} hatch-agent started", "✓".green());
        println!("Your Mac will start offering idle GPU time to the network.");
    } else {
        // Plist may not exist yet — give a helpful error
        if !std::path::Path::new(plist).exists() {
            anyhow::bail!(
                "LaunchDaemon plist not found at {}.\n\
                 Run the installer first:  curl -fsSL https://install.hatch.network | bash",
                plist
            );
        }
        anyhow::bail!("Failed to start hatch-agent daemon (sudo launchctl load returned error)");
    }
    Ok(())
}

async fn stop_daemon() -> Result<()> {
    Command::new("sudo")
        .args(["launchctl", "unload", "-w", "/Library/LaunchDaemons/io.hatch.agent.plist"])
        .status()?;
    println!("{} hatch-agent stopped", "✓".green());
    Ok(())
}

async fn show_status(ctx: &ClientContext) -> Result<()> {
    // Check launchd service status
    let running = Command::new("launchctl")
        .args(["list", "io.hatch.agent"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    println!("{}", "Provider Status".bold().cyan());
    println!("─────────────────────────────────────────");

    let status_str = if running { "● Running".green().to_string() } else { "○ Stopped".red().to_string() };
    println!("  Agent:        {}", status_str);

    // Try to get provider info from coordinator
    if running {
        match ctx.http().get(ctx.coordinator_url("/api/v1/stats")).send().await {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    println!("  Network:      {} available providers", json["available_providers"].as_i64().unwrap_or(0));
                    println!("  Network RAM:  {} GB available across network", json["total_available_ram_gb"].as_i64().unwrap_or(0));
                }
            }
            Err(_) => println!("  Network:      (coordinator unreachable)"),
        }
    }

    // Detect local chip
    if let Ok(chip) = Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]).output() {
        let chip_str = String::from_utf8_lossy(&chip.stdout).trim().to_string();
        println!("  Chip:         {}", chip_str.yellow());
    }

    Ok(())
}

async fn configure(
    idle_threshold: Option<f32>,
    idle_minutes: Option<u32>,
    floor_price: Option<f64>,
    max_job_ram: Option<u32>,
) -> Result<()> {
    let cfg_path = dirs::config_dir()
        .unwrap_or_default()
        .join("hatch/agent.toml");

    let content = if cfg_path.exists() {
        std::fs::read_to_string(&cfg_path)?
    } else {
        String::new()
    };

    let mut cfg: nm_common::config::AgentConfig = if content.is_empty() {
        nm_common::config::AgentConfig::default()
    } else {
        toml::from_str(&content).unwrap_or_default()
    };

    if let Some(t) = idle_threshold { cfg.idle_threshold_pct = t; }
    if let Some(m) = idle_minutes   { cfg.idle_duration_minutes = m; }
    if let Some(p) = floor_price    { cfg.floor_price_nmc_per_hour = p; }
    if let Some(r) = max_job_ram    { cfg.max_job_ram_gb = Some(r); }

    std::fs::create_dir_all(cfg_path.parent().unwrap())?;
    std::fs::write(&cfg_path, toml::to_string_pretty(&cfg)?)?;
    println!("{} Provider config updated", "✓".green());

    Ok(())
}

fn find_agent_binary() -> Result<String> {
    // Look for hatch-agent in PATH or next to the hatch CLI binary
    if let Ok(out) = Command::new("which").arg("hatch-agent").output() {
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
        }
    }
    // Try same directory as current binary
    let cur = std::env::current_exe()?;
    let sibling = cur.parent().unwrap().join("hatch-agent");
    if sibling.exists() {
        return Ok(sibling.to_string_lossy().to_string());
    }
    anyhow::bail!("hatch-agent binary not found. Reinstall Hatch.")
}

// ── Account registration ─────────────────────────────────────────────────────

/// Ensure this machine has a Hatch account.
/// Public so `account.rs` can call it directly.
/// - Loads or generates an Ed25519 keypair at ~/.config/neuralmesh/identity.key
/// - Derives a device fingerprint from the Mac serial number
/// - POSTs to the coordinator's /api/v1/account/register (idempotent)
/// - Saves the returned account_id to the CLI config file
/// Returns the account_id.
pub async fn ensure_account(ctx: &ClientContext) -> Result<String> {
    use nm_crypto::keys::NmKeypair;
    use sha2::{Digest, Sha256};

    let cfg_dir = dirs::config_dir()
        .unwrap_or_default()
        .join("hatch");
    std::fs::create_dir_all(&cfg_dir).context("create config dir")?;

    // 1. Load or generate keypair
    let key_path = cfg_dir.join("identity.key");
    let keypair = if key_path.exists() {
        NmKeypair::load_from_file(&key_path).context("Loading keypair")?
    } else {
        let kp = NmKeypair::generate();
        kp.save_to_file(&key_path).context("Saving keypair")?;
        kp
    };

    let pubkey_hex = keypair.public_key_hex();

    // 2. Derive device fingerprint from Mac serial number (or hostname fallback)
    let serial = Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .find(|l| l.contains("IOPlatformSerialNumber"))
                .and_then(|l| l.split('"').nth(3))
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| {
            hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "unknown".to_string())
        });

    let mut h = Sha256::new();
    h.update(pubkey_hex.as_bytes());
    h.update(b"||");
    h.update(serial.as_bytes());
    let fingerprint_hash = hex::encode(h.finalize()); // 64 hex chars

    // 3. Detect chip label for device_label
    let chip = Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "Apple Silicon".to_string());

    // 4. Register (idempotent)
    let resp = ctx
        .http()
        .post(ctx.coordinator_url("/api/v1/account/register"))
        .json(&serde_json::json!({
            "ecdsa_pubkey_hex":        pubkey_hex,
            "device_fingerprint_hash": fingerprint_hash,
            "device_label":            chip,
            "platform":                "macos",
        }))
        .send()
        .await
        .context("Reaching coordinator for account registration")?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("Coordinator returned error: {}", err);
    }

    let body: serde_json::Value = resp.json().await.context("Parsing registration response")?;

    if body["ok"].as_bool() != Some(true) {
        anyhow::bail!(
            "{}",
            body["message"].as_str().unwrap_or("registration failed")
        );
    }

    let account_id = body["account_id"]
        .as_str()
        .context("Missing account_id in response")?
        .to_string();

    // 5. Save account_id to CLI config
    let cli_cfg_path = cfg_dir.join("cli.toml");
    let mut cli_cfg = if cli_cfg_path.exists() {
        let raw = std::fs::read_to_string(&cli_cfg_path).unwrap_or_default();
        toml::from_str::<crate::config::CliConfig>(&raw).unwrap_or_default()
    } else {
        crate::config::CliConfig::default()
    };
    cli_cfg.account_id = Some(account_id.clone());
    std::fs::write(&cli_cfg_path, toml::to_string_pretty(&cli_cfg)?)?;

    Ok(account_id)
}
