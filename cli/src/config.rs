use anyhow::Result;
use serde::{Deserialize, Serialize};

const DEFAULT_COORDINATOR: &str = "https://neuralmesh-coordinator-production-666f.up.railway.app";
const DEFAULT_LEDGER: &str      = "https://neuralmesh-ledger-production-9e83.up.railway.app";

#[derive(Debug, Serialize, Deserialize)]
pub struct CliConfig {
    pub coordinator_url: String,
    pub ledger_url: String,
    pub account_id: Option<String>,
}

impl CliConfig {
    pub fn load(coordinator_override: Option<&str>, ledger_override: Option<&str>) -> Result<Self> {
        let path = config_path();
        let mut cfg = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            toml::from_str::<CliConfig>(&content)
                .unwrap_or_else(|_| Self::default())
        } else {
            Self::default()
        };

        if let Some(c) = coordinator_override { cfg.coordinator_url = c.to_string(); }
        if let Some(l) = ledger_override      { cfg.ledger_url      = l.to_string(); }
        if let Ok(id) = std::env::var("NM_ACCOUNT_ID") { cfg.account_id = Some(id); }

        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            coordinator_url: DEFAULT_COORDINATOR.into(),
            ledger_url:      DEFAULT_LEDGER.into(),
            account_id:      None,
        }
    }
}

fn config_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("hatch")
        .join("cli.toml")
}
