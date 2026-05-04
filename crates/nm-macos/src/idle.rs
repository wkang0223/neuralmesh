//! Idle detection for macOS — combines GPU util, screen lock state, and CPU util.

use crate::gpu_detect::sample_gpu_utilization;
use std::process::Command;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, info};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdleState {
    /// Machine is actively being used — DO NOT offer GPU
    Busy,
    /// GPU util just dropped below threshold — waiting for cool-down
    CoolingDown,
    /// GPU has been idle for the configured duration — safe to offer
    Available,
    /// A NeuralMesh job is running
    Leased,
    /// Provider manually paused offering
    Paused,
}

pub struct IdleDetector {
    /// GPU utilization % below which we consider the machine idle
    threshold_pct: f32,
    /// How long GPU must stay below threshold before declaring Available
    cool_down: Duration,
    state: IdleState,
    cool_down_since: Option<Instant>,
    poll_interval: Duration,
}

impl IdleDetector {
    pub fn new(threshold_pct: f32, cool_down_minutes: u32) -> Self {
        Self {
            threshold_pct,
            cool_down: Duration::from_secs(cool_down_minutes as u64 * 60),
            state: IdleState::Busy,
            cool_down_since: None,
            poll_interval: Duration::from_secs(30),
        }
    }

    pub fn current_state(&self) -> &IdleState {
        &self.state
    }

    /// Set to Leased when a job is accepted. Call release_lease() when done.
    pub fn set_leased(&mut self) {
        self.state = IdleState::Leased;
        self.cool_down_since = None;
    }

    /// Call when job completes or provider reclaims machine.
    pub fn release_lease(&mut self) {
        self.state = IdleState::Busy; // Re-enter busy; cool-down will follow
    }

    pub fn pause(&mut self) {
        self.state = IdleState::Paused;
    }

    pub fn resume(&mut self) {
        if self.state == IdleState::Paused {
            self.state = IdleState::Busy;
        }
    }

    /// Run a single poll cycle — returns Some(new_state) if state changed.
    pub fn poll(&mut self) -> Option<IdleState> {
        if self.state == IdleState::Leased || self.state == IdleState::Paused {
            return None; // Don't change state while leased or paused
        }

        let gpu_idle    = self.is_gpu_idle();
        // NM_FORCE_AVAILABLE=1 skips screen-lock check (dev/testing only)
        let force       = std::env::var("NM_FORCE_AVAILABLE").map(|v| v == "1").unwrap_or(false);
        let screen_locked = force || is_screen_locked();
        let user_idle   = gpu_idle || force;  // force bypasses screen+GPU check

        debug!(
            gpu_idle,
            screen_locked,
            state = ?self.state,
            "IdleDetector poll"
        );

        let old = self.state.clone();

        match &self.state {
            IdleState::Busy | IdleState::Available => {
                if user_idle {
                    match &self.state {
                        IdleState::Busy => {
                            self.state = IdleState::CoolingDown;
                            self.cool_down_since = Some(Instant::now());
                        }
                        IdleState::Available => {} // Stay available
                        _ => {}
                    }
                } else {
                    self.state = IdleState::Busy;
                    self.cool_down_since = None;
                }
            }
            IdleState::CoolingDown => {
                if !user_idle {
                    // User came back — reset
                    self.state = IdleState::Busy;
                    self.cool_down_since = None;
                } else if let Some(since) = self.cool_down_since {
                    if since.elapsed() >= self.cool_down {
                        info!("GPU idle threshold met — provider now Available");
                        self.state = IdleState::Available;
                        self.cool_down_since = None;
                    }
                }
            }
            IdleState::Leased | IdleState::Paused => {}
        }

        if self.state != old {
            Some(self.state.clone())
        } else {
            None
        }
    }

    /// Run poll loop — sends state changes over the returned channel.
    pub async fn run(mut self) -> tokio::sync::mpsc::Receiver<IdleState> {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            loop {
                if let Some(new_state) = self.poll() {
                    if tx.send(new_state).await.is_err() {
                        break; // Receiver dropped
                    }
                }
                sleep(self.poll_interval).await;
            }
        });
        rx
    }

    fn is_gpu_idle(&self) -> bool {
        sample_gpu_utilization()
            .map(|s| s.utilization_pct < self.threshold_pct)
            .unwrap_or(true) // If we can't read GPU util, assume idle
    }
}

/// Check if the macOS screen saver / lock screen is active via CGSession.
/// Returns true if the display is locked (user not actively using the machine).
pub fn is_screen_locked() -> bool {
    // `CGSessionCopyCurrentDictionary` requires Objective-C — use the simpler
    // `ioreg` approach: read IdleTime from IOHIDSystem to detect user inactivity.
    let idle_secs = user_idle_seconds().unwrap_or(0);

    // Also check screensaver / lock state via `defaults`
    let screensaver_active = Command::new("python3")
        .args(["-c", "import subprocess; r=subprocess.run(['osascript','-e',\
            'tell application \"System Events\" to get name of processes'], \
            capture_output=True, text=True); print(\"ScreenSaverEngine\" in r.stdout)"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "True")
        .unwrap_or(false);

    // Consider locked if screen saver is running OR user idle > 5 minutes
    screensaver_active || idle_secs > 300
}

/// Returns seconds since last user HID event (keyboard/mouse).
pub fn user_idle_seconds() -> Option<u64> {
    let out = Command::new("ioreg")
        .args(["-c", "IOHIDSystem"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if line.contains("HIDIdleTime") {
            // Value is in nanoseconds
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() == 2 {
                let ns: u64 = parts[1].trim().parse().ok()?;
                return Some(ns / 1_000_000_000);
            }
        }
    }
    None
}
