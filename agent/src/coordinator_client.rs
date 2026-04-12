//! gRPC client for communicating with the NeuralMesh coordinator.

use anyhow::{Context, Result};
use nm_common::{config::AgentConfig, MacChipInfo};
use nm_crypto::NmKeypair;
use nm_proto::agent::agent_service_client::AgentServiceClient;
use nm_proto::agent::{
    MacChipInfo as ProtoMacChipInfo,
    ProviderCapabilities, RegisterProviderRequest,
    ProviderHeartbeat, HeartbeatResponse,
    JobComplete, JobCompleteAck,
    JobAccepted,
};
use std::time::Duration;
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, info, warn};

#[derive(Clone)]
pub struct CoordinatorClient {
    /// Active gRPC channel (connected to one coordinator)
    channel: Channel,
    provider_id: String,
    /// gRPC endpoint string e.g. "http://localhost:9090"
    endpoint_str: String,
}

impl CoordinatorClient {
    /// Connect to the first available coordinator from the list.
    pub async fn connect(endpoints: &[String]) -> Result<Self> {
        for endpoint_str in endpoints {
            match Self::try_connect(endpoint_str).await {
                Ok(client) => return Ok(client),
                Err(e) => warn!(endpoint = endpoint_str, error = %e, "Coordinator unreachable, trying next"),
            }
        }
        anyhow::bail!("No coordinator reachable from list: {:?}", endpoints)
    }

    async fn try_connect(endpoint_str: &str) -> Result<Self> {
        let endpoint = Endpoint::from_shared(endpoint_str.to_string())
            .context("Invalid coordinator endpoint")?
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .keep_alive_while_idle(true);

        let channel = endpoint.connect().await
            .with_context(|| format!("Connecting to {}", endpoint_str))?;

        info!(endpoint = endpoint_str, "Connected to coordinator");
        Ok(Self { channel, provider_id: String::new(), endpoint_str: endpoint_str.to_string() })
    }

    fn client(&self) -> AgentServiceClient<Channel> {
        AgentServiceClient::new(self.channel.clone())
    }

    /// Provider ID assigned after registration.
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// Derive REST base URL from gRPC endpoint (swap port 9090 → 8080).
    pub fn rest_base(&self) -> String {
        self.endpoint_str
            .replace(":9090", ":8080")
            .replace("localhost:9090", "localhost:8080")
    }

    /// Register this provider with the coordinator.
    pub async fn register(
        &self,
        keypair: &NmKeypair,
        chip: &MacChipInfo,
        cfg: &AgentConfig,
        runtimes: &[String],
    ) -> Result<String> {
        let chip_proto = ProtoMacChipInfo {
            chip_model:       chip.chip_model.clone(),
            unified_memory_gb: chip.unified_memory_gb,
            gpu_cores:        chip.gpu_cores,
            cpu_cores:        chip.cpu_cores,
            metal_version:    chip.metal_version.clone(),
            serial_number:    chip.serial_number.clone(),
            platform_uuid:    chip.platform_uuid.clone(),
        };

        let capabilities = ProviderCapabilities {
            chip:                    Some(chip_proto),
            installed_runtimes:      runtimes.to_vec(),
            max_job_ram_gb:          cfg.max_job_ram_gb.unwrap_or(chip.unified_memory_gb.saturating_sub(4)),
            bandwidth_mbps:          100, // TODO: measure actual bandwidth
            region:                  cfg.region.clone(),
            floor_price_nmc_per_hour: cfg.floor_price_nmc_per_hour,
            wireguard_public_key:    String::new(), // Filled per-job
            ipv4_address:            String::new(), // Filled by coordinator
            port:                    cfg.wireguard_listen_port as u32,
        };

        // Sign the capabilities for attestation
        let cap_bytes = serde_json::to_vec(&chip.serial_number)?;
        let sig = keypair.sign(&cap_bytes);

        let req = RegisterProviderRequest {
            capabilities: Some(capabilities),
            provider_id:  keypair.public_key_hex(),
            attestation_signature: sig,
            nm_version:   env!("CARGO_PKG_VERSION").to_string(),
        };

        let response = self.client()
            .register_provider(req)
            .await
            .context("RegisterProvider RPC")?
            .into_inner();

        if response.status != "ok" {
            anyhow::bail!("Registration rejected: {}", response.message);
        }

        info!(provider_id = %keypair.public_key_hex(), "Provider registered successfully");
        Ok(response.assigned_relay)
    }

    /// Send a heartbeat to the coordinator.
    pub async fn heartbeat(
        &self,
        state: &str,
        gpu_util_pct: f32,
        ram_used_gb: u32,
        active_job_id: Option<&str>,
        uptime_secs: u64,
    ) -> Result<HeartbeatResponse> {
        use prost_types::Timestamp;
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let ts  = Timestamp {
            seconds: now.as_secs() as i64,
            nanos: now.subsec_nanos() as i32,
        };

        let hb = ProviderHeartbeat {
            provider_id:    self.provider_id.clone(),
            state:          state.to_string(),
            gpu_util_pct,
            ram_used_gb,
            active_job_id:  active_job_id.unwrap_or("").to_string(),
            uptime_seconds: uptime_secs,
            timestamp:      Some(ts),
        };

        let resp = self.client()
            .send_heartbeat(hb)
            .await
            .context("Heartbeat RPC")?
            .into_inner();

        debug!(ok = resp.ok, "Heartbeat sent");
        Ok(resp)
    }

    /// Report job completion to the coordinator.
    pub async fn report_completion(
        &self,
        job_id: &str,
        exit_code: i32,
        output_hash: &str,
        actual_runtime_s: u64,
        avg_gpu_util_pct: f32,
        peak_ram_gb: u32,
        provider_signature: Vec<u8>,
    ) -> Result<JobCompleteAck> {
        let msg = JobComplete {
            job_id:           job_id.to_string(),
            provider_id:      self.provider_id.clone(),
            exit_code,
            output_hash:      output_hash.to_string(),
            actual_runtime_s,
            avg_gpu_util_pct,
            peak_ram_gb,
            compute_proof:    vec![],  // Phase 3: ZK proof
            provider_signature,
        };

        let ack = self.client()
            .report_completion(msg)
            .await
            .context("ReportCompletion RPC")?
            .into_inner();

        info!(
            job_id,
            credits_earned = ack.credits_earned,
            "Job completion reported"
        );
        Ok(ack)
    }

    pub fn with_provider_id(mut self, id: String) -> Self {
        self.provider_id = id;
        self
    }
}
