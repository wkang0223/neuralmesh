use serde::{Deserialize, Serialize};

/// Agent configuration (stored at ~/.config/neuralmesh/agent.toml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub provider_id: Option<String>,          // Set after first registration
    pub idle_threshold_pct: f32,              // Default: 5.0
    pub idle_duration_minutes: u32,           // Default: 10
    pub floor_price_nmc_per_hour: f64,        // Default: 0.05
    pub max_job_ram_gb: Option<u32>,          // Default: total_ram - 4GB
    pub allowed_runtimes: Vec<String>,        // Default: all installed
    pub coordinator_endpoints: Vec<String>,   // Bootstrap coordinator nodes
    pub region: String,                       // e.g. "us-west-2"
    pub wireguard_listen_port: u16,           // Default: 51820
    pub ssh_job_port: u16,                    // Default: 2222
    pub log_level: String,                    // Default: "info"
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider_id: None,
            idle_threshold_pct: 5.0,
            idle_duration_minutes: 10,
            floor_price_nmc_per_hour: 0.05,
            max_job_ram_gb: None,
            allowed_runtimes: vec![
                "mlx".into(),
                "torch-mps".into(),
                "onnx-coreml".into(),
                "llama-cpp".into(),
            ],
            coordinator_endpoints: vec![
                "https://coord1.neuralmesh.io:8080".into(),
                "https://coord2.neuralmesh.io:8080".into(),
                "https://coord3.neuralmesh.io:8080".into(),
            ],
            region: detect_region(),
            wireguard_listen_port: 51820,
            ssh_job_port: 2222,
            log_level: "info".into(),
        }
    }
}

fn detect_region() -> String {
    // Basic timezone-to-region heuristic; can be improved
    std::env::var("NM_REGION").unwrap_or_else(|_| "us-east-1".into())
}

/// Coordinator configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorConfig {
    pub grpc_listen_addr: String,    // Default: "0.0.0.0:9090"
    pub rest_listen_addr: String,    // Default: "0.0.0.0:8080"
    pub database_url: String,        // PostgreSQL connection string
    pub redis_url: String,
    pub nats_url: String,
    pub platform_fee_pct: f64,       // Default: 8.0  (provider receives 92%)
    pub heartbeat_timeout_secs: u64, // Default: 90
    pub auction_window_secs: u64,    // Default: 30
    pub bootstrap_peers: Vec<String>,
}
